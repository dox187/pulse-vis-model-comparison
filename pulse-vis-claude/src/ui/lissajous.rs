//! Vectorscope / goniometer: left channel against right channel.

use crate::app::App;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Circle, Line, Points};
use std::f32::consts::FRAC_1_SQRT_2;

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let cfg = &app.cfg;
    let theme = &app.theme;
    let avail = app.snap_l.len().min(app.snap_r.len());
    if avail == 0 {
        return;
    }
    let n = ((cfg.audio.sample_rate as f32 * cfg.display.xy_ms / 1000.0) as usize).clamp(64, avail);
    let l = &app.snap_l[avail - n..];
    let r = &app.snap_r[avail - n..];
    let gain = app.analyzer.gain_lin(cfg);
    let rotate = cfg.display.lissajous_rotate;

    // Braille dots are roughly square: 2 per cell horizontally, 4 vertically.
    let wd = area.width as f64 * 2.0;
    let hd = area.height as f64 * 4.0;
    let m = wd.min(hd).max(1.0);
    let (ax, ay) = (wd / m * 1.05, hd / m * 1.05);

    const BUCKETS: usize = 6;
    let mut groups: Vec<Vec<(f64, f64)>> = (0..BUCKETS).map(|_| Vec::with_capacity(n / BUCKETS + 1)).collect();
    for (i, (&x, &y)) in l.iter().zip(r).enumerate() {
        let (px, py) = if rotate {
            ((x - y) * FRAC_1_SQRT_2, (x + y) * FRAC_1_SQRT_2)
        } else {
            (x, y)
        };
        let px = (px * gain).clamp(-1.0, 1.0) as f64;
        let py = (py * gain).clamp(-1.0, 1.0) as f64;
        groups[(i * BUCKETS / n).min(BUCKETS - 1)].push((px, py));
    }

    let axis = theme.dim_text();
    Canvas::default()
        .marker(super::marker(cfg.display.marker))
        .x_bounds([-ax, ax])
        .y_bounds([-ay, ay])
        .paint(|ctx| {
            if rotate {
                ctx.draw(&Line { x1: -1.0, y1: -1.0, x2: 1.0, y2: 1.0, color: axis });
                ctx.draw(&Line { x1: -1.0, y1: 1.0, x2: 1.0, y2: -1.0, color: axis });
                ctx.draw(&Line { x1: 0.0, y1: -1.0, x2: 0.0, y2: 1.0, color: axis });
            } else {
                ctx.draw(&Line { x1: -1.0, y1: 0.0, x2: 1.0, y2: 0.0, color: axis });
                ctx.draw(&Line { x1: 0.0, y1: -1.0, x2: 0.0, y2: 1.0, color: axis });
            }
            ctx.draw(&Circle { x: 0.0, y: 0.0, radius: 1.0, color: axis });
            for (b, pts) in groups.iter().enumerate() {
                if pts.is_empty() {
                    continue;
                }
                let t = (b as f32 + 1.0) / BUCKETS as f32;
                ctx.draw(&Points { coords: pts, color: theme.faded(t, 0.35 + 0.65 * t) });
            }
        })
        .render(area, buf);

    let dim = Style::default().fg(theme.dim_text());
    let text = Style::default().fg(theme.text());
    if rotate {
        buf.set_string(area.x + 1, area.y, "L", dim);
        buf.set_string(area.x + area.width.saturating_sub(2), area.y, "R", dim);
    } else {
        buf.set_string(area.x + area.width.saturating_sub(2), area.y + area.height / 2, "L", dim);
        buf.set_string(area.x + area.width / 2, area.y, "R", dim);
    }
    let corr = format!("Φ {:+.2}", app.analyzer.levels.correlation);
    buf.set_string(area.x + 1, area.y + area.height - 1, corr, text);
}
