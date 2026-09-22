//! Application state, key handling and the frame loop.

use crate::audio::{AppStream, AudioHandle, InputEntry, SinkEntry, Status, Target};
use crate::config::{Channels, Config, Mode, SpectrogramDirection};
use crate::dsp::{Analyzer, Request};
use crate::theme::{self, Theme};
use crate::ui;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub enum Popup {
    None,
    Help,
    Sources { selected: usize },
}

#[derive(Clone, Debug)]
pub enum SourceEntry {
    All,
    Sink(SinkEntry),
    Input(InputEntry),
    App(AppStream),
}

impl SourceEntry {
    pub fn target(&self) -> Target {
        match self {
            SourceEntry::All => Target::All,
            SourceEntry::Sink(s) => Target::Sink(s.name.clone()),
            SourceEntry::Input(i) => Target::Source(i.name.clone()),
            SourceEntry::App(a) => Target::App { name: a.app_name.clone(), index: Some(a.index) },
        }
    }

    pub fn is_current(&self, t: &Target) -> bool {
        match (self, t) {
            (SourceEntry::All, Target::All) => true,
            (SourceEntry::Sink(s), Target::Sink(n)) => s.name == *n,
            (SourceEntry::Input(i), Target::Source(n)) => i.name == *n,
            (SourceEntry::App(a), Target::App { name, index }) => {
                *index == Some(a.index) || (index.is_none() && a.matches(name))
            }
            _ => false,
        }
    }

    pub fn label(&self, st: &Status) -> String {
        match self {
            SourceEntry::All => {
                let desc = st
                    .sinks
                    .iter()
                    .find(|s| s.name == st.default_sink)
                    .map(|s| s.description.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| st.default_sink.clone());
                if desc.is_empty() { "All output".into() } else { format!("All output  ({desc})") }
            }
            SourceEntry::Sink(s) => {
                format!("Output: {}", if s.description.is_empty() { &s.name } else { &s.description })
            }
            SourceEntry::Input(i) => {
                let def = if i.name == st.default_source { "  (default)" } else { "" };
                format!("Input: {}{def}", if i.description.is_empty() { &i.name } else { &i.description })
            }
            SourceEntry::App(a) => {
                let mut s = a.app_name.clone();
                if !a.media_name.is_empty() && a.media_name != a.app_name {
                    s.push_str(" — ");
                    s.push_str(&a.media_name);
                }
                if !a.binary.is_empty() && !a.app_name.eq_ignore_ascii_case(&a.binary) {
                    s.push_str(&format!("  [{}]", a.binary));
                }
                s
            }
        }
    }
}

pub struct App {
    pub cfg: Config,
    pub config_path: PathBuf,
    pub theme: Theme,
    pub audio: AudioHandle,
    pub analyzer: Analyzer,
    pub status: Status,
    pub snap_l: Vec<f32>,
    pub snap_r: Vec<f32>,
    pub popup: Popup,
    pub paused: bool,
    pub flash: Option<(String, Instant)>,
    pub main_area: Rect,
    pub fps: f32,
    pub quit: bool,
    pub source_entries: Vec<SourceEntry>,
}

impl App {
    pub fn new(cfg: Config, config_path: PathBuf, target: Target) -> App {
        let theme = Theme::from_config(&cfg.colors, cfg.display.truecolor);
        let audio = AudioHandle::start(&cfg.audio, target);
        let analyzer = Analyzer::new(cfg.analysis.fft_size, cfg.audio.sample_rate);
        let fps = cfg.display.fps as f32;
        App {
            cfg,
            config_path,
            theme,
            audio,
            analyzer,
            status: Status::default(),
            snap_l: Vec::new(),
            snap_r: Vec::new(),
            popup: Popup::None,
            paused: false,
            flash: None,
            main_area: Rect::new(0, 0, 80, 23),
            fps,
            quit: false,
            source_entries: Vec::new(),
        }
    }

    fn stereo(&self) -> bool {
        self.cfg.display.channels == Channels::Stereo
    }

    fn request(&self) -> Request {
        let a = self.main_area;
        let stereo = self.stereo();
        match self.cfg.display.mode {
            Mode::Bars | Mode::Mirror => {
                let lay = ui::bars::layout(a.width, &self.cfg, stereo);
                Request { bars: lay.per_channel, stereo, spectro_bins: 0, spectro_rows: 0 }
            }
            Mode::Radial => {
                let bars = if self.cfg.display.radial_bars > 0 {
                    let n = self.cfg.display.radial_bars;
                    if stereo { (n / 2).max(2) } else { n }
                } else {
                    ui::radial::auto_bands(a, stereo)
                };
                Request { bars, stereo, spectro_bins: 0, spectro_rows: 0 }
            }
            Mode::Spectrogram => {
                let (bins, rows) = match self.cfg.display.spectrogram_direction {
                    SpectrogramDirection::Up | SpectrogramDirection::Down => {
                        (a.width as usize, a.height as usize * 2)
                    }
                    SpectrogramDirection::Left => (a.height as usize * 2, a.width as usize),
                };
                Request { bars: 16, stereo: false, spectro_bins: bins, spectro_rows: rows }
            }
            Mode::Wave | Mode::Vu | Mode::Lissajous => {
                Request { bars: 16, stereo: false, spectro_bins: 0, spectro_rows: 0 }
            }
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.status = self.audio.status();
        if matches!(self.popup, Popup::Sources { .. }) {
            self.rebuild_sources();
        }
        if self.paused {
            return;
        }
        let n = self.analyzer.frames_needed(&self.cfg);
        self.audio.snapshot(n, &mut self.snap_l, &mut self.snap_r);
        let req = self.request();
        self.analyzer.update(&self.cfg, &self.snap_l, &self.snap_r, dt, req);
    }

    fn rebuild_sources(&mut self) {
        let st = &self.status;
        let mut entries = vec![SourceEntry::All];
        entries.extend(st.apps.iter().cloned().map(SourceEntry::App));
        if st.sinks.len() > 1 {
            entries.extend(st.sinks.iter().cloned().map(SourceEntry::Sink));
        }
        entries.extend(st.inputs.iter().filter(|i| !i.is_monitor).cloned().map(SourceEntry::Input));
        self.source_entries = entries;
        if let Popup::Sources { selected } = &mut self.popup {
            *selected = (*selected).min(self.source_entries.len().saturating_sub(1));
        }
    }

    fn open_sources(&mut self) {
        self.audio.refresh();
        self.popup = Popup::Sources { selected: 0 };
        self.rebuild_sources();
        let cur = self.source_entries.iter().position(|e| e.is_current(&self.status.target)).unwrap_or(0);
        self.popup = Popup::Sources { selected: cur };
    }

    fn apply_target(&mut self, t: Target) {
        self.flash(format!("source: {}", t.short_label()));
        self.cfg.audio.source = t.to_spec();
        self.status.target = t.clone();
        self.audio.set_target(t);
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }

    fn refresh_theme(&mut self) {
        self.theme = Theme::from_config(&self.cfg.colors, self.cfg.display.truecolor);
    }

    fn save_config(&mut self) {
        match self.cfg.save(&self.config_path) {
            Ok(()) => self.flash(format!("saved {}", self.config_path.display())),
            Err(e) => self.flash(format!("save failed: {e}")),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match &mut self.popup {
            Popup::Help => {
                self.popup = Popup::None;
                return;
            }
            Popup::Sources { selected } => {
                let len = self.source_entries.len();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => {
                        if *selected + 1 < len {
                            *selected += 1;
                        }
                    }
                    KeyCode::Home => *selected = 0,
                    KeyCode::End => *selected = len.saturating_sub(1),
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if let Some(e) = self.source_entries.get(*selected).cloned() {
                            self.popup = Popup::None;
                            self.apply_target(e.target());
                        }
                    }
                    KeyCode::Char('r') => self.audio.refresh(),
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => self.popup = Popup::None,
                    _ => {}
                }
                return;
            }
            Popup::None => {}
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let d = &mut self.cfg.display;
        let a = &mut self.cfg.analysis;
        let s = &mut self.cfg.sensitivity;
        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Tab | KeyCode::Char('m') => d.mode = d.mode.next(),
            KeyCode::BackTab | KeyCode::Char('M') => d.mode = d.mode.prev(),
            KeyCode::Char(ch @ '1'..='7') => {
                if let Some(m) = Mode::from_digit(ch.to_digit(10).unwrap_or(0)) {
                    d.mode = m;
                }
            }
            KeyCode::Char('s') => self.open_sources(),
            KeyCode::Char('r') => {
                self.audio.refresh();
                self.flash("refreshing sources");
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                s.gain_db = (s.gain_db + 2.0).min(60.0);
                let g = s.gain_db;
                self.flash(format!("gain {g:+.0} dB"));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                s.gain_db = (s.gain_db - 2.0).max(-60.0);
                let g = s.gain_db;
                self.flash(format!("gain {g:+.0} dB"));
            }
            KeyCode::Char('a') => {
                s.auto_gain = !s.auto_gain;
                let on = s.auto_gain;
                self.flash(if on { "auto gain on" } else { "auto gain off" });
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                let step = if key.code == KeyCode::Char('c') { 1 } else { -1 };
                self.cfg.colors.theme = theme::cycle_preset(&self.cfg.colors.theme, step).to_string();
                self.refresh_theme();
                let name = self.cfg.colors.theme.clone();
                self.flash(format!("theme: {name}"));
            }
            KeyCode::Char('d') => {
                self.cfg.colors.direction = self.cfg.colors.direction.next();
                self.refresh_theme();
                let dir = self.cfg.colors.direction;
                self.flash(format!("gradient: {dir:?}").to_lowercase());
            }
            KeyCode::Char('t') => {
                d.channels = if d.channels == Channels::Stereo { Channels::Mono } else { Channels::Stereo };
            }
            KeyCode::Char('p') => d.peaks = !d.peaks,
            KeyCode::Char('[') => a.bar_width = a.bar_width.saturating_sub(1).max(1),
            KeyCode::Char(']') => a.bar_width = (a.bar_width + 1).min(16),
            KeyCode::Char('{') => a.bar_gap = a.bar_gap.saturating_sub(1),
            KeyCode::Char('}') => a.bar_gap = (a.bar_gap + 1).min(8),
            KeyCode::Char(',') => {
                a.decay_ms = (a.decay_ms - 30.0).max(0.0);
                let v = a.decay_ms;
                self.flash(format!("decay {v:.0} ms"));
            }
            KeyCode::Char('.') => {
                a.decay_ms = (a.decay_ms + 30.0).min(2000.0);
                let v = a.decay_ms;
                self.flash(format!("decay {v:.0} ms"));
            }
            KeyCode::Char('<') => {
                a.fft_size = (a.fft_size / 2).max(256);
                let v = a.fft_size;
                self.flash(format!("fft {v}"));
            }
            KeyCode::Char('>') => {
                a.fft_size = (a.fft_size * 2).min(32768);
                let v = a.fft_size;
                self.flash(format!("fft {v}"));
            }
            KeyCode::Char('l') => {
                a.scale = a.scale.next();
                let v = a.scale;
                self.flash(format!("scale: {v:?}").to_lowercase());
            }
            KeyCode::Char('v') => d.wave_style = d.wave_style.next(),
            KeyCode::Char('o') => d.spectrogram_direction = d.spectrogram_direction.next(),
            KeyCode::Char('x') => d.lissajous_rotate = !d.lissajous_rotate,
            KeyCode::Char('b') => d.status_bar = !d.status_bar,
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('w') => self.save_config(),
            KeyCode::Char('?') | KeyCode::Char('h') | KeyCode::F(1) => self.popup = Popup::Help,
            _ => {}
        }
    }
}

pub fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = main_loop(&mut app, &mut terminal);
    ratatui::restore();
    result
}

fn main_loop(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let mut last = Instant::now();
    loop {
        let frame = Duration::from_secs_f64(1.0 / app.cfg.display.fps.clamp(10, 240) as f64);
        let deadline = last + frame;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            if event::poll(deadline - now)? {
                match event::read()? {
                    Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => app.on_key(k),
                    _ => {}
                }
                if app.quit {
                    return Ok(());
                }
            }
        }
        let now = Instant::now();
        let dt = (now - last).as_secs_f32().clamp(0.001, 0.1);
        app.fps = app.fps * 0.9 + (1.0 / dt) * 0.1;
        last = now;
        app.tick(dt);
        terminal.draw(|f| ui::draw(f, app))?;
    }
}
