use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, VecDeque},
    io::Read,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const RATE: usize = 48000;
pub type Sample = [f32; 2];
#[derive(Debug, Clone, Deserialize)]
pub struct Sink {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub monitor_source: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Input {
    pub index: u32,
    pub sink: u32,
    #[serde(default)]
    pub corked: bool,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}
impl Input {
    pub fn name(&self) -> &str {
        self.properties
            .get("application.name")
            .or_else(|| self.properties.get("application.process.binary"))
            .map(String::as_str)
            .unwrap_or("Unknown application")
    }
    pub fn matches(&self, filter: &str) -> bool {
        let filter = filter.to_lowercase();
        self.name().to_lowercase().contains(&filter)
            || self
                .properties
                .get("application.process.binary")
                .is_some_and(|v| v.to_lowercase().contains(&filter))
    }
}
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub sinks: Vec<Sink>,
    pub inputs: Vec<Input>,
}
fn pactl(kind: &str) -> Result<Vec<u8>> {
    let mut child = Command::new("pactl")
        .args(["--format=json", "list", kind])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("pactl is required (Fedora: sudo dnf install pulseaudio-utils)")?;
    // Read concurrently so a large inventory cannot fill the pipe and deadlock.
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut b = Vec::new();
        stdout.read_to_end(&mut b).map(|_| b)
    });
    let err = thread::spawn(move || {
        let mut b = String::new();
        stderr.read_to_string(&mut b).map(|_| b)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            bail!("PulseAudio server timed out");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let bytes = out.join().unwrap()?;
    let error = err.join().unwrap()?;
    if !status.success() {
        bail!("PulseAudio: {}", error.trim());
    }
    Ok(bytes)
}
pub fn discover() -> Result<Inventory> {
    Ok(Inventory {
        sinks: serde_json::from_slice(&pactl("sinks")?)?,
        inputs: serde_json::from_slice(&pactl("sink-inputs")?)?,
    })
}
pub struct Discovery {
    pub rx: mpsc::Receiver<Result<Inventory>>,
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}
impl Discovery {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            loop {
                if tx.send(discover()).is_err() {
                    break;
                }
                if stop_rx.recv_timeout(Duration::from_secs(2))
                    != Err(mpsc::RecvTimeoutError::Timeout)
                {
                    break;
                }
            }
        });
        Self {
            rx,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub monitor: String,
    pub input: Option<u32>,
}
pub fn targets(inv: &Inventory, apps: &[String], sink: Option<&str>) -> Vec<Target> {
    if apps.is_empty() {
        return inv
            .sinks
            .iter()
            .filter(|s| sink.is_none_or(|name| s.name == name))
            .map(|s| Target {
                monitor: s.monitor_source.clone(),
                input: None,
            })
            .collect();
    }
    inv.inputs
        .iter()
        .filter(|i| apps.iter().any(|a| i.matches(a)))
        .filter_map(|i| {
            inv.sinks
                .iter()
                .find(|s| s.index == i.sink)
                .map(|s| Target {
                    monitor: s.monitor_source.clone(),
                    input: Some(i.index),
                })
        })
        .collect()
}
struct Ring {
    data: VecDeque<Sample>,
    updated: Instant,
    received: u64,
}
struct Capture {
    child: Child,
    reader: Option<JoinHandle<()>>,
    errors: Option<JoinHandle<String>>,
    ring: Arc<Mutex<Ring>>,
}
impl Capture {
    fn start(target: &Target) -> Result<Self> {
        let mut command = Command::new("parec");
        command
            .args([
                "--raw",
                "--format=float32le",
                "--rate=48000",
                "--channels=2",
                "--channel-map=front-left,front-right",
                "--latency-msec=30",
                "--process-time-msec=10",
                "--client-name=pulse-vis",
                "--stream-name=Visualizer monitor",
            ])
            .arg(format!("--device={}", target.monitor));
        if let Some(index) = target.input {
            command.arg(format!("--monitor-stream={index}"));
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("parec is required (Fedora: sudo dnf install pulseaudio-utils)")?;
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let ring = Arc::new(Mutex::new(Ring {
            data: VecDeque::with_capacity(16384),
            updated: Instant::now(),
            received: 0,
        }));
        let shared = ring.clone();
        let reader = thread::spawn(move || {
            let mut bytes = [0u8; 4096];
            let mut pending = Vec::with_capacity(4104);
            loop {
                match stdout.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&bytes[..n]);
                        let usable = pending.len() / 8 * 8;
                        {
                            let mut ring = shared.lock().unwrap();
                            for frame in pending[..usable].as_chunks::<8>().0 {
                                let l = f32::from_le_bytes(frame[..4].try_into().unwrap());
                                let r = f32::from_le_bytes(frame[4..].try_into().unwrap());
                                ring.data.push_back([
                                    if l.is_finite() { l } else { 0.0 },
                                    if r.is_finite() { r } else { 0.0 },
                                ]);
                            }
                            while ring.data.len() > 16384 {
                                ring.data.pop_front();
                            }
                            ring.received += (usable / 8) as u64;
                            ring.updated = Instant::now();
                        }
                        pending.drain(..usable);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });
        let errors = thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            s
        });
        Ok(Self {
            child,
            reader: Some(reader),
            errors: Some(errors),
            ring,
        })
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(t) = self.reader.take() {
            let _ = t.join();
        }
        if let Some(t) = self.errors.take() {
            let _ = t.join();
        }
    }
}
#[derive(Default)]
pub struct Audio {
    captures: BTreeMap<Target, Capture>,
    pub error: Option<String>,
}
impl Audio {
    pub fn reconcile(&mut self, wanted: &[Target]) {
        self.error = None;
        self.captures.retain(|target, capture| {
            if !wanted.contains(target) {
                return false;
            }
            if let Ok(Some(status)) = capture.child.try_wait() {
                let detail = capture
                    .errors
                    .take()
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                self.error = Some(format!("Capture exited ({status}): {}", detail.trim()));
                return false;
            }
            true
        });
        for target in wanted {
            if !self.captures.contains_key(target) {
                match Capture::start(target) {
                    Ok(capture) => {
                        self.captures.insert(target.clone(), capture);
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                    }
                }
            }
        }
    }
    pub fn samples(&self, size: usize) -> Vec<Sample> {
        let mut mix = vec![[0.0; 2]; size];
        for capture in self.captures.values() {
            let ring = capture.ring.lock().unwrap();
            if ring.updated.elapsed() > Duration::from_millis(250) {
                continue;
            }
            let count = ring.data.len().min(size);
            for (dest, src) in mix[size - count..]
                .iter_mut()
                .zip(ring.data.iter().skip(ring.data.len() - count))
            {
                dest[0] += src[0];
                dest[1] += src[1];
            }
        }
        mix
    }
    pub fn received(&self) -> u64 {
        self.captures
            .values()
            .map(|c| c.ring.lock().unwrap().received)
            .sum()
    }
    pub fn count(&self) -> usize {
        self.captures.len()
    }
}
pub fn demo(size: usize, time: f32) -> Vec<Sample> {
    (0..size)
        .map(|i| {
            let t = time + i as f32 / RATE as f32;
            let beat = (t * 2.8).sin().max(0.0).powi(4);
            let bass = (t * 110.0 * std::f32::consts::TAU).sin() * (0.1 + beat * 0.4);
            let lead =
                (t * (440.0 + 100.0 * (time * 0.4).sin()) * std::f32::consts::TAU).sin() * 0.15;
            [bass + lead, bass + lead * (t * 2.0).cos()]
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_groups_streams_and_tracks_sink_moves() {
        let mut inv = Inventory {
            sinks: vec![
                Sink {
                    index: 1,
                    name: "a".into(),
                    description: "A".into(),
                    monitor_source: "a.monitor".into(),
                },
                Sink {
                    index: 2,
                    name: "b".into(),
                    description: "B".into(),
                    monitor_source: "b.monitor".into(),
                },
            ],
            inputs: vec![Input {
                index: 9,
                sink: 1,
                corked: false,
                properties: BTreeMap::from([("application.name".into(), "Browser".into())]),
            }],
        };
        assert_eq!(targets(&inv, &[], None).len(), 2);
        assert_eq!(targets(&inv, &["BROW".into()], None)[0].input, Some(9));
        inv.inputs[0].sink = 2;
        assert_eq!(
            targets(&inv, &["browser".into()], None)[0].monitor,
            "b.monitor"
        );
        assert!(targets(&inv, &["missing".into()], None).is_empty());
        assert_eq!(
            targets(&inv, &["browser".into(), "brow".into()], None).len(),
            1
        );
    }
}
