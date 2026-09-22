//! Radix-2 iterative FFT (Cooley-Tukey) producing the magnitude half-spectrum
//! of a real signal for the visualizer.
use std::f32::consts::PI;

/// FFT engine with a pre-built twiddle table.
pub struct Fft {
    n: usize,
    tw_re: Vec<f32>,
    tw_im: Vec<f32>,
}

impl Fft {
    pub fn new(mut n: usize) -> Self {
        if n < 2 { n = 2; }
        let mut size = 1usize;
        while size < n { size <<= 1; }
        let mut tw_re = Vec::with_capacity(size);
        let mut tw_im = Vec::with_capacity(size);
        for k in 0..size {
            let t = PI * 2.0 * (k as f32 / size as f32);
            tw_re.push(t.cos());
            tw_im.push(t.sin());
        }
        Self { n: size, tw_re, tw_im }
    }

    pub fn n(&self) -> usize { self.n }

    /// FFT of x (length self.n) -> half-spectrum magnitudes in out (len n/2+1).
    pub fn transform(&self, x: &[f32], out: &mut Vec<f32>) {
        let n = self.n;
        let mut re = vec![0.0f32; n];
        let mut im = vec![0.0f32; n];
        for i in 0..n { re[i] = x[i]; }

        // Bit-reversal
        let log2n = n.ilog2() as u32;
        for i in 0..n {
            let j = bit_reverse(i as u32, log2n) as usize;
            if i > j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }

        // Iterative Cooley-Tukey
        let mut size = 2usize;
        while size <= n {
            let half = size / 2;
            let wlen = n / size;
            for i in (0..n).step_by(size) {
                for k in 0..half {
                    let widx = k * wlen;
                    let w_re = self.tw_re[widx];
                    let w_im = self.tw_im[widx];
                    let a = re[i + k];
                    let b = im[i + k];
                    let c = re[i + k + half];
                    let d = im[i + k + half];
                    let t_re = c * w_re - d * w_im;
                    let t_im = c * w_im + d * w_re;
                    re[i + k] = a + t_re;
                    im[i + k] = b + t_im;
                    re[i + k + half] = a - t_re;
                    im[i + k + half] = b - t_im;
                }
            }
            size <<= 1;
        }

        for i in 0..=n / 2 {
            out[i] = (re[i].powi(2) + im[i].powi(2)).sqrt();
        }
    }
}

fn bit_reverse(mut x: u32, bits: u32) -> u32 {
    let mut y = 0u32;
    for i in 0..bits {
        y |= (x & 1) << (bits - 1 - i);
        x >>= 1;
    }
    y
}

/// FFT of x (length x.len()) -> half-spectrum magnitudes in out (len x.len()/2+1).
pub fn fft_transform(x: &[f32], out: &mut Vec<f32>) {
    let fft = Fft::new(x.len().max(2));
    let mut z = vec![0.0f32; fft.n()];
    for i in 0..x.len() { z[i] = x[i]; }
    fft.transform(&z, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_dc_signal() {
        let n = 16;
        let x = vec![1.0f32; n];
        let mut out = vec![0.0f32; n / 2 + 1];
        fft_transform(&x, &mut out);
        assert!((out[0] - 16.0).abs() < 1e-3);
        for i in 1..out.len() {
            assert!((out[i] - 0.0).abs() < 1e-3, "out[{}] = {}", i, out[i]);
        }
    }

    #[test]
    fn test_sin_freq_2() {
        let n = 16;
        let x: Vec<f32> = (0..n).map(|i| (2.0 * PI * 2.0 * i as f32 / n as f32).sin()).collect();
        let mut out = vec![0.0f32; n / 2 + 1];
        fft_transform(&x, &mut out);
        assert!((out[2] - 8.0).abs() < 0.5);
    }

    #[test]
    fn test_sin_freq_7() {
        let n = 16;
        let x: Vec<f32> = (0..n).map(|i| (2.0 * PI * 7.0 * i as f32 / n as f32).sin()).collect();
        let mut out = vec![0.0f32; n / 2 + 1];
        fft_transform(&x, &mut out);
        assert!((out[7] - 8.0).abs() < 0.5);
    }
}
