//! Signal analysis: windowed FFT, logarithmic band folding, gain handling, smoothing,
//! peak ballistics, level metering and spectrogram history.

use crate::config::{BandMode, Config, Scale};
use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use std::collections::VecDeque;
use std::sync::Arc;

/// Maps FFT bins onto `n` logarithmically spaced bands.
struct Bands {
    n: usize,
    fft_size: usize,
    rate: u32,
    fmin: f32,
    fmax: f32,
    tilt_db: f32,
    /// Band edges in (fractional) bin units, `n + 1` entries.
    edges: Vec<f32>,
    /// Linear gain per band implementing the spectral tilt.
    tilt: Vec<f32>,
}

impl Bands {
    fn build(n: usize, fft_size: usize, rate: u32, fmin: f32, fmax: f32, tilt_db: f32) -> Bands {
        let n = n.max(1);
        let nyquist = rate as f32 / 2.0;
        let fmax_c = fmax.min(nyquist * 0.98).max(20.0);
        let fmin_c = fmin.clamp(1.0, fmax_c * 0.5);
        let bin_hz = rate as f32 / fft_size as f32;
        let ratio = fmax_c / fmin_c;
        let edges: Vec<f32> = (0..=n)
            .map(|k| fmin_c * ratio.powf(k as f32 / n as f32) / bin_hz)
            .collect();
        let tilt: Vec<f32> = (0..n)
            .map(|k| {
                let fc = ((edges[k] * edges[k + 1]).sqrt() * bin_hz).max(1.0);
                10f32.powf(tilt_db * (fc / 1000.0).log2() / 20.0)
            })
            .collect();
        Bands { n, fft_size, rate, fmin, fmax, tilt_db, edges, tilt }
    }

    fn matches(&self, n: usize, fft_size: usize, rate: u32, fmin: f32, fmax: f32, tilt_db: f32) -> bool {
        self.n == n.max(1)
            && self.fft_size == fft_size
            && self.rate == rate
            && self.fmin == fmin
            && self.fmax == fmax
            && self.tilt_db == tilt_db
    }

    /// Folds linear magnitudes (one per bin) into linear band values.
    fn apply(&self, mags: &[f32], mode: BandMode, out: &mut Vec<f32>) {
        out.clear();
        let max_bin = mags.len().saturating_sub(1);
        if max_bin == 0 {
            out.resize(self.n, 0.0);
            return;
        }
        for k in 0..self.n {
            let lo = self.edges[k];
            let hi = self.edges[k + 1];
            let v = if hi - lo >= 1.0 {
                let a = (lo.ceil() as usize).clamp(1, max_bin);
                let b = (hi.floor() as usize).clamp(a, max_bin);
                match mode {
                    BandMode::Peak => mags[a..=b].iter().fold(0.0f32, |m, &x| m.max(x)),
                    BandMode::Average => mags[a..=b].iter().sum::<f32>() / (b - a + 1) as f32,
                }
            } else {
                interp(mags, (lo + hi) * 0.5)
            };
            out.push(v * self.tilt[k]);
        }
    }
}

fn interp(mags: &[f32], pos: f32) -> f32 {
    let pos = pos.max(0.0);
    let i = pos.floor() as usize;
    let f = pos - i as f32;
    let a = mags.get(i).copied().unwrap_or(0.0);
    let b = mags.get(i + 1).copied().unwrap_or(a);
    a + (b - a) * f
}

/// Per-channel band state.
#[derive(Default, Clone)]
pub struct Chan {
    /// Linear band magnitudes after tilt, before gain.
    lin: Vec<f32>,
    /// Normalised (0..1) instantaneous values.
    norm: Vec<f32>,
    /// Temporally smoothed 0..1 values; what the bars show.
    pub smooth: Vec<f32>,
    /// Peak marker positions, 0..1.
    pub peak: Vec<f32>,
    peak_vel: Vec<f32>,
    peak_hold: Vec<f32>,
}

impl Chan {
    fn resize(&mut self, n: usize) {
        if self.smooth.len() != n {
            self.lin = vec![0.0; n];
            self.norm = vec![0.0; n];
            self.smooth = vec![0.0; n];
            self.peak = vec![0.0; n];
            self.peak_vel = vec![0.0; n];
            self.peak_hold = vec![0.0; n];
        }
    }
}

/// Meter readings, linear full-scale units (1.0 = 0 dBFS).
#[derive(Default, Clone, Copy)]
pub struct Levels {
    pub rms: [f32; 2],
    pub peak: [f32; 2],
    pub peak_hold: [f32; 2],
    hold_t: [f32; 2],
    pub clip: [bool; 2],
    pub correlation: f32,
}

pub fn to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

/// What the current display needs from the analyser this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    /// Bands per channel.
    pub bars: usize,
    pub stereo: bool,
    /// Frequency resolution of one spectrogram row.
    pub spectro_bins: usize,
    /// Rows of history to keep.
    pub spectro_rows: usize,
}

pub const LEFT: usize = 0;
pub const RIGHT: usize = 1;
pub const MID: usize = 2;

pub struct Analyzer {
    fft_size: usize,
    rate: u32,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// Normalisation so that a full-scale sine reads 1.0.
    window_gain: f32,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mags: [Vec<f32>; 3],
    bands: Option<Bands>,
    spectro_bands: Option<Bands>,
    band_buf: Vec<f32>,
    /// Left, right and mid channel band state.
    pub ch: [Chan; 3],
    pub stereo: bool,
    pub bars: usize,
    env_db: f32,
    /// Gain (dB) contributed by auto gain; add `gain_db` for the total.
    pub auto_offset_db: f32,
    pub levels: Levels,
    pub spectro: VecDeque<Vec<f32>>,
    spectro_bins: usize,
}

impl Analyzer {
    pub fn new(fft_size: usize, rate: u32) -> Analyzer {
        let mut a = Analyzer {
            fft_size: 0,
            rate,
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(fft_size),
            window: Vec::new(),
            window_gain: 1.0,
            input: Vec::new(),
            output: Vec::new(),
            scratch: Vec::new(),
            mags: [Vec::new(), Vec::new(), Vec::new()],
            bands: None,
            spectro_bands: None,
            band_buf: Vec::new(),
            ch: [Chan::default(), Chan::default(), Chan::default()],
            stereo: false,
            bars: 0,
            env_db: -20.0,
            auto_offset_db: 0.0,
            levels: Levels::default(),
            spectro: VecDeque::new(),
            spectro_bins: 0,
        };
        a.set_fft(fft_size, rate);
        a
    }

    fn set_fft(&mut self, fft_size: usize, rate: u32) {
        if self.fft_size == fft_size && self.rate == rate && !self.window.is_empty() {
            return;
        }
        self.fft_size = fft_size;
        self.rate = rate;
        self.fft = RealFftPlanner::<f32>::new().plan_fft_forward(fft_size);
        // Hann window.
        self.window = (0..fft_size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / fft_size as f32).cos())
            .collect();
        let sum: f32 = self.window.iter().sum();
        self.window_gain = 2.0 / sum.max(1e-6);
        self.input = self.fft.make_input_vec();
        self.output = self.fft.make_output_vec();
        self.scratch = self.fft.make_scratch_vec();
        for m in &mut self.mags {
            *m = vec![0.0; fft_size / 2 + 1];
        }
        self.bands = None;
        self.spectro_bands = None;
    }

    /// Number of frames the caller should hand over per update.
    pub fn frames_needed(&self, cfg: &Config) -> usize {
        let ms = cfg.display.wave_ms.max(cfg.display.xy_ms).clamp(1.0, 2000.0);
        let window = (cfg.audio.sample_rate as f32 * ms / 1000.0) as usize;
        cfg.analysis.fft_size.max(window).max(cfg.audio.sample_rate as usize / 10)
    }

    /// Runs one analysis pass on the newest samples. `l`/`r` must hold at least `fft_size`
    /// frames (zero-padded by the ring buffer when the stream just started).
    pub fn update(&mut self, cfg: &Config, l: &[f32], r: &[f32], dt: f32, req: Request) {
        self.set_fft(cfg.analysis.fft_size, cfg.audio.sample_rate);
        let n = req.bars.max(1);
        self.stereo = req.stereo;
        self.bars = n;
        for c in &mut self.ch {
            c.resize(n);
        }

        // Spectra.
        if req.stereo {
            self.compute_mags(LEFT, l, None);
            self.compute_mags(RIGHT, r, None);
        }
        self.compute_mags(MID, l, Some(r));

        // Bands.
        let a = &cfg.analysis;
        let needs_rebuild = self
            .bands
            .as_ref()
            .map(|b| !b.matches(n, self.fft_size, self.rate, a.freq_min, a.freq_max, a.tilt_db_per_oct))
            .unwrap_or(true);
        if needs_rebuild {
            self.bands = Some(Bands::build(n, self.fft_size, self.rate, a.freq_min, a.freq_max, a.tilt_db_per_oct));
        }
        let channels: &[usize] = if req.stereo { &[LEFT, RIGHT, MID] } else { &[MID] };
        let bands = self.bands.as_ref().unwrap();
        for &c in channels {
            bands.apply(&self.mags[c], a.band_mode, &mut self.band_buf);
            self.ch[c].lin.copy_from_slice(&self.band_buf);
        }

        // Auto gain follows the loudest band of the displayed channels.
        let s = &cfg.sensitivity;
        let shown: &[usize] = if req.stereo { &[LEFT, RIGHT] } else { &[MID] };
        let mut frame_peak_db = f32::NEG_INFINITY;
        for &c in shown {
            for &v in &self.ch[c].lin {
                let db = to_db(v);
                if db > s.noise_floor_db && db > frame_peak_db {
                    frame_peak_db = db;
                }
            }
        }
        let env_floor = s.noise_floor_db + s.dynamic_range_db;
        if frame_peak_db.is_finite() && frame_peak_db > self.env_db {
            self.env_db += (frame_peak_db - self.env_db) * (dt * 25.0).min(1.0);
        } else {
            self.env_db -= s.auto_gain_release_db_per_s.max(0.0) * dt;
        }
        self.env_db = self.env_db.max(env_floor);
        let desired = if s.auto_gain { -1.0 - self.env_db } else { 0.0 };
        self.auto_offset_db += (desired - self.auto_offset_db) * (dt * 3.0).min(1.0);
        let gain_lin = 10f32.powf((self.auto_offset_db + s.gain_db) / 20.0);

        // Normalise, spread, smooth, peaks.
        let attack_k = coeff(dt, a.attack_ms);
        let decay_k = coeff(dt, a.decay_ms);
        let hold_s = cfg.display.peak_hold_ms.max(0.0) / 1000.0;
        let gravity = cfg.display.peak_gravity.max(0.0);
        for &c in channels {
            let ch = &mut self.ch[c];
            for i in 0..n {
                ch.norm[i] = normalize(ch.lin[i], gain_lin, a.scale, s.dynamic_range_db);
            }
            monstercat(&mut ch.norm, a.monstercat);
            for i in 0..n {
                let target = ch.norm[i];
                let cur = ch.smooth[i];
                let k = if target > cur { attack_k } else { decay_k };
                let v = cur + (target - cur) * k;
                ch.smooth[i] = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
                // Peak ballistics.
                if ch.smooth[i] >= ch.peak[i] {
                    ch.peak[i] = ch.smooth[i];
                    ch.peak_vel[i] = 0.0;
                    ch.peak_hold[i] = hold_s;
                } else if ch.peak_hold[i] > 0.0 {
                    ch.peak_hold[i] -= dt;
                } else {
                    ch.peak_vel[i] += gravity * dt;
                    ch.peak[i] = (ch.peak[i] - ch.peak_vel[i] * dt).max(ch.smooth[i]);
                }
            }
        }

        // Spectrogram row from the mid channel.
        if req.spectro_bins > 0 && req.spectro_rows > 0 {
            let bins = req.spectro_bins;
            let rebuild = self
                .spectro_bands
                .as_ref()
                .map(|b| !b.matches(bins, self.fft_size, self.rate, a.freq_min, a.freq_max, a.tilt_db_per_oct))
                .unwrap_or(true);
            if rebuild {
                self.spectro_bands =
                    Some(Bands::build(bins, self.fft_size, self.rate, a.freq_min, a.freq_max, a.tilt_db_per_oct));
            }
            if self.spectro_bins != bins {
                self.spectro.clear();
                self.spectro_bins = bins;
            }
            let sb = self.spectro_bands.as_ref().unwrap();
            sb.apply(&self.mags[MID], a.band_mode, &mut self.band_buf);
            let row: Vec<f32> = self
                .band_buf
                .iter()
                .map(|&v| normalize(v, gain_lin, a.scale, s.dynamic_range_db))
                .collect();
            self.spectro.push_back(row);
            while self.spectro.len() > req.spectro_rows {
                self.spectro.pop_front();
            }
        } else if !self.spectro.is_empty() {
            self.spectro.clear();
        }

        self.update_levels(l, r, dt);
    }

    fn compute_mags(&mut self, slot: usize, a: &[f32], b: Option<&[f32]>) {
        let n = self.fft_size;
        let a = &a[a.len().saturating_sub(n)..];
        match b {
            Some(b) => {
                let b = &b[b.len().saturating_sub(n)..];
                for i in 0..n {
                    let x = (a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0)) * 0.5;
                    self.input[i] = x * self.window[i];
                }
            }
            None => {
                for i in 0..n {
                    self.input[i] = a.get(i).copied().unwrap_or(0.0) * self.window[i];
                }
            }
        }
        if self.fft.process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch).is_err() {
            return;
        }
        let g = self.window_gain;
        for (m, c) in self.mags[slot].iter_mut().zip(self.output.iter()) {
            *m = c.norm() * g;
        }
    }

    fn update_levels(&mut self, l: &[f32], r: &[f32], dt: f32) {
        let win = (self.rate as usize / 10).max(1).min(l.len()).min(r.len());
        let l = &l[l.len() - win..];
        let r = &r[r.len() - win..];
        let (mut sl, mut sr, mut slr, mut pl, mut pr) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for (&x, &y) in l.iter().zip(r.iter()) {
            sl += x * x;
            sr += y * y;
            slr += x * y;
            pl = pl.max(x.abs());
            pr = pr.max(y.abs());
        }
        let inv = 1.0 / win as f32;
        let rms = [(sl * inv).sqrt(), (sr * inv).sqrt()];
        let peak = [pl, pr];
        let lv = &mut self.levels;
        lv.correlation = if sl > 1e-12 && sr > 1e-12 { (slr / (sl * sr).sqrt()).clamp(-1.0, 1.0) } else { 0.0 };
        let up = coeff(dt, 30.0);
        let down = coeff(dt, 300.0);
        for c in 0..2 {
            let cur = lv.rms[c];
            lv.rms[c] = cur + (rms[c] - cur) * if rms[c] > cur { up } else { down };
            lv.peak[c] = peak[c];
            lv.clip[c] = peak[c] >= 0.99;
            if peak[c] >= lv.peak_hold[c] {
                lv.peak_hold[c] = peak[c];
                lv.hold_t[c] = 1.0;
            } else if lv.hold_t[c] > 0.0 {
                lv.hold_t[c] -= dt;
            } else {
                // Fall 20 dB per second.
                lv.peak_hold[c] = (lv.peak_hold[c] * 10f32.powf(-dt)).max(peak[c]);
            }
        }
    }

    /// Linear gain applied by the current sensitivity settings (manual + auto).
    pub fn gain_lin(&self, cfg: &Config) -> f32 {
        10f32.powf((self.auto_offset_db + cfg.sensitivity.gain_db) / 20.0)
    }
}

fn coeff(dt: f32, tau_ms: f32) -> f32 {
    if tau_ms <= 0.0 {
        1.0
    } else {
        1.0 - (-dt / (tau_ms / 1000.0)).exp()
    }
}

fn normalize(lin: f32, gain_lin: f32, scale: Scale, range_db: f32) -> f32 {
    let v = lin * gain_lin;
    let out = match scale {
        Scale::Linear => v,
        Scale::Sqrt => v.max(0.0).sqrt(),
        Scale::Log => {
            if v <= 1e-9 {
                0.0
            } else {
                (to_db(v) + range_db) / range_db
            }
        }
    };
    if out.is_finite() { out.clamp(0.0, 1.0) } else { 0.0 }
}

/// Spreads each bar into its neighbours so the spectrum reads as a smooth shape.
fn monstercat(v: &mut [f32], factor: f32) {
    if factor <= 1.0 || v.len() < 2 {
        return;
    }
    let n = v.len();
    for i in 0..n {
        let base = v[i];
        if base <= 0.0 {
            continue;
        }
        let mut f = factor;
        for d in 1..n {
            let spread = base / f;
            if spread < 0.002 {
                break;
            }
            if i + d < n && v[i + d] < spread {
                v[i + d] = spread;
            }
            if d <= i && v[i - d] < spread {
                v[i - d] = spread;
            }
            f *= factor;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_reads_zero_db() {
        let mut a = Analyzer::new(4096, 44100);
        let s = sine(1000.0, 44100, 4096, 1.0);
        a.compute_mags(MID, &s, Some(&s));
        let peak = a.mags[MID].iter().cloned().fold(0.0f32, f32::max);
        assert!((to_db(peak)).abs() < 0.5, "peak {} dB", to_db(peak));
    }

    #[test]
    fn band_edges_are_monotonic_and_bounded() {
        let b = Bands::build(64, 4096, 44100, 40.0, 16000.0, 0.0);
        assert_eq!(b.edges.len(), 65);
        for w in b.edges.windows(2) {
            assert!(w[1] > w[0]);
        }
        assert!(b.edges[64] <= 2049.0);
    }

    #[test]
    fn tone_lands_in_expected_band() {
        let mut a = Analyzer::new(4096, 44100);
        let s = sine(1000.0, 44100, 4096, 0.5);
        a.compute_mags(MID, &s, Some(&s));
        let b = Bands::build(32, 4096, 44100, 40.0, 16000.0, 0.0);
        let mut out = Vec::new();
        b.apply(&a.mags[MID], BandMode::Peak, &mut out);
        let (imax, _) = out.iter().enumerate().fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
        // 1 kHz on a 40..16000 log axis sits at ln(25)/ln(400) = 53.7% of the way.
        let expected = (0.537 * 32.0) as usize;
        assert!((imax as i32 - expected as i32).abs() <= 1, "band {imax}, expected ~{expected}");
    }

    #[test]
    fn update_produces_sane_bars() {
        let cfg = Config::default();
        let mut a = Analyzer::new(cfg.analysis.fft_size, cfg.audio.sample_rate);
        let n = a.frames_needed(&cfg);
        let l = sine(440.0, 44100, n, 0.5);
        let r = sine(3000.0, 44100, n, 0.5);
        let req = Request { bars: 40, stereo: true, spectro_bins: 80, spectro_rows: 10 };
        for _ in 0..30 {
            a.update(&cfg, &l, &r, 1.0 / 60.0, req);
        }
        assert_eq!(a.ch[LEFT].smooth.len(), 40);
        assert!(a.ch[LEFT].smooth.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(a.ch[LEFT].smooth.iter().cloned().fold(0.0f32, f32::max) > 0.5);
        assert_eq!(a.spectro.len(), 10);
        assert_eq!(a.spectro[0].len(), 80);
        assert!(a.levels.rms[0] > 0.3 && a.levels.rms[0] < 0.4);
        assert!(a.levels.peak_hold[1] > 0.45);
    }

    #[test]
    fn silence_is_zero() {
        let cfg = Config::default();
        let mut a = Analyzer::new(1024, 44100);
        let z = vec![0.0; 8192];
        let req = Request { bars: 16, stereo: false, spectro_bins: 0, spectro_rows: 0 };
        a.update(&cfg, &z, &z, 0.016, req);
        assert!(a.ch[MID].smooth.iter().all(|&v| v == 0.0));
        assert_eq!(a.levels.rms, [0.0, 0.0]);
    }

    #[test]
    fn monstercat_spreads() {
        let mut v = vec![0.0, 0.0, 1.0, 0.0, 0.0];
        monstercat(&mut v, 2.0);
        assert!((v[1] - 0.5).abs() < 1e-6 && (v[3] - 0.5).abs() < 1e-6);
        assert!((v[0] - 0.25).abs() < 1e-6);
    }
}
