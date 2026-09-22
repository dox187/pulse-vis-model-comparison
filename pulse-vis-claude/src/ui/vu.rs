//! Stereo level meters: RMS bar, peak-hold marker, dB scale and correlation.

use super::LEFT_EIGHTHS;
use crate::app::App;
use crate::dsp::to_db;
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let lv = app.analyzer.levels;
    let range = app.cfg.sensitivity.dynamic_range_db.max(30.0);
    let thick: u16 = if area.height >= 14 { 3 } else if area.height >= 9 { 2 } else { 1 };
    let total = 2 * thick + 5;
    let y0 = area.y + area.height.saturating_sub(total) / 2;
    let label_w: u16 = 3;
    let read_w: u16 = 22;
    let bar_x = area.x + label_w;
    let bar_w = area.width.saturating_sub(label_w + read_w).max(8).min(area.width.saturating_sub(label_w));

    for c in 0..2 {
        let y = y0 + c as u16 * (thick + 1);
        draw_meter(
            buf,
            theme,
            area.x + 1,
            y,
            thick,
            bar_x,
            bar_w,
            if c == 0 { "L" } else { "R" },
            lv.rms[c],
            lv.peak_hold[c],
            lv.clip[c],
            range,
        );
    }

    // dB scale.
    let scale_y = y0 + 2 * thick + 2;
    if scale_y < area.y + area.height {
        let dim = Style::default().fg(theme.dim_text());
        let step = if bar_w >= 60 { 10 } else if bar_w >= 30 { 20 } else { 30 };
        let mut db = -(range as i32) / step * step;
        while db <= 0 {
            let x = bar_x + (((db as f32 + range) / range).clamp(0.0, 1.0) * (bar_w - 1) as f32).round() as u16;
            let label = if db == 0 { "0".to_string() } else { db.to_string() };
            let lx = x.saturating_sub((label.len() / 2) as u16).max(bar_x);
            if lx + label.len() as u16 <= bar_x + bar_w {
                buf.set_string(lx, scale_y, &label, dim);
            }
            db += step;
        }
    }

    // Correlation.
    let corr_y = y0 + 2 * thick + 4;
    if corr_y < area.y + area.height && bar_w >= 12 {
        let dim = Style::default().fg(theme.dim_text());
        buf.set_string(area.x + 1, corr_y, "Φ", dim);
        buf.set_string(bar_x, corr_y, "-1", dim);
        let track_x = bar_x + 3;
        let track_w = bar_w.saturating_sub(6);
        for x in track_x..track_x + track_w {
            if let Some(cell) = buf.cell_mut((x, corr_y)) {
                cell.set_char('─').set_style(dim);
            }
        }
        buf.set_string(track_x + track_w + 1, corr_y, "+1", dim);
        let pos = track_x + (((lv.correlation + 1.0) / 2.0).clamp(0.0, 1.0) * (track_w.max(1) - 1) as f32).round() as u16;
        let col = theme.color(((lv.correlation + 1.0) / 2.0).clamp(0.0, 1.0));
        if let Some(cell) = buf.cell_mut((pos, corr_y)) {
            cell.set_char('●').set_fg(col);
        }
        let txt = format!("{:+.2}", lv.correlation);
        buf.set_string(bar_x + bar_w + 2, corr_y, txt, Style::default().fg(theme.text()));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_meter(
    buf: &mut Buffer,
    theme: &Theme,
    label_x: u16,
    y: u16,
    thick: u16,
    bar_x: u16,
    bar_w: u16,
    label: &str,
    rms: f32,
    peak_hold: f32,
    clip: bool,
    range: f32,
) {
    buf.set_string(label_x, y + thick / 2, label, Style::default().fg(theme.text()).add_modifier(Modifier::BOLD));
    let rms_db = to_db(rms);
    let peak_db = to_db(peak_hold);
    let fill8 = (((rms_db + range) / range).clamp(0.0, 1.0) * bar_w as f32 * 8.0).round() as i32;
    let bg = theme.bg();
    for row in 0..thick {
        for col in 0..bar_w {
            let t = if bar_w > 1 { col as f32 / (bar_w - 1) as f32 } else { 0.0 };
            let filled = fill8 - col as i32 * 8;
            let Some(cell) = buf.cell_mut((bar_x + col, y + row)) else { continue };
            if filled >= 8 {
                cell.set_char('█').set_fg(theme.color(t)).set_bg(bg);
            } else if filled > 0 {
                cell.set_char(LEFT_EIGHTHS[filled as usize]).set_fg(theme.color(t)).set_bg(bg);
            } else {
                cell.set_char('░').set_fg(theme.faded(t, 0.3)).set_bg(bg);
            }
        }
    }
    if peak_hold > 0.0 {
        let px = bar_x + (((peak_db + range) / range).clamp(0.0, 1.0) * (bar_w - 1) as f32).round() as u16;
        let col = if clip { Color::Rgb(255, 40, 40) } else { theme.peak() };
        for row in 0..thick {
            if let Some(cell) = buf.cell_mut((px, y + row)) {
                cell.set_char('┃').set_fg(col);
            }
        }
    }
    let text = format!(" {:>6.1} dB  pk {:>6.1}", rms_db.max(-99.9), peak_db.max(-99.9));
    buf.set_string(bar_x + bar_w, y + thick / 2, text, Style::default().fg(theme.text()));
    if clip && thick > 1 {
        buf.set_string(
            bar_x + bar_w + 1,
            y,
            "CLIP",
            Style::default().fg(Color::Rgb(255, 40, 40)).add_modifier(Modifier::BOLD),
        );
    }
}
