//! Status bar, popups and transient messages.

use super::{centered, truncate, truncate_left};
use crate::app::{App, Popup, SourceEntry};
use crate::audio::{Capture, Connection};
use crate::config::Channels;
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Widget};

const WARN: Color = Color::Rgb(255, 200, 60);
const ERR: Color = Color::Rgb(255, 80, 80);

pub fn status_bar(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let cfg = &app.cfg;
    let base = Style::default().fg(theme.text());
    let dim = Style::default().fg(theme.dim_text());
    let badge = Style::default().fg(theme.bg_rgb().to_color(theme.truecolor)).bg(theme.accent()).add_modifier(Modifier::BOLD);

    let (src_text, src_style) = match (&app.status.connection, &app.status.capture) {
        (Connection::Failed(e), _) => (format!("audio server: {e}"), Style::default().fg(ERR)),
        (Connection::Connecting, _) => ("connecting to audio server…".to_string(), Style::default().fg(WARN)),
        (_, Capture::Streaming(l)) => (l.clone(), base),
        (_, Capture::Connecting(l)) => (format!("{l} (connecting)"), Style::default().fg(WARN)),
        (_, Capture::Waiting(m)) => (m.clone(), Style::default().fg(WARN)),
        (_, Capture::Error(e)) => (e.clone(), Style::default().fg(ERR)),
        (_, Capture::Idle) => ("no capture".to_string(), dim),
    };

    let mut gain = format!("gain {:+.1} dB", cfg.sensitivity.gain_db);
    if cfg.sensitivity.auto_gain {
        gain.push_str(&format!(" · auto {:+.0}", app.analyzer.auto_offset_db));
    }
    let mut spans = vec![
        Span::styled(format!(" {} ", cfg.display.mode.title().to_uppercase()), badge),
        Span::styled(" ▶ ", dim),
        Span::styled(truncate(&src_text, (area.width as usize / 2).clamp(16, 60)), src_style),
        Span::styled("  │  ", dim),
        Span::styled(gain, base),
        Span::styled("  │  ", dim),
        Span::styled(format!("fft {}", cfg.analysis.fft_size), base),
        Span::styled("  │  ", dim),
        Span::styled(
            if cfg.display.channels == Channels::Stereo { "stereo" } else { "mono" },
            base,
        ),
        Span::styled("  │  ", dim),
        Span::styled(format!("{:>3.0} fps", app.fps), base),
    ];
    if app.paused {
        spans.push(Span::styled("  ‖ PAUSED", Style::default().fg(WARN).add_modifier(Modifier::BOLD)));
    }
    let left = Line::from(spans);
    let right = Line::from(vec![
        Span::styled("s", Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD)),
        Span::styled(" source  ", dim),
        Span::styled("?", Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD)),
        Span::styled(" help  ", dim),
        Span::styled("q", Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD)),
        Span::styled(" quit ", dim),
    ]);
    let rw = right.width() as u16;
    let lw = area.width.saturating_sub(rw + 1);
    buf.set_line(area.x, area.y, &left, lw);
    if area.width > rw {
        buf.set_line(area.x + area.width - rw, area.y, &right, rw);
    }
}

/// Message in the middle of the visual area while nothing is being captured.
pub fn capture_notice(app: &App, area: Rect, buf: &mut Buffer) {
    let (text, color) = match (&app.status.connection, &app.status.capture) {
        (Connection::Failed(e), _) => (format!("Cannot reach the audio server: {e}"), ERR),
        (Connection::Connecting, _) => ("Connecting to the audio server…".to_string(), WARN),
        (_, Capture::Waiting(m)) => (m.clone(), WARN),
        (_, Capture::Error(e)) => (e.clone(), ERR),
        (_, Capture::Connecting(l)) => (format!("Opening {l}…"), WARN),
        (_, Capture::Streaming(_)) => ("Waiting for audio…".to_string(), app.theme.dim_text()),
        (_, Capture::Idle) => return,
    };
    let w = (text.chars().count() as u16 + 4).min(area.width);
    let r = centered(area, w, 3);
    Clear.render(r, buf);
    buf.set_style(r, Style::default().bg(app.theme.bg()));
    let y = r.y + 1;
    let x = r.x + (r.width.saturating_sub(text.chars().count() as u16)) / 2;
    buf.set_string(x, y, text, Style::default().fg(color));
}

pub fn centered_message(buf: &mut Buffer, area: Rect, text: &str, theme: &Theme) {
    let w = (text.chars().count() as u16).min(area.width);
    let r = centered(area, w, 1);
    buf.set_string(r.x, r.y, text, Style::default().fg(theme.dim_text()));
}

const HELP: &[(&str, &str)] = &[
    ("q  Esc", "quit  (Esc closes a popup first)"),
    ("Tab  Shift-Tab  m  M", "next / previous mode"),
    ("1 … 7", "bars · mirror · wave · vu · spectrogram · vectorscope · radial"),
    ("s", "choose the audio source (application, output, all)"),
    ("r", "refresh the source list"),
    ("+  -", "gain +2 dB / −2 dB"),
    ("a", "toggle automatic gain"),
    ("c  C", "next / previous colour theme"),
    ("d", "cycle gradient direction (vertical, horizontal, level, solid)"),
    ("t", "toggle stereo / mono"),
    ("p", "toggle peak markers"),
    ("[  ]", "thinner / wider bars"),
    ("{  }", "smaller / larger gap between bars"),
    (",  .", "faster / slower decay"),
    ("<  >", "smaller / larger FFT (time vs frequency resolution)"),
    ("l", "cycle bar scale (log, sqrt, linear)"),
    ("v", "cycle oscilloscope style (line, filled, dots)"),
    ("o", "cycle spectrogram direction (up, down, left)"),
    ("x", "toggle vectorscope rotation"),
    ("b", "toggle the status bar"),
    ("Space", "pause / resume"),
    ("w", "save the current settings to the config file"),
    ("?  h  F1", "this help"),
];

pub fn help(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let key_w = HELP.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(8) as u16;
    let text_w = HELP.iter().map(|(_, t)| t.chars().count()).max().unwrap_or(20) as u16;
    let w = (key_w + text_w + 7).min(area.width);
    let h = (HELP.len() as u16 + 3).min(area.height);
    let r = centered(area, w, h);
    Clear.render(r, buf);
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(" pulse-vis-claude ", Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD)),
            Span::styled("keys ", Style::default().fg(theme.text())),
        ]))
        .border_style(Style::default().fg(theme.accent()))
        .style(Style::default().bg(theme.bg()).fg(theme.text()));
    let inner = block.inner(r);
    block.render(r, buf);
    for (i, (k, t)) in HELP.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let kx = inner.x + 1;
        let tx = inner.x + key_w + 4;
        buf.set_string(kx, y, truncate(k, inner.width.saturating_sub(1) as usize), Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD));
        if tx < inner.x + inner.width {
            buf.set_string(tx, y, truncate(t, (inner.x + inner.width - tx) as usize), Style::default().fg(theme.text()));
        }
    }
    let footer = truncate_left(
        &format!("config: {}", app.config_path.display()),
        inner.width.saturating_sub(2) as usize,
    );
    let fy = inner.y + inner.height.saturating_sub(1);
    let fw = footer.chars().count() as u16;
    if fw <= inner.width && inner.height > HELP.len() as u16 {
        buf.set_string(inner.x + (inner.width - fw) / 2, fy, footer, Style::default().fg(theme.dim_text()));
    }
}

pub fn sources(app: &App, area: Rect, buf: &mut Buffer) {
    let theme = &app.theme;
    let Popup::Sources { selected } = app.popup else { return };
    let entries = &app.source_entries;
    let labels: Vec<String> = entries.iter().map(|e| e.label(&app.status)).collect();
    let max_label = labels.iter().map(|l| l.chars().count()).max().unwrap_or(20) as u16;
    let w = (max_label + 8).min(area.width.saturating_sub(4)).max(40.min(area.width));
    let h = (labels.len() as u16 + 5).clamp(6, area.height);
    let r = centered(area, w, h);
    Clear.render(r, buf);
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(" Audio source ", Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD)),
        ]))
        .border_style(Style::default().fg(theme.accent()))
        .style(Style::default().bg(theme.bg()).fg(theme.text()));
    let inner = block.inner(r);
    block.render(r, buf);

    let list_h = inner.height.saturating_sub(2) as usize;
    let first = if selected >= list_h { selected + 1 - list_h } else { 0 };
    for (row, i) in (first..labels.len()).take(list_h).enumerate() {
        let y = inner.y + row as u16;
        let entry = &entries[i];
        let current = entry.is_current(&app.status.target);
        let is_sel = i == selected;
        let mut style = Style::default().fg(theme.text());
        if is_sel {
            style = Style::default().fg(theme.bg_rgb().to_color(theme.truecolor)).bg(theme.accent()).add_modifier(Modifier::BOLD);
            buf.set_style(Rect { x: inner.x, y, width: inner.width, height: 1 }, style);
        }
        let mark = if current { "●" } else { " " };
        let paused = matches!(entry, SourceEntry::App(a) if a.corked);
        let reserve = if paused { 10 } else { 1 };
        let label = truncate(&labels[i], (inner.width as usize).saturating_sub(3 + reserve));
        let line = format!(" {mark} {label}");
        buf.set_string(inner.x, y, &line, style);
        if paused {
            let lw = line.chars().count() as u16;
            let st = if is_sel { style } else { Style::default().fg(theme.dim_text()) };
            buf.set_string(inner.x + lw + 1, y, "(paused)", st);
        }
    }
    if app.status.apps.is_empty() {
        let msg = if app.status.apps_loaded { "no application is playing audio" } else { "loading…" };
        let y = inner.y + (labels.len() as u16).min(inner.height.saturating_sub(3));
        buf.set_string(inner.x + 3, y, msg, Style::default().fg(theme.dim_text()));
    }
    let footer = "↑↓ move · Enter select · r refresh · Esc close";
    let fw = footer.chars().count() as u16;
    let fy = inner.y + inner.height.saturating_sub(1);
    if fw <= inner.width {
        buf.set_string(inner.x + (inner.width - fw) / 2, fy, footer, Style::default().fg(theme.dim_text()));
    }
}

pub fn flash(app: &App, area: Rect, buf: &mut Buffer) {
    let Some((msg, at)) = &app.flash else { return };
    if at.elapsed().as_secs_f32() > 1.8 {
        return;
    }
    let theme = &app.theme;
    let w = (msg.chars().count() as u16 + 2).min(area.width);
    let r = Rect { x: area.x + area.width - w, y: area.y, width: w, height: 1 };
    let style = Style::default().fg(theme.bg_rgb().to_color(theme.truecolor)).bg(theme.accent()).add_modifier(Modifier::BOLD);
    buf.set_string(r.x, r.y, format!(" {msg} "), style);
}
