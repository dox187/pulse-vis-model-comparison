//! Rendering. Each mode is a function that paints directly into the frame buffer.

pub mod bars;
pub mod lissajous;
pub mod mirror;
pub mod overlay;
pub mod radial;
pub mod spectrogram;
pub mod vu;
pub mod wave;

use crate::app::{App, Popup};
use crate::audio::Capture;
use crate::config::{MarkerKind, Mode};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::symbols::Marker;

/// Lower block elements in eighths, index 0 = empty, 8 = full.
pub const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Left block elements in eighths.
pub const LEFT_EIGHTHS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    buf.set_style(area, Style::default().bg(app.theme.bg()).fg(app.theme.text()));

    let (main, status) = if app.cfg.display.status_bar && area.height >= 4 {
        (
            Rect { height: area.height - 1, ..area },
            Some(Rect { y: area.y + area.height - 1, height: 1, ..area }),
        )
    } else {
        (area, None)
    };
    app.main_area = main;

    if main.width < 10 || main.height < 3 {
        overlay::centered_message(buf, main, "terminal too small", &app.theme);
    } else {
        match app.cfg.display.mode {
            Mode::Bars => bars::render(app, main, buf),
            Mode::Mirror => mirror::render(app, main, buf),
            Mode::Wave => wave::render(app, main, buf),
            Mode::Vu => vu::render(app, main, buf),
            Mode::Spectrogram => spectrogram::render(app, main, buf),
            Mode::Lissajous => lissajous::render(app, main, buf),
            Mode::Radial => radial::render(app, main, buf),
        }
        if !matches!(app.status.capture, Capture::Streaming(_)) || app.status.frames == 0 {
            overlay::capture_notice(app, main, buf);
        }
    }

    if let Some(s) = status {
        overlay::status_bar(app, s, buf);
    }
    match app.popup {
        Popup::Help => overlay::help(app, area, buf),
        Popup::Sources { .. } => overlay::sources(app, area, buf),
        Popup::None => {}
    }
    overlay::flash(app, area, buf);
}

pub fn marker(kind: MarkerKind) -> Marker {
    match kind {
        MarkerKind::Braille => Marker::Braille,
        MarkerKind::Octant => Marker::Octant,
        MarkerKind::HalfBlock => Marker::HalfBlock,
        MarkerKind::Block => Marker::Block,
        MarkerKind::Dot => Marker::Dot,
    }
}

/// Cuts `s` to at most `max` characters, marking the cut with an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Like [`truncate`] but keeps the end of the string (useful for paths).
pub fn truncate_left(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::from("…");
    out.extend(s.chars().skip(n - (max - 1)));
    out
}

/// A `w` x `h` rectangle centred in `area`, clamped to it.
pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}
