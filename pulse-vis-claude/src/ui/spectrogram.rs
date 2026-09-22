//! Scrolling spectrogram drawn with half-block characters (two rows of data per cell).

use crate::app::App;
use crate::config::SpectrogramDirection;
use crate::theme::{Rgb, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

fn heat(theme: &Theme, v: f32) -> Color {
    if v.is_nan() || v <= 0.002 {
        return theme.bg();
    }
    let v = v.clamp(0.0, 1.0);
    Rgb::lerp(theme.bg_rgb(), theme.rgb(v), v.powf(0.6)).to_color(theme.truecolor)
}

pub fn render(app: &App, area: Rect, buf: &mut Buffer) {
    let rows = &app.analyzer.spectro;
    if rows.is_empty() {
        return;
    }
    let theme = &app.theme;
    let h = area.height as usize;
    let w = area.width as usize;
    let len = rows.len() as i64;
    let get = |i: i64, x: usize| -> f32 {
        if i < 0 {
            return 0.0;
        }
        rows.get(i as usize).and_then(|r| r.get(x)).copied().unwrap_or(0.0)
    };
    match app.cfg.display.spectrogram_direction {
        SpectrogramDirection::Up | SpectrogramDirection::Down => {
            let newest_bottom = app.cfg.display.spectrogram_direction == SpectrogramDirection::Up;
            for y in 0..h {
                let k = if newest_bottom { h - 1 - y } else { y } as i64;
                // Two data rows per cell; the newer one sits closer to the newest edge.
                let (upper_i, lower_i) = if newest_bottom {
                    (len - 1 - (2 * k + 1), len - 1 - 2 * k)
                } else {
                    (len - 1 - 2 * k, len - 1 - (2 * k + 1))
                };
                if upper_i < 0 && lower_i < 0 {
                    continue;
                }
                for x in 0..w {
                    let fu = heat(theme, get(upper_i, x));
                    let fl = heat(theme, get(lower_i, x));
                    if let Some(cell) = buf.cell_mut((area.x + x as u16, area.y + y as u16)) {
                        cell.set_char('▀').set_fg(fu).set_bg(fl);
                    }
                }
            }
        }
        SpectrogramDirection::Left => {
            for x in 0..w {
                let c = (w - 1 - x) as i64;
                let ri = len - 1 - c;
                if ri < 0 {
                    continue;
                }
                for y in 0..h {
                    let f_upper = 2 * (h - 1 - y) + 1;
                    let f_lower = 2 * (h - 1 - y);
                    let fu = heat(theme, get(ri, f_upper));
                    let fl = heat(theme, get(ri, f_lower));
                    if let Some(cell) = buf.cell_mut((area.x + x as u16, area.y + y as u16)) {
                        cell.set_char('▀').set_fg(fu).set_bg(fl);
                    }
                }
            }
        }
    }
}
