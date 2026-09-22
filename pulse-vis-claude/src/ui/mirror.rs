//! Spectrum mirrored around the horizontal centre line.

use super::EIGHTHS;
use super::bars::{draw_bar_up, draw_peak_up, layout, peak_cell, slot_values};
use crate::app::App;
use crate::config::Channels;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let stereo = app.cfg.display.channels == Channels::Stereo;
    let lay = layout(area.width, &app.cfg, stereo);
    let vals = slot_values(app, &lay);
    let half = area.height / 2;
    if half == 0 {
        return;
    }
    let top_of_lower = area.y + half;
    let bottom_of_upper = top_of_lower - 1;
    let bg = app.theme.bg();
    let bg_concrete = app.theme.background.map(|c| app.theme.convert(c));
    let peak_color = app.theme.peak();
    for (slot, &(v, p, ft)) in vals.iter().enumerate() {
        let x_start = area.x + lay.x0 + slot as u16 * (lay.bar_w + lay.gap);
        for dx in 0..lay.bar_w {
            let x = x_start + dx;
            if x >= area.x + area.width {
                break;
            }
            let color = |row_t: f32| app.theme.bar_color(ft, row_t, v);
            draw_bar_up(buf, x, bottom_of_upper, half, v, color, bg);
            draw_bar_down(buf, x, top_of_lower, half, v, color, bg, bg_concrete);
            if app.cfg.display.peaks {
                draw_peak_up(buf, x, bottom_of_upper, half, v, p, peak_color);
                if let Some((row, upper)) = peak_cell(half, v, p)
                    && let Some(cell) = buf.cell_mut((x, top_of_lower + row))
                {
                    cell.set_char(if upper { '▁' } else { '▔' }).set_fg(peak_color);
                }
            }
        }
    }
}

/// Draws a bar growing downwards from row `top`. With a concrete background colour the
/// fractional cell is drawn by inverting foreground and background, giving 1/8 resolution;
/// otherwise it falls back to half-cell resolution.
#[allow(clippy::too_many_arguments)]
pub fn draw_bar_down(
    buf: &mut Buffer,
    x: u16,
    top: u16,
    height: u16,
    v: f32,
    color: impl Fn(f32) -> Color,
    bg: Color,
    bg_concrete: Option<Color>,
) {
    let total8 = (v.clamp(0.0, 1.0) * height as f32 * 8.0).round() as i32;
    for row in 0..height as i32 {
        let filled = total8 - row * 8;
        if filled <= 0 {
            break;
        }
        let idx = filled.min(8) as usize;
        let t = (row as f32 + 0.5) / height as f32;
        let c = color(t);
        let Some(cell) = buf.cell_mut((x, top + row as u16)) else { continue };
        if idx == 8 {
            cell.set_char('█').set_fg(c).set_bg(bg);
        } else if let Some(bgc) = bg_concrete {
            cell.set_char(EIGHTHS[8 - idx]).set_fg(bgc).set_bg(c);
        } else if idx >= 4 {
            cell.set_char('▀').set_fg(c).set_bg(bg);
        }
    }
}
