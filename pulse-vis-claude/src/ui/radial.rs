//! Spectrum bars arranged around a circle.

use crate::app::App;
use crate::dsp::{LEFT, MID, RIGHT};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Circle, Line, Points};
use std::f64::consts::PI;

/// Bands per channel for a given area, when not fixed by the configuration.
pub fn auto_bands(area: Rect, stereo: bool) -> usize {
    let m = (area.width as usize * 2).min(area.height as usize * 4).max(8);
    let total = (m / 2).clamp(16, 96);
    if stereo { (total / 2).max(8) } else { total }
}

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let an = &app.analyzer;
    let theme = &app.theme;
    let n = an.bars;
    if n == 0 {
        return;
    }
    let stereo = an.stereo;
    let wd = area.width as f64 * 2.0;
    let hd = area.height as f64 * 4.0;
    let m = wd.min(hd).max(1.0);
    let (ax, ay) = (wd / m, hd / m);
    let r0 = 0.30;
    let r1 = 0.97;
    let total = if stereo { 2 * n } else { n };
    let delta = PI / total as f64 * 0.55;
    const SEGS: usize = 4;
    let show_peaks = app.cfg.display.peaks;
    let peak_color = theme.peak();

    Canvas::default()
        .marker(super::marker(app.cfg.display.marker))
        .x_bounds([-ax, ax])
        .y_bounds([-ay, ay])
        .paint(|ctx| {
            ctx.draw(&Circle { x: 0.0, y: 0.0, radius: r0 * 0.9, color: theme.dim_text() });
            let mut peak_pts: Vec<(f64, f64)> = Vec::new();
            for slot in 0..total {
                let (ch, band, theta) = if stereo {
                    if slot < n {
                        (RIGHT, slot, PI / 2.0 - PI * (slot as f64 + 0.5) / n as f64)
                    } else {
                        let i = slot - n;
                        (LEFT, i, PI / 2.0 + PI * (i as f64 + 0.5) / n as f64)
                    }
                } else {
                    (MID, slot, PI / 2.0 - 2.0 * PI * (slot as f64 + 0.5) / n as f64)
                };
                let v = an.ch[ch].smooth.get(band).copied().unwrap_or(0.0);
                let p = an.ch[ch].peak.get(band).copied().unwrap_or(0.0);
                let ft = if n > 1 { band as f32 / (n - 1) as f32 } else { 0.0 };
                let len = r0 + v as f64 * (r1 - r0);
                for &dth in &[-delta, 0.0, delta] {
                    let (s, c) = (theta + dth).sin_cos();
                    for seg in 0..SEGS {
                        let a = r0 + (len - r0) * seg as f64 / SEGS as f64;
                        let b = r0 + (len - r0) * (seg + 1) as f64 / SEGS as f64;
                        if b - a < 1e-4 {
                            break;
                        }
                        let row_t = ((seg as f32 + 0.5) / SEGS as f32) * v;
                        ctx.draw(&Line {
                            x1: a * c,
                            y1: a * s,
                            x2: b * c,
                            y2: b * s,
                            color: theme.bar_color(ft, row_t, v),
                        });
                    }
                }
                if show_peaks && p > v + 0.02 {
                    let (s, c) = theta.sin_cos();
                    let rp = r0 + p as f64 * (r1 - r0);
                    peak_pts.push((rp * c, rp * s));
                }
            }
            if !peak_pts.is_empty() {
                ctx.draw(&Points { coords: &peak_pts, color: peak_color });
            }
        })
        .render(area, buf);
}
