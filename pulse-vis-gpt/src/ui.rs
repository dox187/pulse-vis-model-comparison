use crate::{
    App,
    audio::Sample,
    config::{Mode, parse_color},
    dsp::db,
};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Widget, Wrap,
        canvas::{Canvas, Line as CanvasLine, Points},
    },
};
use std::f64::consts::TAU;

pub fn color(s: &str) -> Color {
    let (r, g, b) = parse_color(s).unwrap_or((255, 255, 255));
    Color::Rgb(r, g, b)
}
pub fn palette(app: &App) -> Vec<Color> {
    let colors: Vec<&str> = match app.config.theme.as_str() {
        "fire" => vec!["#FFCC66", "#FF7755", "#E64B87"],
        "ocean" => vec!["#55E8EA", "#4299FF", "#6868D9"],
        "mono" => vec!["#D8E2F0", "#FFFFFF"],
        "custom" => app.config.gradient.iter().map(String::as_str).collect(),
        _ => vec!["#45E0B8", "#55A7FF", "#C58CFF"],
    };
    colors.into_iter().map(color).collect()
}
fn gradient(colors: &[Color], value: f32) -> Color {
    let t = value.clamp(0.0, 1.0) * (colors.len() - 1) as f32;
    let i = (t.floor() as usize).min(colors.len() - 2);
    let f = t - i as f32;
    match (colors[i], colors[i + 1]) {
        (Color::Rgb(r, g, b), Color::Rgb(r2, g2, b2)) => Color::Rgb(
            (r as f32 + (r2 as f32 - r as f32) * f) as u8,
            (g as f32 + (g2 as f32 - g as f32) * f) as u8,
            (b as f32 + (b2 as f32 - b as f32) * f) as u8,
        ),
        _ => colors[i],
    }
}
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let bg = color(&app.config.background);
    let fg = color(&app.config.foreground);
    let colors = palette(app);
    let accent = colors[0];
    let muted = Color::Rgb(115, 134, 157);
    frame.render_widget(Block::new().style(Style::default().bg(bg).fg(fg)), area);
    if area.width < 40 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("pulse-vis\nEnlarge terminal to at least 40 × 12.\nq quit · ? help")
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .split(area);
    let header = Line::from(vec![
        Span::styled(
            " ◉ PULSE ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" /  sound in motion", Style::default().fg(muted)),
        Span::styled(
            if app.paused {
                "     ⏸ PAUSED"
            } else if app.demo {
                "     ● DEMO"
            } else {
                "     ● LIVE"
            },
            Style::default().fg(accent),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(header).block(
            Block::new()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::Rgb(34, 48, 67))),
        ),
        rows[0],
    );
    let tabs = Line::from(
        Mode::ALL
            .iter()
            .enumerate()
            .flat_map(|(i, m)| {
                [
                    Span::styled(
                        format!(" {} {} ", i + 1, m.name()),
                        if app.config.mode == *m {
                            Style::default()
                                .fg(bg)
                                .bg(accent)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(muted)
                        },
                    ),
                    Span::raw(" "),
                ]
            })
            .collect::<Vec<_>>(),
    );
    frame.render_widget(Paragraph::new(tabs), rows[1]);
    let title = format!(" {}  ·  {} ", app.config.mode.name(), app.source_label());
    let panel = Block::bordered()
        .title(title)
        .border_style(Style::default().fg(Color::Rgb(42, 59, 79)));
    let inner = panel.inner(rows[2]);
    frame.render_widget(panel, rows[2]);
    match app.config.mode {
        Mode::Spectrum => {
            let areas = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
            frame.render_widget(
                Bars {
                    values: &app.analysis.bands,
                    peaks: &app.analysis.peaks,
                    colors: &colors,
                    width: app.config.bar_width,
                    gap: app.config.bar_gap,
                    show_peaks: app.config.show_peaks,
                },
                areas[0],
            );
            frame.render_widget(
                Paragraph::new(format!(
                    " {:.0} Hz   ·   logarithmic frequency →   ·   {:.1} kHz",
                    app.config.min_frequency,
                    app.config.max_frequency / 1000.0
                ))
                .style(Style::default().fg(muted)),
                areas[1],
            );
        }
        Mode::Spectrogram => {
            frame.render_widget(
                Heatmap {
                    history: &app.history,
                    colors: &colors,
                },
                inner,
            );
        }
        Mode::Waveform => waveform(
            frame,
            inner,
            &app.samples,
            &colors,
            app.config.sensitivity * app.analysis.gain,
        ),
        Mode::Vectorscope => {
            let gain = (app.config.sensitivity * app.analysis.gain) as f64;
            let points: Vec<_> = app
                .samples
                .iter()
                .step_by(3)
                .map(|s| {
                    (
                        ((s[0] - s[1]) as f64 * gain * 0.7).clamp(-1.0, 1.0),
                        ((s[0] + s[1]) as f64 * gain * 0.7).clamp(-1.0, 1.0),
                    )
                })
                .collect();
            frame.render_widget(
                Canvas::default()
                    .background_color(bg)
                    .x_bounds([-1.1, 1.1])
                    .y_bounds([-1.1, 1.1])
                    .paint(|ctx| {
                        ctx.draw(&CanvasLine {
                            x1: -1.0,
                            y1: 0.0,
                            x2: 1.0,
                            y2: 0.0,
                            color: Color::DarkGray,
                        });
                        ctx.draw(&CanvasLine {
                            x1: 0.0,
                            y1: -1.0,
                            x2: 0.0,
                            y2: 1.0,
                            color: Color::DarkGray,
                        });
                        ctx.draw(&Points {
                            coords: &points,
                            color: accent,
                        });
                        ctx.print(
                            -1.0,
                            1.0,
                            Span::styled(
                                format!("L/R · correlation {:+.2}", app.analysis.correlation),
                                Style::default().fg(muted),
                            ),
                        );
                    }),
                inner,
            );
        }
        Mode::Vu => {
            let rows = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .split(inner);
            for ch in 0..2 {
                let level = db(app.analysis.rms[ch]);
                let peak = db(app.analysis.peak[ch]);
                let label = format!(
                    "{}   RMS {level:6.1} dBFS   PEAK {peak:6.1} dBFS{}",
                    if ch == 0 { "LEFT " } else { "RIGHT" },
                    if peak >= 0.0 { "  CLIP" } else { "" }
                );
                frame.render_widget(
                    Gauge::default()
                        .block(Block::bordered().title(label))
                        .gauge_style(
                            Style::default()
                                .fg(if peak >= 0.0 { Color::Red } else { colors[ch] })
                                .bg(bg),
                        )
                        .ratio(((level + 60.0) / 60.0).clamp(0.0, 1.0) as f64)
                        .label(""),
                    rows[1 + ch * 2],
                );
            }
            frame.render_widget(
                Paragraph::new(" −60         −40         −20          0 dBFS  ·  raw input levels")
                    .style(Style::default().fg(muted)),
                rows[4],
            );
        }
        Mode::Radial => {
            frame.render_widget(
                Canvas::default()
                    .background_color(bg)
                    .x_bounds([-1.2, 1.2])
                    .y_bounds([-1.2, 1.2])
                    .paint(|ctx| {
                        let count = app.analysis.bands.len();
                        for (i, v) in app.analysis.bands.iter().enumerate() {
                            let angle = i as f64 / count as f64 * TAU;
                            let radius = 0.25 + *v as f64 * 0.75;
                            ctx.draw(&CanvasLine {
                                x1: angle.sin() * 0.25,
                                y1: angle.cos() * 0.25,
                                x2: angle.sin() * radius,
                                y2: angle.cos() * radius,
                                color: gradient(&colors, i as f32 / count as f32),
                            });
                        }
                    }),
                inner,
            );
        }
    }
    let detail = format!(
        " {:?} / {} FFT   {} fps   gain {:.2}×   smooth {:.0}%   AGC {}   {}",
        app.config.quality,
        app.config.quality.size(),
        app.config.fps,
        app.config.sensitivity,
        app.config.smoothing * 100.0,
        if app.config.auto_gain { "on" } else { "off" },
        app.config.theme
    );
    frame.render_widget(
        Paragraph::new(detail).style(Style::default().fg(muted)),
        rows[3],
    );
    let status = app.status_text();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" a", Style::default().fg(accent)),
                Span::raw(" sources  "),
                Span::styled("s", Style::default().fg(accent)),
                Span::raw(" settings  "),
                Span::styled("+ / −", Style::default().fg(accent)),
                Span::raw(" gain  "),
                Span::styled("?", Style::default().fg(accent)),
                Span::raw(" help  "),
                Span::styled("q", Style::default().fg(accent)),
                Span::raw(" quit"),
            ]),
            Line::styled(status, Style::default().fg(muted)),
        ]),
        rows[4],
    );
    if app.help {
        popup(
            frame,
            " Keyboard shortcuts ",
            vec![
                "1–6 / Tab     Select visualization / next mode",
                "a             Choose outputs or applications",
                "s             Live settings panel",
                "+ / −         Increase / decrease sensitivity",
                "[ / ]         Less / more smoothing",
                "g             Toggle automatic gain",
                "p             Cycle color theme",
                "Q             Cycle FFT quality",
                "Space         Pause visualization",
                "w             Save settings and source selection",
                "r             Reload configuration",
                "? / Esc       Close overlay",
                "q / Ctrl+C    Quit",
                "",
                "Audio capture is read-only; playback routing is unchanged.",
                "All outputs combines every sink; a virtual mirrored output",
                "can therefore count the same signal twice.",
            ],
            accent,
            bg,
        );
    }
    if app.sources {
        sources(frame, app, accent, bg);
    }
    if app.settings {
        settings(frame, app, accent, bg);
    }
}
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
fn popup(frame: &mut Frame, title: &str, lines: Vec<&str>, accent: Color, bg: Color) {
    let rect = centered(frame.area(), 66, lines.len() as u16 + 4);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .title(title)
                    .border_style(Style::default().fg(accent)),
            )
            .style(Style::default().bg(bg)),
        rect,
    );
}
fn sources(frame: &mut Frame, app: &App, accent: Color, bg: Color) {
    let rect = centered(frame.area(), 76, 22);
    frame.render_widget(Clear, rect);
    let panel = Block::bordered()
        .title(" Sources · ↑↓ navigate · Space select · Esc close ")
        .style(Style::default().bg(bg))
        .border_style(Style::default().fg(accent));
    let inner = panel.inner(rect);
    frame.render_widget(panel, rect);
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(inner);
    let items: Vec<_> = app
        .source_options()
        .iter()
        .map(|option| {
            ListItem::new(format!(
                " {}  {}",
                if option.selected { "●" } else { "○" },
                option.label
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.source_cursor));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
            .highlight_symbol("› "),
        rows[0],
        &mut state,
    );
    frame.render_widget(Paragraph::new("Applications can be combined. Active streams refresh every 2s.\nFilters follow new streams and output-device changes.\nEnter / Space apply immediately · w saves selection").style(Style::default().fg(Color::Gray)),rows[1]);
}
fn settings(frame: &mut Frame, app: &App, accent: Color, bg: Color) {
    let c = &app.config;
    let rows = vec![
        format!("Sensitivity       {:.2}×", c.sensitivity),
        format!("Smoothing         {:.0}%", c.smoothing * 100.0),
        format!(
            "Quality           {:?} / {} FFT",
            c.quality,
            c.quality.size()
        ),
        format!("Frame rate        {} fps", c.fps),
        format!("Automatic gain    {}", c.auto_gain),
        format!("Color theme       {}", c.theme),
        format!("Bar width         {}", c.bar_width),
        format!("Bar gap           {}", c.bar_gap),
        format!("Peak markers      {}", c.show_peaks),
    ];
    let rect = centered(frame.area(), 58, 16);
    frame.render_widget(Clear, rect);
    let panel = Block::bordered()
        .title(" Settings · ↑↓ choose · ←→ adjust ")
        .style(Style::default().bg(bg))
        .border_style(Style::default().fg(accent));
    let inner = panel.inner(rect);
    frame.render_widget(panel, rect);
    let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
    let mut state = ListState::default().with_selected(Some(app.settings_cursor));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("› ")
            .highlight_style(Style::default().fg(accent)),
        layout[0],
        &mut state,
    );
    frame.render_widget(
        Paragraph::new(
            "w save · r reload · Esc close\nCustom RGB colors and frequency limits: config.toml",
        ),
        layout[1],
    );
}
fn waveform(frame: &mut Frame, area: Rect, samples: &[Sample], colors: &[Color], gain: f32) {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    // Trigger on a rising zero crossing to keep periodic signals steady.
    let start = samples
        .windows(2)
        .take(samples.len() / 2)
        .position(|p| p[0][0] <= 0.0 && p[1][0] > 0.0)
        .unwrap_or(0);
    let window = &samples[start..(start + 1024).min(samples.len())];
    for ch in 0..2 {
        frame.render_widget(
            Canvas::default()
                .block(Block::new().title(if ch == 0 {
                    " L · waveform"
                } else {
                    " R · waveform"
                }))
                .background_color(Color::Reset)
                .x_bounds([0.0, window.len().max(1) as f64])
                .y_bounds([-1.0, 1.0])
                .paint(|ctx| {
                    ctx.draw(&CanvasLine {
                        x1: 0.0,
                        y1: 0.0,
                        x2: window.len() as f64,
                        y2: 0.0,
                        color: Color::DarkGray,
                    });
                    for (i, p) in window.windows(2).enumerate() {
                        ctx.draw(&CanvasLine {
                            x1: i as f64,
                            y1: (p[0][ch] * gain).clamp(-1.0, 1.0) as f64,
                            x2: (i + 1) as f64,
                            y2: (p[1][ch] * gain).clamp(-1.0, 1.0) as f64,
                            color: colors[ch],
                        });
                    }
                }),
            panels[ch],
        );
    }
}
struct Bars<'a> {
    values: &'a [f32],
    peaks: &'a [f32],
    colors: &'a [Color],
    width: u16,
    gap: u16,
    show_peaks: bool,
}
impl Widget for Bars<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let step = self.width + self.gap;
        let count = (area.width / step).max(1) as usize;
        let total = (count as u16 * step)
            .saturating_sub(self.gap)
            .min(area.width);
        let offset = (area.width - total) / 2;
        let glyphs = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        for i in 0..count.min(self.values.len()) {
            let level = self.values[i] * area.height as f32;
            for y in 0..area.height {
                let fill = (level - y as f32).clamp(0.0, 1.0);
                if fill <= 0.0 {
                    continue;
                }
                let c = gradient(self.colors, y as f32 / area.height.max(1) as f32);
                for x in 0..self.width {
                    let px = area.x + offset + i as u16 * step + x;
                    if px < area.right() {
                        buf[(px, area.bottom() - 1 - y)]
                            .set_symbol(glyphs[(fill * 8.0).ceil() as usize])
                            .set_fg(c);
                    }
                }
            }
            if self.show_peaks && self.peaks[i] > 0.01 {
                let y = (self.peaks[i] * area.height as f32).ceil() as u16;
                let py = area.bottom() - y.clamp(1, area.height);
                for x in 0..self.width {
                    let px = area.x + offset + i as u16 * step + x;
                    if px < area.right() {
                        buf[(px, py)]
                            .set_symbol("▔")
                            .set_fg(gradient(self.colors, 1.0));
                    }
                }
            }
        }
    }
}
struct Heatmap<'a> {
    history: &'a std::collections::VecDeque<Vec<f32>>,
    colors: &'a [Color],
}
impl Widget for Heatmap<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (y, row) in self
            .history
            .iter()
            .rev()
            .take(area.height as usize)
            .enumerate()
        {
            for x in 0..area.width {
                if row.is_empty() {
                    continue;
                }
                let v = row[x as usize * row.len() / area.width as usize];
                if v > 0.02 {
                    buf[(area.x + x, area.bottom() - 1 - y as u16)]
                        .set_symbol(if v < 0.2 {
                            "░"
                        } else if v < 0.45 {
                            "▒"
                        } else if v < 0.7 {
                            "▓"
                        } else {
                            "█"
                        })
                        .set_fg(gradient(self.colors, v));
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_every_mode_and_overlays_at_small_sizes() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new(
            crate::config::Config::default(),
            std::path::PathBuf::from("unused"),
            true,
        );
        app.samples = crate::audio::demo(4096, 0.1);
        app.analysis.update(&app.samples, 32, &app.config, 0.1);
        for (w, h) in [(1, 1), (40, 12), (80, 24), (160, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for mode in Mode::ALL {
                app.config.mode = mode;
                terminal.draw(|f| draw(f, &app)).unwrap();
            }
            app.sources = true;
            terminal.draw(|f| draw(f, &app)).unwrap();
            app.sources = false;
            app.settings = true;
            terminal.draw(|f| draw(f, &app)).unwrap();
            app.settings = false;
            app.help = true;
            terminal.draw(|f| draw(f, &app)).unwrap();
            app.help = false;
        }
    }

    #[test]
    fn spectrum_layout_has_signal_and_controls() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new(crate::config::Config::default(), "unused".into(), true);
        app.tick(120, 0.1, 0.1);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("PULSE"));
        assert!(screen.contains("Stereo demo"));
        assert!(screen.contains("sources"));
        assert!(screen.contains('█'));
        if std::env::var_os("PULSE_VIS_PRINT_FRAME").is_some() {
            for row in terminal.backend().buffer().content.chunks(120) {
                println!(
                    "{}",
                    row.iter().map(|cell| cell.symbol()).collect::<String>()
                );
            }
        }
    }
}
