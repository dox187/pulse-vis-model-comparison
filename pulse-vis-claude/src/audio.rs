//! PulseAudio capture backend. Works against a real PulseAudio daemon and against
//! PipeWire's PulseAudio compatibility server.
//!
//! One dedicated thread owns the PulseAudio main loop, the context and the record stream.
//! It publishes samples into a shared ring buffer and status into a shared struct; the UI
//! thread sends commands through a channel. Per-application capture uses
//! `pa_stream_set_monitor_stream`, the same mechanism pavucontrol uses for its per-stream
//! meters, so no audio is re-routed.

use crate::config::AudioConfig;
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{ServerInfo, SinkInfo, SinkInputInfo, SourceInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet, Operation as SubOp};
use pulse::context::{Context, FlagSet as CtxFlags, State as CtxState};
use pulse::def::BufferAttr;
use pulse::mainloop::standard::Mainloop;
use pulse::proplist::{Proplist, properties};
use pulse::sample::{Format, Spec};
use pulse::stream::{FlagSet as StreamFlags, PeekResult, State as StreamState, Stream};
use pulse::time::MicroSeconds;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What to capture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Target {
    /// Capture nothing (used for `--list`).
    None,
    /// The monitor of the default sink: everything that is being played.
    #[default]
    All,
    /// The monitor of a specific sink, by name or description substring.
    Sink(String),
    /// A capture source such as a microphone, by name.
    Source(String),
    /// A single application's playback stream. `index` pins a specific stream while it
    /// exists; `name` is used to find it again after it restarts.
    App { name: String, index: Option<u32> },
}

impl Target {
    /// Parses the `audio.source` configuration string.
    pub fn parse(spec: &str) -> Target {
        let s = spec.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("all") {
            return Target::All;
        }
        if let Some(rest) = s.strip_prefix("app:") {
            return Target::App { name: rest.trim().to_string(), index: None };
        }
        if let Some(rest) = s.strip_prefix("sink:") {
            return Target::Sink(rest.trim().to_string());
        }
        if let Some(rest) = s.strip_prefix("source:") {
            return Target::Source(rest.trim().to_string());
        }
        Target::App { name: s.to_string(), index: None }
    }

    /// Inverse of [`Target::parse`], for saving the configuration.
    pub fn to_spec(&self) -> String {
        match self {
            Target::None | Target::All => "all".into(),
            Target::Sink(n) => format!("sink:{n}"),
            Target::Source(n) => format!("source:{n}"),
            Target::App { name, .. } => format!("app:{name}"),
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            Target::None => "nothing".into(),
            Target::All => "All output".into(),
            Target::Sink(n) => format!("Sink {n}"),
            Target::Source(n) => format!("Source {n}"),
            Target::App { name, .. } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Connection {
    #[default]
    Connecting,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Capture {
    #[default]
    Idle,
    Connecting(String),
    Streaming(String),
    Waiting(String),
    Error(String),
}

/// A playback stream of an application (a PulseAudio "sink input").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppStream {
    pub index: u32,
    pub sink: u32,
    pub app_name: String,
    pub media_name: String,
    pub binary: String,
    pub corked: bool,
    pub muted: bool,
}

impl AppStream {
    fn from_info(i: &SinkInputInfo) -> AppStream {
        let p = &i.proplist;
        let stream_name = i.name.as_deref().unwrap_or("").to_string();
        let app_name = p
            .get_str(properties::APPLICATION_NAME)
            .filter(|s| !s.is_empty())
            .or_else(|| p.get_str(properties::APPLICATION_PROCESS_BINARY))
            .unwrap_or_else(|| if stream_name.is_empty() { "unknown".into() } else { stream_name.clone() });
        let media_name = p
            .get_str(properties::MEDIA_NAME)
            .filter(|s| !s.is_empty())
            .unwrap_or(stream_name);
        AppStream {
            index: i.index,
            sink: i.sink,
            app_name,
            media_name,
            binary: p.get_str(properties::APPLICATION_PROCESS_BINARY).unwrap_or_default(),
            corked: i.corked,
            muted: i.mute,
        }
    }

    /// Case-insensitive substring match against the application name, binary or stream title.
    pub fn matches(&self, needle: &str) -> bool {
        let n = needle.to_lowercase();
        if n.is_empty() {
            return false;
        }
        self.app_name.to_lowercase().contains(&n)
            || self.binary.to_lowercase().contains(&n)
            || self.media_name.to_lowercase().contains(&n)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SinkEntry {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub monitor: String,
}

/// A capture source. Monitors of sinks are included (flagged) so explicit names resolve.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputEntry {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub is_monitor: bool,
}

impl InputEntry {
    pub fn matches(&self, needle: &str) -> bool {
        let n = needle.to_lowercase();
        !n.is_empty() && (self.name.to_lowercase().contains(&n) || self.description.to_lowercase().contains(&n))
    }
}

/// Snapshot of the backend state for the UI.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub connection: Connection,
    pub server_name: String,
    pub default_sink: String,
    pub sinks: Vec<SinkEntry>,
    pub inputs: Vec<InputEntry>,
    pub inputs_loaded: bool,
    pub default_source: String,
    pub apps: Vec<AppStream>,
    pub apps_loaded: bool,
    pub target: Target,
    pub capture: Capture,
    pub frames: u64,
}

/// Stereo sample history.
pub struct Ring {
    l: VecDeque<f32>,
    r: VecDeque<f32>,
    cap: usize,
    pub total: u64,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        Ring { l: VecDeque::with_capacity(cap), r: VecDeque::with_capacity(cap), cap, total: 0 }
    }

    pub fn push_interleaved(&mut self, data: &[f32]) {
        for fr in data.as_chunks::<2>().0 {
            if self.l.len() == self.cap {
                self.l.pop_front();
                self.r.pop_front();
            }
            self.l.push_back(fr[0]);
            self.r.push_back(fr[1]);
        }
        self.total += (data.len() / 2) as u64;
    }

    /// Copies the newest `n` frames into `out_l`/`out_r`, zero-padding at the front if fewer exist.
    pub fn latest(&self, n: usize, out_l: &mut Vec<f32>, out_r: &mut Vec<f32>) {
        out_l.clear();
        out_r.clear();
        let avail = self.l.len().min(n);
        out_l.resize(n - avail, 0.0);
        out_r.resize(n - avail, 0.0);
        out_l.extend(self.l.range(self.l.len() - avail..));
        out_r.extend(self.r.range(self.r.len() - avail..));
    }
}

pub struct Shared {
    pub ring: Mutex<Ring>,
    pub status: Mutex<Status>,
}

enum Cmd {
    SetTarget(Target),
    Refresh,
    Quit,
}

/// Owner handle of the audio thread. Dropping it stops the thread.
pub struct AudioHandle {
    pub shared: Arc<Shared>,
    tx: Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

impl AudioHandle {
    pub fn start(cfg: &AudioConfig, target: Target) -> AudioHandle {
        let rate = cfg.sample_rate;
        let shared = Arc::new(Shared {
            ring: Mutex::new(Ring::new(rate as usize * 2)),
            status: Mutex::new(Status { target: target.clone(), ..Default::default() }),
        });
        let (tx, rx) = mpsc::channel();
        let server = if cfg.server.trim().is_empty() { None } else { Some(cfg.server.trim().to_string()) };
        let frag_bytes = (rate as u64 * 2 * 4 * cfg.buffer_ms.clamp(1, 500) as u64 / 1000).max(256) as u32;
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("pulse-capture".into())
            .spawn(move || {
                let Some(mainloop) = Mainloop::new() else {
                    thread_shared.status.lock().unwrap().connection =
                        Connection::Failed("cannot create PulseAudio main loop".into());
                    return;
                };
                Backend {
                    stream: None,
                    ctx: None,
                    mainloop,
                    shared: thread_shared,
                    rx,
                    inbox: Rc::new(RefCell::new(Inbox::default())),
                    target,
                    spec: Spec { format: Format::F32le, channels: 2, rate },
                    frag_bytes,
                    server,
                    key: None,
                    ctx_ready: false,
                    inflight: Inflight::default(),
                    want: Inflight::default(),
                    next_connect: Instant::now(),
                    next_stream_retry: Instant::now(),
                    default_sink: String::new(),
                    sinks: Vec::new(),
                    inputs: Vec::new(),
                    inputs_loaded: false,
                    apps: Vec::new(),
                    scratch: Vec::with_capacity(1 << 14),
                }
                .run();
            })
            .expect("spawn audio thread");
        AudioHandle { shared, tx, thread: Some(thread) }
    }

    pub fn set_target(&self, t: Target) {
        let _ = self.tx.send(Cmd::SetTarget(t));
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Cmd::Refresh);
    }

    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    /// Newest `n` frames; returns the total number of frames captured so far.
    pub fn snapshot(&self, n: usize, l: &mut Vec<f32>, r: &mut Vec<f32>) -> u64 {
        let ring = self.shared.ring.lock().unwrap();
        ring.latest(n, l, r);
        ring.total
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Results delivered by PulseAudio callbacks, consumed by the backend loop.
#[derive(Default)]
struct Inbox {
    apps_acc: Vec<AppStream>,
    apps: Option<Vec<AppStream>>,
    sinks_acc: Vec<SinkEntry>,
    sinks: Option<Vec<SinkEntry>>,
    inputs_acc: Vec<InputEntry>,
    inputs: Option<Vec<InputEntry>>,
    /// (default sink, default source, server name)
    server: Option<(String, String, String)>,
    events: Vec<(Option<Facility>, Option<SubOp>, u32)>,
}

#[derive(Default, Clone, Copy)]
struct Inflight {
    apps: bool,
    sinks: bool,
    inputs: bool,
    server: bool,
}

impl Inflight {
    const ALL: Inflight = Inflight { apps: true, sinks: true, inputs: true, server: true };
}

/// Identifies a connected record stream.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamKey {
    source: String,
    sink_input: Option<u32>,
    label: String,
}

enum Resolved {
    Key(StreamKey),
    Waiting(String),
    Nothing,
}

// Field order matters: the stream must be dropped before the context, the context before the loop.
struct Backend {
    stream: Option<Stream>,
    ctx: Option<Context>,
    mainloop: Mainloop,
    shared: Arc<Shared>,
    rx: Receiver<Cmd>,
    inbox: Rc<RefCell<Inbox>>,
    target: Target,
    spec: Spec,
    frag_bytes: u32,
    server: Option<String>,
    key: Option<StreamKey>,
    ctx_ready: bool,
    inflight: Inflight,
    want: Inflight,
    next_connect: Instant,
    next_stream_retry: Instant,
    default_sink: String,
    sinks: Vec<SinkEntry>,
    inputs: Vec<InputEntry>,
    inputs_loaded: bool,
    apps: Vec<AppStream>,
    scratch: Vec<f32>,
}

impl Backend {
    fn run(mut self) {
        loop {
            if self.ctx.is_none() && Instant::now() >= self.next_connect {
                self.connect_context();
            }
            // One main-loop iteration with a bounded wait so commands stay responsive.
            match self.mainloop.prepare(Some(MicroSeconds(15_000))) {
                Ok(()) => {
                    let _ = self.mainloop.poll();
                    let _ = self.mainloop.dispatch();
                }
                Err(_) => std::thread::sleep(Duration::from_millis(15)),
            }
            self.poll_context();
            self.drain_stream();
            self.process_inbox();
            self.issue_requests();
            if self.ctx_ready && self.stream.is_none() && Instant::now() >= self.next_stream_retry {
                self.next_stream_retry = Instant::now() + Duration::from_secs(1);
                self.reconcile();
            }
            if !self.process_cmds() {
                break;
            }
        }
        self.drop_stream(Capture::Idle);
        if let Some(mut ctx) = self.ctx.take() {
            ctx.disconnect();
        }
    }

    fn set_connection(&self, c: Connection) {
        self.shared.status.lock().unwrap().connection = c;
    }

    fn set_capture(&self, c: Capture) {
        self.shared.status.lock().unwrap().capture = c;
    }

    fn connect_context(&mut self) {
        self.next_connect = Instant::now() + Duration::from_secs(2);
        let mut proplist = Proplist::new().expect("proplist");
        let _ = proplist.set_str(properties::APPLICATION_NAME, "pulse-vis-claude");
        let _ = proplist.set_str(properties::APPLICATION_ID, "dev.pulse-vis-claude");
        let _ = proplist.set_str(properties::APPLICATION_ICON_NAME, "audio-x-generic");
        let Some(mut ctx) = Context::new_with_proplist(&self.mainloop, "pulse-vis-claude", &proplist) else {
            self.set_connection(Connection::Failed("cannot create PulseAudio context".into()));
            return;
        };
        let inbox = self.inbox.clone();
        ctx.set_subscribe_callback(Some(Box::new(move |facility, op, index| {
            inbox.borrow_mut().events.push((facility, op, index));
        })));
        match ctx.connect(self.server.as_deref(), CtxFlags::NOFAIL, None) {
            Ok(()) => {
                self.ctx = Some(ctx);
                self.ctx_ready = false;
                self.set_connection(Connection::Connecting);
            }
            Err(e) => self.set_connection(Connection::Failed(format!("connect: {e}"))),
        }
    }

    fn poll_context(&mut self) {
        let Some(state) = self.ctx.as_ref().map(|c| c.get_state()) else { return };
        match state {
            CtxState::Ready if !self.ctx_ready => {
                self.ctx_ready = true;
                self.set_connection(Connection::Ready);
                if let Some(ctx) = self.ctx.as_mut() {
                    let mask = InterestMaskSet::SINK_INPUT
                        | InterestMaskSet::SINK
                        | InterestMaskSet::SOURCE
                        | InterestMaskSet::SERVER;
                    let _ = ctx.subscribe(mask, |_| {});
                }
                self.want = Inflight::ALL;
                self.next_stream_retry = Instant::now();
            }
            CtxState::Failed | CtxState::Terminated => {
                let err = self.ctx.as_ref().and_then(|c| c.errno().to_string()).unwrap_or_else(|| "unknown error".into());
                self.drop_stream(Capture::Error("connection to the audio server lost".into()));
                self.ctx = None;
                self.ctx_ready = false;
                self.inflight = Inflight::default();
                self.set_connection(Connection::Failed(err));
                self.next_connect = Instant::now() + Duration::from_secs(1);
            }
            _ => {}
        }
    }

    fn issue_requests(&mut self) {
        if !self.ctx_ready {
            return;
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        let introspect = ctx.introspect();
        if self.want.server && !self.inflight.server {
            self.want.server = false;
            self.inflight.server = true;
            let inbox = self.inbox.clone();
            introspect.get_server_info(move |info: &ServerInfo| {
                inbox.borrow_mut().server = Some((
                    info.default_sink_name.as_deref().unwrap_or("").to_string(),
                    info.default_source_name.as_deref().unwrap_or("").to_string(),
                    info.server_name.as_deref().unwrap_or("").to_string(),
                ));
            });
        }
        if self.want.sinks && !self.inflight.sinks {
            self.want.sinks = false;
            self.inflight.sinks = true;
            let inbox = self.inbox.clone();
            introspect.get_sink_info_list(move |res: ListResult<&SinkInfo>| match res {
                ListResult::Item(s) => inbox.borrow_mut().sinks_acc.push(SinkEntry {
                    index: s.index,
                    name: s.name.as_deref().unwrap_or("").to_string(),
                    description: s.description.as_deref().unwrap_or("").to_string(),
                    monitor: s.monitor_source_name.as_deref().unwrap_or("").to_string(),
                }),
                ListResult::End | ListResult::Error => {
                    let mut ib = inbox.borrow_mut();
                    let acc = std::mem::take(&mut ib.sinks_acc);
                    ib.sinks = Some(acc);
                }
            });
        }
        if self.want.inputs && !self.inflight.inputs {
            self.want.inputs = false;
            self.inflight.inputs = true;
            let inbox = self.inbox.clone();
            introspect.get_source_info_list(move |res: ListResult<&SourceInfo>| match res {
                ListResult::Item(i) => inbox.borrow_mut().inputs_acc.push(InputEntry {
                    index: i.index,
                    name: i.name.as_deref().unwrap_or("").to_string(),
                    description: i.description.as_deref().unwrap_or("").to_string(),
                    is_monitor: i.monitor_of_sink.is_some(),
                }),
                ListResult::End | ListResult::Error => {
                    let mut ib = inbox.borrow_mut();
                    let acc = std::mem::take(&mut ib.inputs_acc);
                    ib.inputs = Some(acc);
                }
            });
        }
        if self.want.apps && !self.inflight.apps {
            self.want.apps = false;
            self.inflight.apps = true;
            let inbox = self.inbox.clone();
            introspect.get_sink_input_info_list(move |res: ListResult<&SinkInputInfo>| match res {
                ListResult::Item(i) => inbox.borrow_mut().apps_acc.push(AppStream::from_info(i)),
                ListResult::End | ListResult::Error => {
                    let mut ib = inbox.borrow_mut();
                    let acc = std::mem::take(&mut ib.apps_acc);
                    ib.apps = Some(acc);
                }
            });
        }
    }

    fn process_inbox(&mut self) {
        let (apps, sinks, inputs, server, events) = {
            let mut ib = self.inbox.borrow_mut();
            (ib.apps.take(), ib.sinks.take(), ib.inputs.take(), ib.server.take(), std::mem::take(&mut ib.events))
        };
        let mut changed = false;
        for (facility, op, index) in events {
            match facility {
                Some(Facility::SinkInput) => {
                    self.want.apps = true;
                    if op == Some(SubOp::Removed)
                        && self.key.as_ref().and_then(|k| k.sink_input) == Some(index)
                    {
                        self.drop_stream(Capture::Waiting("stream ended".into()));
                        changed = true;
                    }
                }
                Some(Facility::Sink) => self.want.sinks = true,
                Some(Facility::Source) => self.want.inputs = true,
                Some(Facility::Server) => self.want.server = true,
                _ => {}
            }
        }
        if let Some((default_sink, default_source, server_name)) = server {
            self.inflight.server = false;
            if default_sink != self.default_sink {
                self.default_sink = default_sink.clone();
                changed = true;
            }
            let mut st = self.shared.status.lock().unwrap();
            st.default_sink = default_sink;
            st.default_source = default_source;
            st.server_name = server_name;
        }
        if let Some(inputs) = inputs {
            self.inflight.inputs = false;
            if inputs != self.inputs || !self.inputs_loaded {
                self.inputs = inputs.clone();
                changed = true;
            }
            self.inputs_loaded = true;
            let mut st = self.shared.status.lock().unwrap();
            st.inputs = inputs;
            st.inputs_loaded = true;
        }
        if let Some(sinks) = sinks {
            self.inflight.sinks = false;
            if sinks != self.sinks {
                self.sinks = sinks.clone();
                changed = true;
            }
            self.shared.status.lock().unwrap().sinks = sinks;
        }
        if let Some(apps) = apps {
            self.inflight.apps = false;
            if apps != self.apps {
                self.apps = apps.clone();
                changed = true;
            }
            let mut st = self.shared.status.lock().unwrap();
            st.apps = apps;
            st.apps_loaded = true;
        }
        if changed {
            self.reconcile();
        }
    }

    fn monitor_of(&self, sink_name: &str) -> String {
        self.sinks
            .iter()
            .find(|s| s.name == sink_name)
            .filter(|s| !s.monitor.is_empty())
            .map(|s| s.monitor.clone())
            .unwrap_or_else(|| format!("{sink_name}.monitor"))
    }

    fn resolve(&mut self) -> Resolved {
        match self.target.clone() {
            Target::None => Resolved::Nothing,
            Target::All => {
                if self.default_sink.is_empty() {
                    return Resolved::Waiting("waiting for the default output".into());
                }
                let desc = self
                    .sinks
                    .iter()
                    .find(|s| s.name == self.default_sink)
                    .map(|s| s.description.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| self.default_sink.clone());
                Resolved::Key(StreamKey {
                    source: self.monitor_of(&self.default_sink),
                    sink_input: None,
                    label: format!("All output · {desc}"),
                })
            }
            Target::Sink(name) => {
                let needle = name.to_lowercase();
                match self.sinks.iter().find(|s| {
                    s.name.to_lowercase().contains(&needle) || s.description.to_lowercase().contains(&needle)
                }) {
                    Some(s) => Resolved::Key(StreamKey {
                        source: if s.monitor.is_empty() { format!("{}.monitor", s.name) } else { s.monitor.clone() },
                        sink_input: None,
                        label: format!("Output · {}", if s.description.is_empty() { &s.name } else { &s.description }),
                    }),
                    None => Resolved::Waiting(format!("output '{name}' not found")),
                }
            }
            Target::Source(name) => {
                if !self.inputs_loaded {
                    return Resolved::Waiting("resolving input".into());
                }
                // Exact name first, then a substring of the name or description.
                let found = self
                    .inputs
                    .iter()
                    .find(|i| i.name == name)
                    .or_else(|| self.inputs.iter().find(|i| !i.is_monitor && i.matches(&name)))
                    .or_else(|| self.inputs.iter().find(|i| i.matches(&name)));
                match found {
                    Some(i) => Resolved::Key(StreamKey {
                        source: i.name.clone(),
                        sink_input: None,
                        label: format!("Input · {}", if i.description.is_empty() { &i.name } else { &i.description }),
                    }),
                    None => Resolved::Waiting(format!("input '{name}' not found")),
                }
            }
            Target::App { name, index } => {
                let found = index
                    .and_then(|i| self.apps.iter().find(|a| a.index == i))
                    .or_else(|| self.apps.iter().find(|a| a.matches(&name) && !a.corked))
                    .or_else(|| self.apps.iter().find(|a| a.matches(&name)))
                    .cloned();
                let Some(app) = found else {
                    if let Target::App { index, .. } = &mut self.target {
                        *index = None;
                    }
                    return Resolved::Waiting(format!("waiting for '{name}' to play audio"));
                };
                if let Target::App { index, .. } = &mut self.target {
                    *index = Some(app.index);
                }
                match self.sinks.iter().find(|s| s.index == app.sink) {
                    Some(s) if !s.monitor.is_empty() => Resolved::Key(StreamKey {
                        source: s.monitor.clone(),
                        sink_input: Some(app.index),
                        label: if app.media_name.is_empty() || app.media_name == app.app_name {
                            app.app_name.clone()
                        } else {
                            format!("{} · {}", app.app_name, app.media_name)
                        },
                    }),
                    _ => {
                        self.want.sinks = true;
                        Resolved::Waiting("resolving the application's output".into())
                    }
                }
            }
        }
    }

    fn reconcile(&mut self) {
        if !self.ctx_ready {
            return;
        }
        match self.resolve() {
            Resolved::Key(key) => {
                let same = self.key.as_ref() == Some(&key);
                let alive = self
                    .stream
                    .as_ref()
                    .map(|s| matches!(s.get_state(), StreamState::Ready | StreamState::Creating))
                    .unwrap_or(false);
                if !(same && alive) {
                    self.drop_stream(Capture::Idle);
                    self.connect_stream(key);
                }
            }
            Resolved::Waiting(msg) => self.drop_stream(Capture::Waiting(msg)),
            Resolved::Nothing => self.drop_stream(Capture::Idle),
        }
    }

    fn connect_stream(&mut self, key: StreamKey) {
        let result: Result<Stream, String> = (|| {
            let ctx = self.ctx.as_mut().ok_or("no context")?;
            let mut stream =
                Stream::new(ctx, "pulse-vis-claude capture", &self.spec, None).ok_or("cannot create stream")?;
            if let Some(idx) = key.sink_input {
                stream.set_monitor_stream(idx).map_err(|e| format!("monitor stream: {e}"))?;
            }
            let attr = BufferAttr {
                maxlength: u32::MAX,
                tlength: u32::MAX,
                prebuf: u32::MAX,
                minreq: u32::MAX,
                fragsize: self.frag_bytes,
            };
            stream
                .connect_record(
                    Some(&key.source),
                    Some(&attr),
                    StreamFlags::ADJUST_LATENCY | StreamFlags::DONT_MOVE,
                )
                .map_err(|e| format!("record: {e}"))?;
            Ok(stream)
        })();
        match result {
            Ok(stream) => {
                self.set_capture(Capture::Connecting(key.label.clone()));
                self.stream = Some(stream);
                self.key = Some(key);
            }
            Err(e) => {
                self.set_capture(Capture::Error(e));
                self.next_stream_retry = Instant::now() + Duration::from_secs(1);
            }
        }
    }

    fn drop_stream(&mut self, capture: Capture) {
        if let Some(mut s) = self.stream.take() {
            let _ = s.disconnect();
        }
        self.key = None;
        self.set_capture(capture);
    }

    fn drain_stream(&mut self) {
        let state = match self.stream.as_ref() {
            Some(s) => s.get_state(),
            None => return,
        };
        match state {
            StreamState::Ready => {}
            StreamState::Failed | StreamState::Terminated => {
                let err = self.ctx.as_ref().and_then(|c| c.errno().to_string()).unwrap_or_else(|| "unknown error".into());
                self.drop_stream(Capture::Error(format!("stream failed: {err}")));
                self.next_stream_retry = Instant::now() + Duration::from_secs(1);
                return;
            }
            _ => return,
        }
        let Some(stream) = self.stream.as_mut() else { return };
        self.scratch.clear();
        loop {
            match stream.peek() {
                Ok(PeekResult::Data(bytes)) => {
                    self.scratch.extend(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
                    if stream.discard().is_err() {
                        break;
                    }
                }
                Ok(PeekResult::Hole(n)) => {
                    self.scratch.extend(std::iter::repeat_n(0.0, (n / 4) & !1));
                    if stream.discard().is_err() {
                        break;
                    }
                }
                Ok(PeekResult::Empty) | Err(_) => break,
            }
            if self.scratch.len() > 1 << 20 {
                break;
            }
        }
        if self.scratch.is_empty() {
            return;
        }
        let total = {
            let mut ring = self.shared.ring.lock().unwrap();
            ring.push_interleaved(&self.scratch);
            ring.total
        };
        let mut st = self.shared.status.lock().unwrap();
        st.frames = total;
        if let Capture::Connecting(label) = &st.capture {
            st.capture = Capture::Streaming(label.clone());
        }
    }

    /// Returns false when the thread should exit.
    fn process_cmds(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(Cmd::SetTarget(t)) => {
                    self.target = t.clone();
                    self.shared.status.lock().unwrap().target = t;
                    self.want.apps = true;
                    self.want.sinks = true;
                    self.want.inputs = true;
                    self.reconcile();
                }
                Ok(Cmd::Refresh) => self.want = Inflight::ALL,
                Ok(Cmd::Quit) | Err(TryRecvError::Disconnected) => return false,
                Err(TryRecvError::Empty) => return true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_targets() {
        assert_eq!(Target::parse("all"), Target::All);
        assert_eq!(Target::parse(""), Target::All);
        assert_eq!(Target::parse("app:Firefox"), Target::App { name: "Firefox".into(), index: None });
        assert_eq!(Target::parse("mpv"), Target::App { name: "mpv".into(), index: None });
        assert_eq!(Target::parse("sink:hdmi"), Target::Sink("hdmi".into()));
        assert_eq!(Target::parse("source:mic"), Target::Source("mic".into()));
        assert_eq!(Target::parse("app:Firefox").to_spec(), "app:Firefox");
    }

    #[test]
    fn ring_latest_pads() {
        let mut ring = Ring::new(8);
        ring.push_interleaved(&[1.0, -1.0, 2.0, -2.0]);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        ring.latest(4, &mut l, &mut r);
        assert_eq!(l, vec![0.0, 0.0, 1.0, 2.0]);
        assert_eq!(r, vec![0.0, 0.0, -1.0, -2.0]);
        ring.push_interleaved(&[3.0; 40]);
        ring.latest(3, &mut l, &mut r);
        assert_eq!(l, vec![3.0, 3.0, 3.0]);
        assert_eq!(ring.total, 22);
    }

    #[test]
    fn app_matching() {
        let a = AppStream { app_name: "Firefox".into(), binary: "firefox-bin".into(), media_name: "YouTube".into(), ..Default::default() };
        assert!(a.matches("fire"));
        assert!(a.matches("youtube"));
        assert!(!a.matches("mpv"));
        assert!(!a.matches(""));
    }
}
