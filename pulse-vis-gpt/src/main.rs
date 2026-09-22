mod audio;
mod config;
mod dsp;
mod ui;

use anyhow::{Result, bail};
use clap::Parser;
use config::{Config, Mode, Quality};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::{
    collections::VecDeque,
    io::IsTerminal,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "PulseAudio / PipeWire terminal audio visualizer",
    after_help = "Keys: 1–6 modes · a sources · s settings · +/- gain · ? help · q quit"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_enum)]
    mode: Option<Mode>,
    #[arg(long, value_enum)]
    quality: Option<Quality>,
    #[arg(long)]
    fps: Option<u16>,
    #[arg(long)]
    sensitivity: Option<f32>,
    /// Case-insensitive application name/binary substring; repeat to combine apps
    #[arg(long, conflicts_with = "sink")]
    app: Vec<String>,
    /// Capture just this output's symbolic sink name (default: all outputs)
    #[arg(long)]
    sink: Option<String>,
    /// Generate a stereo demo without connecting to an audio server
    #[arg(long)]
    demo: bool,
    /// Print available outputs and playback applications
    #[arg(long)]
    list_sources: bool,
    /// Write a default config to --config or the XDG config path, then exit
    #[arg(long)]
    init_config: bool,
    /// Capture for this many seconds without a TUI and report signal levels
    #[arg(long, value_parser=clap::value_parser!(u64).range(1..=60))]
    check: Option<u64>,
}
#[derive(Clone)]
enum SourceKind {
    All,
    Sink(String),
    App(String),
}
struct SourceOption {
    label: String,
    selected: bool,
    kind: SourceKind,
}
pub struct App {
    config: Config,
    config_path: PathBuf,
    demo: bool,
    paused: bool,
    help: bool,
    sources: bool,
    settings: bool,
    source_cursor: usize,
    settings_cursor: usize,
    inventory: audio::Inventory,
    audio: audio::Audio,
    analysis: dsp::Analysis,
    samples: Vec<audio::Sample>,
    history: VecDeque<Vec<f32>>,
    notice: String,
    server_error: Option<String>,
    last_history: Instant,
}
impl App {
    fn new(config: Config, config_path: PathBuf, demo: bool) -> Self {
        let analysis = dsp::Analysis::new(config.quality.size());
        Self {
            config,
            config_path,
            demo,
            paused: false,
            help: false,
            sources: false,
            settings: false,
            source_cursor: 0,
            settings_cursor: 0,
            inventory: audio::Inventory::default(),
            audio: audio::Audio::default(),
            analysis,
            samples: vec![],
            history: VecDeque::new(),
            notice: String::new(),
            server_error: None,
            last_history: Instant::now(),
        }
    }
    fn source_label(&self) -> String {
        if self.demo {
            "Stereo demo".into()
        } else if !self.config.apps.is_empty() {
            self.config.apps.join(" + ")
        } else if let Some(s) = &self.config.sink {
            self.inventory
                .sinks
                .iter()
                .find(|i| &i.name == s)
                .map(|i| i.description.clone())
                .unwrap_or_else(|| s.clone())
        } else {
            "All outputs".into()
        }
    }
    fn status_text(&self) -> String {
        if let Some(e) = self.server_error.as_ref().or(self.audio.error.as_ref()) {
            return format!(" {e} · retrying every 2s");
        }
        if !self.notice.is_empty() {
            return format!(" {}", self.notice);
        }
        if !self.demo && self.audio.count() == 0 {
            return " Waiting for matching playback sources…  a to select sources".into();
        }
        format!(
            " {} capture{} · stereo 48 kHz · peak {:5.1} dBFS · {}",
            self.audio.count(),
            if self.audio.count() == 1 { "" } else { "s" },
            dsp::db(self.analysis.peak[0].max(self.analysis.peak[1])),
            if self.analysis.peak.iter().all(|p| *p < 0.00001) {
                "silence"
            } else {
                "receiving audio"
            }
        )
    }
    fn source_options(&self) -> Vec<SourceOption> {
        let mut options = vec![SourceOption {
            label: "All outputs / combined system audio".into(),
            selected: self.config.apps.is_empty() && self.config.sink.is_none(),
            kind: SourceKind::All,
        }];
        for sink in &self.inventory.sinks {
            options.push(SourceOption {
                label: format!("Output · {}", sink.description),
                selected: self.config.apps.is_empty()
                    && self.config.sink.as_ref() == Some(&sink.name),
                kind: SourceKind::Sink(sink.name.clone()),
            });
        }
        let mut names: Vec<String> = self
            .inventory
            .inputs
            .iter()
            .map(|i| i.name().to_owned())
            .collect();
        names.extend(self.config.apps.iter().cloned());
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        for name in names {
            let matching: Vec<_> = self
                .inventory
                .inputs
                .iter()
                .filter(|i| i.matches(&name))
                .collect();
            let active = matching.iter().filter(|i| !i.corked).count();
            options.push(SourceOption {
                label: format!(
                    "App · {name}  ({} streams, {active} active)",
                    matching.len()
                ),
                selected: self
                    .config
                    .apps
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&name)),
                kind: SourceKind::App(name),
            });
        }
        options
    }
    fn reconcile(&mut self) {
        if !self.demo {
            self.audio.reconcile(&audio::targets(
                &self.inventory,
                &self.config.apps,
                self.config.sink.as_deref(),
            ));
        }
    }
    fn reset_view(&mut self) {
        self.history.clear();
        self.analysis = dsp::Analysis::new(self.config.quality.size());
        self.samples.clear();
    }
    fn choose_source(&mut self) {
        let options = self.source_options();
        if let Some(option) = options.get(self.source_cursor) {
            match &option.kind {
                SourceKind::All => {
                    self.config.apps.clear();
                    self.config.sink = None;
                }
                SourceKind::Sink(s) => {
                    self.config.apps.clear();
                    self.config.sink = Some(s.clone());
                }
                SourceKind::App(name) => {
                    self.config.sink = None;
                    if option.selected {
                        self.config.apps.retain(|a| !a.eq_ignore_ascii_case(name));
                    } else {
                        self.config.apps.push(name.clone());
                    }
                }
            }
        }
        self.reconcile();
        self.reset_view();
    }
    fn theme_step(&mut self, direction: i32) {
        let themes = ["aurora", "fire", "ocean", "mono", "custom"];
        let i = themes
            .iter()
            .position(|t| *t == self.config.theme)
            .unwrap_or(0);
        self.config.theme =
            themes[(i as i32 + direction).rem_euclid(themes.len() as i32) as usize].into();
    }
    fn setting(&mut self, direction: i32) {
        let c = &mut self.config;
        match self.settings_cursor {
            0 => {
                c.sensitivity =
                    (c.sensitivity * if direction > 0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.05, 20.0)
            }
            1 => c.smoothing = (c.smoothing + direction as f32 * 0.05).clamp(0.0, 0.98),
            2 => {
                let times = if direction > 0 { 1 } else { 3 };
                for _ in 0..times {
                    c.quality = c.quality.next();
                }
            }
            3 => c.fps = (c.fps as i32 + direction * 5).clamp(10, 144) as u16,
            4 => c.auto_gain = !c.auto_gain,
            5 => self.theme_step(direction),
            6 => c.bar_width = (c.bar_width as i32 + direction).clamp(1, 8) as u16,
            7 => c.bar_gap = (c.bar_gap as i32 + direction).clamp(0, 4) as u16,
            8 => c.show_peaks = !c.show_peaks,
            _ => {}
        }
    }
    fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        if key.code == KeyCode::Char('q')
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            return true;
        }
        self.notice.clear();
        if key.code == KeyCode::Esc {
            self.help = false;
            self.sources = false;
            self.settings = false;
            return false;
        }
        if self.sources {
            match key.code {
                KeyCode::Up => self.source_cursor = self.source_cursor.saturating_sub(1),
                KeyCode::Down => {
                    self.source_cursor =
                        (self.source_cursor + 1).min(self.source_options().len().saturating_sub(1))
                }
                KeyCode::Enter | KeyCode::Char(' ') => self.choose_source(),
                _ => {}
            }
        } else if self.settings {
            match key.code {
                KeyCode::Up => self.settings_cursor = self.settings_cursor.saturating_sub(1),
                KeyCode::Down => self.settings_cursor = (self.settings_cursor + 1).min(8),
                KeyCode::Left => self.setting(-1),
                KeyCode::Right | KeyCode::Enter => self.setting(1),
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('1'..='6') => {
                if let KeyCode::Char(n) = key.code {
                    self.config.mode = Mode::ALL[n as usize - '1' as usize];
                }
            }
            KeyCode::Tab => self.config.mode = self.config.mode.next(),
            KeyCode::Char('a') => {
                self.sources = !self.sources;
                self.settings = false;
                self.help = false;
            }
            KeyCode::Char('s') => {
                self.settings = !self.settings;
                self.sources = false;
                self.help = false;
            }
            KeyCode::Char('?') => {
                self.help = !self.help;
                self.settings = false;
                self.sources = false;
            }
            KeyCode::Char(' ') if !self.sources && !self.settings => self.paused = !self.paused,
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.config.sensitivity = (self.config.sensitivity * 1.1).min(20.0)
            }
            KeyCode::Char('-') => {
                self.config.sensitivity = (self.config.sensitivity / 1.1).max(0.05)
            }
            KeyCode::Char('[') => self.config.smoothing = (self.config.smoothing - 0.05).max(0.0),
            KeyCode::Char(']') => self.config.smoothing = (self.config.smoothing + 0.05).min(0.98),
            KeyCode::Char('g') => self.config.auto_gain = !self.config.auto_gain,
            KeyCode::Char('p') => self.theme_step(1),
            KeyCode::Char('Q') => self.config.quality = self.config.quality.next(),
            KeyCode::Char('w') => {
                self.notice = match self.config.save(&self.config_path) {
                    Ok(()) => format!("Saved {}", self.config_path.display()),
                    Err(e) => format!("Save failed: {e:#}"),
                }
            }
            KeyCode::Char('r') => match Config::load(&self.config_path) {
                Ok(c) => {
                    self.config = c;
                    self.reconcile();
                    self.reset_view();
                    self.notice = "Configuration reloaded".into();
                }
                Err(e) => self.notice = format!("Reload failed: {e:#}"),
            },
            _ => {}
        }
        false
    }
    fn tick(&mut self, width: u16, time: f32, dt: f32) {
        if self.paused {
            return;
        }
        self.samples = if self.demo {
            audio::demo(self.config.quality.size(), time)
        } else {
            self.audio.samples(self.config.quality.size())
        };
        let count = match self.config.mode {
            Mode::Spectrum => (width.saturating_sub(2)
                / (self.config.bar_width + self.config.bar_gap))
                .max(1) as usize,
            Mode::Radial => 96,
            _ => width.saturating_sub(2).clamp(32, 256) as usize,
        };
        self.analysis.update(&self.samples, count, &self.config, dt);
        if self.last_history.elapsed() >= Duration::from_millis(33) {
            self.history.push_back(self.analysis.bands.clone());
            while self.history.len() > 256 {
                self.history.pop_front();
            }
            self.last_history = Instant::now();
        }
    }
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = cli.config.unwrap_or_else(config::default_path);
    if cli.init_config {
        if path.exists() {
            bail!(
                "{} already exists; choose another --config path",
                path.display()
            );
        }
        Config::default().save(&path)?;
        println!("Created {}", path.display());
        return Ok(());
    }
    if cli.list_sources {
        let inv = audio::discover()?;
        println!("OUTPUTS");
        for s in &inv.sinks {
            println!("  {}  [{}]", s.description, s.name);
        }
        println!("APPLICATIONS");
        for i in &inv.inputs {
            println!(
                "  {}  [stream {}, sink {}, {}]",
                i.name(),
                i.index,
                i.sink,
                if i.corked { "paused" } else { "active" }
            );
        }
        return Ok(());
    }
    let mut config = Config::load(&path)?;
    if let Some(mode) = cli.mode {
        config.mode = mode;
    }
    if let Some(quality) = cli.quality {
        config.quality = quality;
    }
    if let Some(fps) = cli.fps {
        config.fps = fps;
    }
    if let Some(sensitivity) = cli.sensitivity {
        config.sensitivity = sensitivity;
    }
    if !cli.app.is_empty() {
        config.apps = cli.app;
        config.sink = None;
    }
    if let Some(sink) = cli.sink {
        config.sink = Some(sink);
        config.apps.clear();
    }
    config.validate()?;
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    let mut app = App::new(config, path, cli.demo);
    if let Some(seconds) = cli.check {
        return check(&mut app, seconds, &stop);
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!(
            "The TUI needs an interactive terminal. Use --check 3 or --demo --check 3 for diagnostics."
        );
    }
    let discovery = if cli.demo {
        None
    } else {
        Some(audio::Discovery::start())
    };
    // Ratatui installs a panic hook to restore the terminal; the guard also covers errors.
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    let start = Instant::now();
    let mut last = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let frame_start = Instant::now();
        if let Some(discovery) = &discovery {
            while let Ok(result) = discovery.rx.try_recv() {
                match result {
                    Ok(inv) => {
                        app.inventory = inv;
                        app.server_error = None;
                        app.source_cursor = app
                            .source_cursor
                            .min(app.source_options().len().saturating_sub(1));
                        app.reconcile();
                    }
                    Err(e) => {
                        app.server_error = Some(e.to_string());
                        app.audio.reconcile(&[]);
                    }
                }
            }
        }
        let dt = last.elapsed().as_secs_f32().clamp(0.001, 0.25);
        last = Instant::now();
        app.tick(terminal.size()?.width, start.elapsed().as_secs_f32(), dt);
        terminal.draw(|f| ui::draw(f, &app))?;
        let wait = Duration::from_secs_f64(1.0 / app.config.fps as f64)
            .saturating_sub(frame_start.elapsed());
        if event::poll(wait)?
            && let Event::Key(key) = event::read()?
            && app.key(key)
        {
            break;
        }
    }
    Ok(())
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
fn check(app: &mut App, seconds: u64, stop: &AtomicBool) -> Result<()> {
    if !app.demo {
        app.inventory = audio::discover()?;
        app.reconcile();
        if let Some(e) = &app.audio.error {
            bail!("{e}");
        }
        if app.audio.count() == 0 {
            bail!("No matching playback sources. Use --list-sources.");
        }
    }
    let start = Instant::now();
    let mut peak = 0.0_f32;
    let mut next_refresh = Instant::now() + Duration::from_secs(2);
    while start.elapsed() < Duration::from_secs(seconds) && !stop.load(Ordering::Relaxed) {
        if !app.demo && Instant::now() >= next_refresh {
            app.inventory = audio::discover()?;
            app.reconcile();
            next_refresh = Instant::now() + Duration::from_secs(2);
        }
        app.tick(100, start.elapsed().as_secs_f32(), 1.0 / 60.0);
        peak = peak.max(app.analysis.peak[0]).max(app.analysis.peak[1]);
        std::thread::sleep(Duration::from_millis(16));
    }
    if let Some(e) = &app.audio.error {
        bail!("Capture failed: {e}");
    }
    if !app.demo && app.audio.received() == 0 {
        app.reconcile();
        bail!(
            "No samples received: {}",
            app.audio
                .error
                .as_deref()
                .unwrap_or("check the PulseAudio server and source availability")
        );
    }
    println!(
        "Source: {}\nCaptures: {}\nSamples received: {}\nPeak: {:.2} dBFS\nFFT: {} samples\nStatus: {}",
        app.source_label(),
        app.audio.count(),
        app.audio.received(),
        dsp::db(peak),
        app.config.quality.size(),
        if peak > 0.00001 {
            "audio detected"
        } else {
            "capture connected; source is silent"
        }
    );
    Ok(())
}
