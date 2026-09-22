use crate::{
    audio::{RATE, Sample},
    config::Config,
};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::Arc;

pub struct Analysis {
    fft: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    pub bands: Vec<f32>,
    pub peaks: Vec<f32>,
    pub rms: [f32; 2],
    pub peak: [f32; 2],
    pub correlation: f32,
    pub gain: f32,
}
impl Analysis {
    pub fn new(size: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(size);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Self {
            fft,
            buffer: vec![Complex::default(); size],
            scratch,
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (size - 1) as f32).cos())
                .collect(),
            bands: vec![],
            peaks: vec![],
            rms: [0.0; 2],
            peak: [0.0; 2],
            correlation: 0.0,
            gain: 1.0,
        }
    }
    pub fn update(&mut self, samples: &[Sample], count: usize, config: &Config, dt: f32) {
        if self.buffer.len() != config.quality.size() {
            *self = Self::new(config.quality.size());
        }
        let size = self.buffer.len();
        let n = samples.len().min(size);
        let samples = &samples[samples.len() - n..];
        self.rms = [0.0; 2];
        self.peak = [0.0; 2];
        let mut cross = 0.0;
        for sample in samples {
            for (ch, value) in sample.iter().enumerate() {
                self.rms[ch] += value * value;
                self.peak[ch] = self.peak[ch].max(value.abs());
            }
            cross += sample[0] * sample[1];
        }
        self.correlation = if self.rms[0] * self.rms[1] > 1e-12 {
            (cross / (self.rms[0] * self.rms[1]).sqrt()).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.rms
            .iter_mut()
            .for_each(|v| *v = (*v / n.max(1) as f32).sqrt());
        let level = self.rms[0].max(self.rms[1]);
        let desired = if config.auto_gain && level > 0.0001 {
            (0.18 / level).clamp(0.1, 20.0)
        } else {
            1.0
        };
        let speed = if desired < self.gain { 12.0 } else { 1.5 };
        self.gain += (desired - self.gain) * (1.0 - (-dt * speed).exp());
        // Average channel powers, not samples: anti-phase stereo must remain visible.
        let mut power = vec![0.0; size / 2 + 1];
        for ch in [0, 1] {
            for i in 0..size {
                self.buffer[i] = Complex::new(
                    if i < size - n {
                        0.0
                    } else {
                        samples[i - (size - n)][ch] * self.window[i]
                    },
                    0.0,
                );
            }
            self.fft
                .process_with_scratch(&mut self.buffer, &mut self.scratch);
            for (p, bin) in power.iter_mut().zip(&self.buffer) {
                *p += bin.norm_sqr() * 0.5;
            }
        }
        let count = count.max(1);
        self.bands.resize(count, 0.0);
        self.peaks.resize(count, 0.0);
        let retention = config.smoothing.powf(dt * 60.0);
        for band in 0..count {
            let low = config.min_frequency
                * (config.max_frequency / config.min_frequency).powf(band as f32 / count as f32);
            let high = config.min_frequency
                * (config.max_frequency / config.min_frequency)
                    .powf((band + 1) as f32 / count as f32);
            let start = ((low * size as f32 / RATE as f32).floor() as usize)
                .max(1)
                .min(size / 2);
            let end = ((high * size as f32 / RATE as f32).ceil() as usize)
                .max(start + 1)
                .min(size / 2 + 1);
            let amplitude = power[start..end]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max)
                .sqrt()
                * 4.0
                / size as f32
                * config.sensitivity
                * self.gain;
            let db = 20.0 * amplitude.max(1e-9).log10();
            let target = ((db - config.floor_db) / -config.floor_db).clamp(0.0, 1.0);
            self.bands[band] = retention * self.bands[band] + (1.0 - retention) * target;
            self.peaks[band] = (self.peaks[band] - dt * 0.3).max(self.bands[band]);
        }
    }
}
pub fn db(value: f32) -> f32 {
    (20.0 * value.max(1e-6).log10()).max(-120.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_tone_and_preserves_antiphase() {
        let config = Config {
            auto_gain: false,
            smoothing: 0.0,
            ..Config::default()
        };
        let samples: Vec<_> = (0..4096)
            .map(|i| {
                let x = (std::f32::consts::TAU * 1000.0 * i as f32 / RATE as f32).sin() * 0.5;
                [x, -x]
            })
            .collect();
        let mut a = Analysis::new(4096);
        a.update(&samples, 64, &config, 1.0 / 60.0);
        let peak = a
            .bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let freq = config.min_frequency
            * (config.max_frequency / config.min_frequency).powf((peak as f32 + 0.5) / 64.0);
        assert!((freq - 1000.0).abs() < 150.0, "{freq}");
        assert!(a.bands[peak] > 0.8);
        assert!(a.correlation < -0.99);
        assert!((a.rms[0] - 0.3535).abs() < 0.01);
    }
    #[test]
    fn silence_decays_and_quality_can_change() {
        let mut c = Config::default();
        let mut a = Analysis::new(4096);
        a.update(&vec![[0.5; 2]; 4096], 50, &c, 0.1);
        c.quality = crate::config::Quality::Low;
        for _ in 0..200 {
            a.update(&vec![[0.0; 2]; 1024], 50, &c, 0.1);
        }
        assert!(a.bands.iter().all(|v| *v < 0.001));
        assert_eq!(a.rms, [0.0; 2]);
    }
}
