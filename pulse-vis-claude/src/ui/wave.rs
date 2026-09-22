//! Oscilloscope: the raw waveform of the last few tens of milliseconds.

use crate::app::App;
use crate::config::{Channels, WaveStyle};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line, Points};

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let cfg = &app.cfg;
    let avail = app.snap_l.len().min(app.snap_r.len());
    if avail == 0 {
        return;
    }
    let n = ((cfg.audio.sample_rate as f32 * cfg.display.wave_ms / 1000.0) as usize).clamp(16, avail);
    let l = &app.snap_l[avail - n..];
    let r = &app.snap_r[avail - n..];
    let gain = app.analyzer.gain_lin(cfg);
    if cfg.display.channels == Channels::Stereo && area.height >= 4 {
        let top = Rect { height: area.height / 2, ..area };
        let bottom = Rect { y: area.y + top.height, height: area.height - top.height, ..area };
        draw_channel(app, top, buf, l, gain, Some("L"));
        draw_channel(app, bottom, buf, r, gain, Some("R"));
    } else {
        let mono: Vec<f32> = l.iter().zip(r).map(|(a, b)| (a + b) * 0.5).collect();
        draw_channel(app, area, buf, &mono, gain, None);
    }
}

fn draw_channel(app: &App, area: Rect, buf: &mut Buffer, samples: &[f32], gain: f32, label: Option<&str>) {
    let theme = &app.theme;
    let style = app.cfg.display.wave_style;
    if style != WaveStyle::Filled {
        let mid_y = area.y + area.height / 2;
        let axis = Style::default().fg(theme.dim_text());
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, mid_y)) {
                cell.set_char('╌').set_style(axis);
            }
        }
    }
    match style {
        WaveStyle::Filled => draw_filled(app, area, buf, samples, gain),
        WaveStyle::Line | WaveStyle::Dots => {
            let cols = (area.width as usize * 2).max(2);
            let chunk = (samples.len() / cols).max(1);
            let ys: Vec<f64> = (0..cols)
                .map(|j| {
                    let s = (j * samples.len() / cols).min(samples.len() - 1);
                    let e = (s + chunk).min(samples.len());
                    let avg = samples[s..e].iter().sum::<f32>() / (e - s) as f32;
                    (avg * gain).clamp(-1.0, 1.0) as f64
                })
                .collect();
            let mut buckets: Vec<Vec<(f64, f64)>> = vec![Vec::new(); 8];
            for (j, &y) in ys.iter().enumerate() {
                let b = ((y.abs() * 7.999) as usize).min(7);
                buckets[b].push((j as f64, y));
            }
            Canvas::default()
                .marker(super::marker(app.cfg.display.marker))
                .x_bounds([0.0, cols as f64])
                .y_bounds([-1.05, 1.05])
                .paint(|ctx| {
                    if style == WaveStyle::Line {
                        for j in 0..cols - 1 {
                            let amp = ys[j].abs().max(ys[j + 1].abs()) as f32;
                            ctx.draw(&Line {
                                x1: j as f64,
                                y1: ys[j],
                                x2: (j + 1) as f64,
                                y2: ys[j + 1],
                                color: theme.color(amp),
                            });
                        }
                    } else {
                        for (b, pts) in buckets.iter().enumerate() {
                            if !pts.is_empty() {
                                ctx.draw(&Points { coords: pts, color: theme.color((b as f32 + 0.5) / 8.0) });
                            }
                        }
                    }
                })
                .render(area, buf);
        }
    }
    if let Some(label) = label {
        buf.set_string(area.x + 1, area.y, label, Style::default().fg(theme.dim_text()));
    }
}

fn draw_filled(app: &App, area: Rect, buf: &mut Buffer, samples: &[f32], gain: f32) {
    let theme = &app.theme;
    let bg = theme.bg();
    let cols = area.width as usize;
    if cols == 0 || area.height == 0 {
        return;
    }
    let chunk = (samples.len() / cols).max(1);
    let hh = area.height as f32; // half-rows from centre to edge
    for x in 0..cols {
        let s = (x * samples.len() / cols).min(samples.len() - 1);
        let e = (s + chunk).min(samples.len());
        let (mut mn, mut mx) = (f32::MAX, f32::MIN);
        for &v in &samples[s..e] {
            let v = (v * gain).clamp(-1.0, 1.0);
            mn = mn.min(v);
            mx = mx.max(v);
        }
        let top_hr = ((1.0 - mx) * hh).round() as i32;
        let bot_hr = ((1.0 - mn) * hh).round() as i32;
        let a = top_hr.min(bot_hr);
        let b = top_hr.max(bot_hr).max(a + 1);
        for y in 0..area.height as i32 {
            let upper = 2 * y >= a && 2 * y < b;
            let lower = 2 * y + 1 >= a && 2 * y + 1 < b;
            let glyph = match (upper, lower) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                _ => continue,
            };
            let t = ((y as f32 + 0.5) - hh / 2.0).abs() / (hh / 2.0);
            if let Some(cell) = buf.cell_mut((area.x + x as u16, area.y + y as u16)) {
                cell.set_char(glyph).set_fg(theme.color(t)).set_bg(bg);
            }
        }
    }
}
