//! Classic spectrum analyser: vertical bars with 1/8-cell resolution and falling peak caps.

use super::EIGHTHS;
use crate::app::App;
use crate::config::{Channels, Config};
use crate::dsp::{LEFT, MID, RIGHT};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

pub struct BarLayout {
    /// Bars drawn on screen (both channels together in stereo).
    pub slots: usize,
    /// Bands the analyser must produce per channel.
    pub per_channel: usize,
    pub bar_w: u16,
    pub gap: u16,
    /// Left margin that centres the bars.
    pub x0: u16,
}

pub fn layout(width: u16, cfg: &Config, stereo: bool) -> BarLayout {
    let width = width.max(1) as usize;
    let mut gap = cfg.analysis.bar_gap as usize;
    let mut bar_w = cfg.analysis.bar_width.max(1) as usize;
    let mut slots;
    if cfg.analysis.bars > 0 {
        slots = cfg.analysis.bars.min(width).max(1);
        if stereo {
            slots = slots.max(2);
            slots -= slots % 2;
        }
        if slots * (1 + gap) - gap > width {
            gap = 0;
        }
        bar_w = ((width + gap) / slots).saturating_sub(gap).max(1);
    } else {
        slots = ((width + gap) / (bar_w + gap)).max(1);
        if stereo {
            slots = slots.max(2);
            slots -= slots % 2;
        }
    }
    let mut used = slots * bar_w + (slots - 1) * gap;
    if used > width {
        // Very narrow terminal: fall back to 1-cell bars without gaps.
        gap = 0;
        bar_w = 1;
        used = slots;
    }
    BarLayout {
        slots,
        per_channel: if stereo { slots / 2 } else { slots },
        bar_w: bar_w as u16,
        gap: gap as u16,
        x0: (width.saturating_sub(used) / 2) as u16,
    }
}

/// `(level, peak, frequency position 0..1)` for every slot, left to right.
pub fn slot_values(app: &App, lay: &BarLayout) -> Vec<(f32, f32, f32)> {
    let an = &app.analyzer;
    let mut out = Vec::with_capacity(lay.slots);
    let get = |ch: usize, i: usize| -> (f32, f32) {
        (
            an.ch[ch].smooth.get(i).copied().unwrap_or(0.0),
            an.ch[ch].peak.get(i).copied().unwrap_or(0.0),
        )
    };
    let n = lay.per_channel.max(1);
    let ft = |i: usize| if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
    if an.stereo && lay.per_channel * 2 == lay.slots {
        for i in 0..lay.per_channel {
            let band = lay.per_channel - 1 - i;
            let (v, p) = get(LEFT, band);
            out.push((v, p, ft(band)));
        }
        for band in 0..lay.per_channel {
            let (v, p) = get(RIGHT, band);
            out.push((v, p, ft(band)));
        }
    } else {
        for band in 0..lay.slots {
            let (v, p) = get(MID, band);
            out.push((v, p, ft(band)));
        }
    }
    out
}

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let stereo = app.cfg.display.channels == Channels::Stereo;
    let lay = layout(area.width, &app.cfg, stereo);
    let vals = slot_values(app, &lay);
    let bottom = area.y + area.height - 1;
    let bg = app.theme.bg();
    let peak_color = app.theme.peak();
    for (slot, &(v, p, ft)) in vals.iter().enumerate() {
        let x_start = area.x + lay.x0 + slot as u16 * (lay.bar_w + lay.gap);
        for dx in 0..lay.bar_w {
            let x = x_start + dx;
            if x >= area.x + area.width {
                break;
            }
            draw_bar_up(buf, x, bottom, area.height, v, |row_t| app.theme.bar_color(ft, row_t, v), bg);
            if app.cfg.display.peaks {
                draw_peak_up(buf, x, bottom, area.height, v, p, peak_color);
            }
        }
    }
}

/// Draws a bar of relative height `v` growing upwards from row `bottom`.
pub fn draw_bar_up(
    buf: &mut Buffer,
    x: u16,
    bottom: u16,
    height: u16,
    v: f32,
    color: impl Fn(f32) -> Color,
    bg: Color,
) {
    let total8 = (v.clamp(0.0, 1.0) * height as f32 * 8.0).round() as i32;
    for row in 0..height as i32 {
        let filled = total8 - row * 8;
        if filled <= 0 {
            break;
        }
        let idx = filled.min(8) as usize;
        let t = (row as f32 + 0.5) / height as f32;
        if let Some(cell) = buf.cell_mut((x, bottom - row as u16)) {
            cell.set_char(EIGHTHS[idx]).set_fg(color(t)).set_bg(bg);
        }
    }
}

/// Peak cap above an upward bar; skipped when the bar itself covers that cell.
pub fn draw_peak_up(buf: &mut Buffer, x: u16, bottom: u16, height: u16, v: f32, p: f32, color: Color) {
    let Some((row, upper)) = peak_cell(height, v, p) else { return };
    if let Some(cell) = buf.cell_mut((x, bottom - row)) {
        cell.set_char(if upper { '▔' } else { '▁' }).set_fg(color);
    }
}

/// Which cell (counted from the bar's base) holds the peak line and whether the line sits in
/// the upper half of that cell. `None` when the bar covers the cell or there is no peak.
pub fn peak_cell(height: u16, v: f32, p: f32) -> Option<(u16, bool)> {
    if p <= 0.0 || height == 0 {
        return None;
    }
    let v8 = (v.clamp(0.0, 1.0) * height as f32 * 8.0).round() as i32;
    let bar_top_row = (v8 + 7) / 8 - 1;
    let p8 = (p.clamp(0.0, 1.0) * height as f32 * 8.0).round() as i32;
    let pos = (p8 - 1).max(0);
    let row = (pos / 8).min(height as i32 - 1);
    if row <= bar_top_row {
        return None;
    }
    Some((row as u16, pos % 8 >= 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits_width() {
        let cfg = Config::default();
        for w in 1..200u16 {
            for stereo in [false, true] {
                let l = layout(w, &cfg, stereo);
                let used = l.slots * l.bar_w as usize + (l.slots - 1) * l.gap as usize;
                assert!(used <= w.max(2) as usize, "w={w} stereo={stereo} used={used}");
                if stereo {
                    assert_eq!(l.slots % 2, 0);
                }
            }
        }
    }

    #[test]
    fn fixed_bar_count() {
        let mut cfg = Config::default();
        cfg.analysis.bars = 30;
        let l = layout(120, &cfg, false);
        assert_eq!(l.slots, 30);
        assert_eq!(l.bar_w, 3);
        let l = layout(20, &cfg, false);
        assert_eq!(l.slots, 20);
        assert_eq!(l.bar_w, 1);
    }

    #[test]
    fn peak_hidden_inside_bar() {
        assert_eq!(peak_cell(10, 0.5, 0.5), None);
        assert_eq!(peak_cell(10, 0.5, 0.0), None);
        let (row, _) = peak_cell(10, 0.2, 0.95).unwrap();
        assert_eq!(row, 9);
    }
}
