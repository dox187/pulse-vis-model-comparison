//! Audio capture engine: reads from PulseAudio streams, stores samples, and
//! renders frames for each visualization mode.

use crate::config::{Config, ColorScheme};
use crate::fft::fft_transform;
use crate::ffi::{SourceInfo, Stream};

/// A ring buffer of f32 samples (mono, mixed channels) for one source.
pub struct SampleBuffer {
    buffer: Vec<f32>,
    write_pos: usize,
    count: usize,
}

impl SampleBuffer {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(2);
        Self { buffer: vec![0.0f32; cap], write_pos: 0, count: 0 }
    }

    /// Append samples into the ring buffer, discarding the oldest when full.
    pub fn push(&mut self, samples: &[f32]) {
        if samples.is_empty() { return; }
        if self.count >= self.buffer.len() {
            self.count = 0;
            self.write_pos = 0;
        }
        for s in samples {
            self.buffer[self.write_pos] = *s;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            self.count += 1;
            if self.count >= self.buffer.len() {
                self.count = self.buffer.len();
                break;
            }
        }
    }

    /// The most recent `n` samples, oldest -> newest. Zeros where data is missing.
    pub fn latest(&self, n: usize) -> Vec<f32> {
        let take = n.min(self.count);
        let mut out = vec![0.0f32; n];
        let base = self.write_pos.wrapping_sub(take);
        for i in 0..take {
            let idx = (base + i) % self.buffer.len();
            out[i] = self.buffer[idx];
        }
        out
    }

    /// Peak absolute value over the most recent `n` samples.
    pub fn peak(&self, n: usize) -> f32 {
        self.latest(n).iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// RMS over the most recent `n` samples.
    pub fn rms(&self, n: usize) -> f32 {
        let take = n.min(self.count);
        if take == 0 { return 0.0; }
        let mut sum = 0.0f32;
        let base = self.write_pos.wrapping_sub(take);
        for i in 0..take {
            let idx = (base + i) % self.buffer.len();
            let v = self.buffer[idx];
            sum += v * v;
        }
        (sum / take as f32).sqrt()
    }

    pub fn count(&self) -> usize { self.count }
}

#[derive(Clone, Debug, PartialEq, Copy)]
pub enum VizMode {
    Bars,
    Scope,
    Spectrum,
    Waveform,
    Meters,
}

impl VizMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "scope" => VizMode::Scope,
            "spectrum" => VizMode::Spectrum,
            "waveform" => VizMode::Waveform,
            "meters" => VizMode::Meters,
            _ => VizMode::Bars,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            VizMode::Bars => "bars",
            VizMode::Scope => "scope",
            VizMode::Spectrum => "spectrum",
            VizMode::Waveform => "waveform",
            VizMode::Meters => "meters",
        }
    }

    pub fn order_index(self) -> usize {
        match self {
            VizMode::Bars => 0,
            VizMode::Scope => 1,
            VizMode::Spectrum => 2,
            VizMode::Waveform => 3,
            VizMode::Meters => 4,
        }
    }
}

pub struct VizEngine {
    pub mode: VizMode,
    pub config: Config,
    pub sources: Vec<SourceInfo>,
    /// 0 = all sources, otherwise 1-based index of selected source.
    pub selected_source: usize,
    pub buffers: Vec<SampleBuffer>,
}

impl VizEngine {
    pub fn new(config: Config, sources: Vec<SourceInfo>, capacity: usize) -> Self {
        let n = sources.len();
        Self {
            mode: VizMode::from_str(&config.mode),
            config,
            sources,
            selected_source: 0,
            buffers: (0..n).map(|_| SampleBuffer::new(capacity)).collect(),
        }
    }

    pub fn get_mode(&self) -> VizMode { self.mode }
    pub fn get_mode_name(&self) -> &'static str { self.mode.name() }
    pub fn set_mode_name(&mut self, mode: &str) { self.mode = VizMode::from_str(mode); }

    /// 1-based selected source index (0 = all).
    pub fn get_selected_source(&self) -> usize { self.selected_source }

    /// Set a 1-based source index; 0 means "all".
    pub fn set_selected_source(&mut self, idx: usize) {
        if idx == 0 || idx > self.buffers.len() {
            self.selected_source = 0;
        } else {
            self.selected_source = idx;
        }
    }

    pub fn set_selected_source_name(&mut self, name: &str) {
        let fl = name.to_lowercase();
        for (i, s) in self.sources.iter().enumerate() {
            if s.name.to_lowercase().contains(&fl) || s.alias.to_lowercase().contains(&fl) {
                self.selected_source = i + 1;
                return;
            }
        }
        self.selected_source = 0;
    }

    /// 1-based indices of sources to render: all when unselected, else one.
    pub fn active_indices(&self) -> Vec<usize> {
        if self.selected_source == 0 {
            (0..self.buffers.len()).collect()
        } else {
            vec![self.selected_source - 1]
        }
    }

    /// Read a chunk from every matching stream into the corresponding buffer.
    /// `streams` is aligned by index with `buffers`.
    pub fn read_all(&mut self, streams: &[Stream], buffer_size: usize) -> bool {
        let mut any = false;
        for (i, stream) in streams.iter().enumerate() {
            if i >= self.buffers.len() { break; }
            let available = stream.readable_size();
            if available == 0 { continue; }
            let want = available.min(buffer_size);
            let buf = vec![0u8; want];
            let got = stream.read(&buf);
            if got == 0 { continue; }
            let fmt = stream.get_sample_format();
            let ch = stream.get_channels().max(1);
            let samples = pcm_to_f32(&buf, fmt, ch);
            if samples.is_empty() { continue; }
            let sens = self.config.sensitivity.max(0.01);
            let scaled: Vec<f32> = samples.into_iter().map(|s| s * sens).collect();
            self.buffers[i].push(&scaled);
            any = true;
        }
        any
    }

    /// Mix samples across all active sources into a single mono buffer.
    pub fn combined(&self, n: usize) -> Vec<f32> {
        let idxs = self.active_indices();
        if idxs.is_empty() { return vec![0.0f32; n]; }
        let mut out = vec![0.0f32; n];
        let weight = 1.0 / idxs.len() as f32;
        for &idx in &idxs {
            let samples = self.buffers[idx].latest(n);
            for (i, &s) in samples.iter().enumerate() {
                out[i] += s * weight;
            }
        }
        out
    }

    const RESET: &'static str = "\x1b[0m";

    /// Build an ANSI foreground color string for RGB.
    fn rgb_fg(&self, rgb: (u8, u8, u8)) -> String {
        format!("\x1b[38;2;{};{};{}m{}", rgb.0, rgb.1, rgb.2, VizEngine::RESET)
    }

    /// Color for a normalized position `t` in [0,1].
    fn scheme_color(&self, t: f32) -> (u8, u8, u8) {
        let t = t.clamp(0.0, 1.0);
        match self.config.color_scheme {
            ColorScheme::Cyan => {
                (
                    (t * 0.0) as u8,
                    (t * 255.0) as u8,
                    (255.0 * (1.0 - t)) as u8,
                )
            }
            ColorScheme::Green => {
                (
                    0,
                    (t * 255.0) as u8,
                    (255.0 * (1.0 - t)) as u8,
                )
            }
            ColorScheme::Purple => {
                (
                    (255.0 * (1.0 - t)) as u8,
                    0,
                    (255.0 * (0.5 + 0.5 * t)) as u8,
                )
            }
            ColorScheme::Rainbow => {
                let (r, g, b) = hsv2rgb(t * 360.0f32);
                (r as u8, g as u8, b as u8)
            }
        }
    }

    /// Render the current mode into `height` rows, each up to `width` chars.
    pub fn render(&self, width: usize, height: usize) -> Vec<String> {
        match self.mode {
            VizMode::Bars => self.render_bars(width, height),
            VizMode::Scope => self.render_scope(width, height),
            VizMode::Spectrum => self.render_spectrum(width, height),
            VizMode::Waveform => self.render_waveform(width, height),
            VizMode::Meters => self.render_meters(width, height),
        }
    }

    fn render_bars(&self, w: usize, h: usize) -> Vec<String> {
        let mut lines = vec![String::new(); h];
        let samples = self.combined(w.max(1) * 8);
        if samples.is_empty() { return lines; }
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-9);
        let usable = if h > 1 { h - 1 } else { 0 };
        for i in 0..w {
            let t = if w > 1 { i as f32 / (w - 1) as f32 } else { 0.0 };
            let idx = i * 8;
            let v = samples.get(idx).copied().unwrap_or(0.0) / peak;
            let bar_h = (v.abs().clamp(0.0, 1.0) * usable as f32) as usize;
            let color = self.scheme_color(t);
            for j in 0..bar_h {
                let y = (h - 1) - j;
                if y >= h { continue; }
                lines[y].push_str(&self.rgb_fg(color));
                lines[y].push_str("█");
            }
            // ensure the row stays within width
        }
        lines
    }

    fn render_scope(&self, w: usize, h: usize) -> Vec<String> {
        let mut lines = vec![String::new(); h];
        let samples = self.combined(w.max(2));
        if samples.is_empty() { return lines; }
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-9);
        if h == 0 { return lines; }
        let mid = h / 2;
        for i in 0..w {
            let t = if w > 1 { i as f32 / (w - 1) as f32 } else { 0.0 };
            let idx = (t * samples.len() as f32) as usize;
            let idx = idx.min(samples.len() - 1);
            let val = samples[idx] / peak;
            let span = (h as f32 / 2.0 - 1.0).max(0.5);
            let y = ((mid as f32 + val * span).clamp(0.0, (f32::MAX))) as usize;
            let y = y.min(h - 1);
            lines[y].push_str(&self.rgb_fg(self.scheme_color(t)));
            lines[y].push_str("·");
        }
        lines
    }

    fn render_spectrum(&self, w: usize, h: usize) -> Vec<String> {
        let mut lines = vec![String::new(); h];
        let fft_size = 256;
        let samples = self.combined(fft_size);
        let mut x = vec![0.0f32; fft_size];
        for i in 0..samples.len().min(fft_size) {
            x[i] = samples[i];
        }
        let mut out = vec![0.0f32; fft_size / 2 + 1];
        fft_transform(&x, &mut out);
        let half = out.len();
        let global = out.iter().fold(0.0f32, |m, x| m.max(*x)).max(1e-9);
        for i in 0..w {
            let t = if w > 1 { i as f32 / (w - 1) as f32 } else { 0.0 };
            let bin_idx = (t * (half as f32 - 1.0)) as usize;
            let bin_idx = bin_idx.min(half - 1);
            let amp = out[bin_idx] / global;
            let bar_h = (amp.clamp(0.0, 1.0) * h as f32) as usize;
            let color = self.scheme_color(t);
            for j in 0..bar_h {
                let y = h - 1 - j;
                lines[y].push_str(&self.rgb_fg(color));
                lines[y].push_str("█");
            }
        }
        lines
    }

    fn render_waveform(&self, w: usize, h: usize) -> Vec<String> {
        if h == 0 { return vec![String::new(); 0]; }
        let mut lines = vec![String::new(); h];
        let samples = self.combined(w.max(2));
        if samples.is_empty() { return lines; }
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-9);
        for i in 0..w {
            let t = if w > 1 { i as f32 / (w - 1) as f32 } else { 0.0 };
            let idx = (t * samples.len() as f32) as usize;
            let idx = idx.min(samples.len() - 1);
            let val = (samples[idx] / peak).clamp(-1.0, 1.0);
            let y = ((val * 0.5 + 0.5) * h as f32).clamp(0.0, f32::MAX) as usize;
            let y = y.min(h - 1);
            lines[y].push_str(&self.rgb_fg(self.scheme_color(t)));
            lines[y].push_str("─");
        }
        lines
    }

    fn render_meters(&self, _w: usize, h: usize) -> Vec<String> {
        if h == 0 { return vec![String::new(); 0]; }
        let mut lines = vec![String::new(); h];
        let idxs = self.active_indices();
        if idxs.is_empty() { return lines; }
        let n = idxs.len();
        for (pos, &i) in idxs.iter().enumerate() {
            let rms = self.buffers[i].rms(128);
            let t = 1.0 - (pos as f32 + 0.5) / n as f32;
            let color = self.scheme_color(t);
            let fg = self.rgb_fg(color);
            for j in 0..h {
                let y = h - 1 - j;
                if rms * h as f32 > j as f32 {
                    lines[y].push_str(&fg);
                    lines[y].push_str("█");
                }
            }
        }
        lines
    }
}

impl std::fmt::Display for VizEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lines = self.render(80, 24);
        for line in &lines {
            writeln!(f, "{line}")?;
        }
        Ok(())
    }
}

/// Convert `h` in degrees [0,360) to RGB (0..1).
fn hsv2rgb(h: f32) -> (f32, f32, f32) {
    let h = (h % 360.0f32).max(0.0f32);
    let i = (h / 60.0f32) as u32;
    let k = (h / 60.0f32).fract();
    let (r, g, b): (f32, f32, f32) = match i {
        0 => (1.0, k, 0.0),
        1 => (0.0, 1.0, k),
        2 => (0.0, 1.0 - k, 1.0),
        3 => (k, 0.0, 1.0),
        4 => (1.0, 0.0, 1.0 - k),
        _ => (1.0 - k, 0.0, 1.0),
    };
    (r, g, b)
}

/// Convert interleaved PCM bytes to a mono f32 buffer (channels averaged).
/// `format`: 14=S16LE, 12=FLOAT32LE, 16=S32LE.
pub fn pcm_to_f32(bytes: &[u8], format: i32, channels: i32) -> Vec<f32> {
    if bytes.is_empty() { return vec![]; }
    let ch = channels.max(1) as usize;
    let (size, scale) = match format {
        14 => (2, 1.0 / 32768.0),
        12 => (4, 1.0),
        16 => (4, 1.0 / 2147483648.0),
        _ => (2, 1.0 / 32768.0),
    };
    let total = bytes.len() / (size * ch);
    let mut out = Vec::with_capacity(total);
    for f in 0..total {
        let base = f * ch * size;
        if base + ch * size > bytes.len() { break; }
        let mut sum = 0.0f32;
        for c in 0..ch {
            let o = base + c * size;
            let v: f32 = if format == 14 {
                let raw = i16::from_le_bytes([bytes[o], bytes[o + 1]]);
                raw as f32
            } else if format == 12 {
                let raw = f32::from_le_bytes([bytes[o], bytes[o+1], bytes[o+2], bytes[o+3]]);
                raw
            } else {
                let raw = i32::from_le_bytes([bytes[o], bytes[o+1], bytes[o+2], bytes[o+3]]);
                raw as f32
            };
            sum += v * scale;
        }
        out.push(sum / ch as f32);
    }
    out
}
