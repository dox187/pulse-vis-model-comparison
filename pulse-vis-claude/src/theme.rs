//! Colour handling: hex parsing, gradients, presets and terminal colour mapping.

use crate::config::{ColorConfig, GradientDirection};
use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        let (r, g, b) = match s.len() {
            6 => (
                u8::from_str_radix(&s[0..2], 16).ok()?,
                u8::from_str_radix(&s[2..4], 16).ok()?,
                u8::from_str_radix(&s[4..6], 16).ok()?,
            ),
            3 => {
                let d = |i: usize| u8::from_str_radix(&s[i..i + 1], 16).ok().map(|v| v * 17);
                (d(0)?, d(1)?, d(2)?)
            }
            _ => return None,
        };
        Some(Rgb(r, g, b))
    }

    pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
    }

    pub fn to_color(self, truecolor: bool) -> Color {
        if truecolor {
            return Color::Rgb(self.0, self.1, self.2);
        }
        let (r, g, b) = (self.0 as i32, self.1 as i32, self.2 as i32);
        if (r - g).abs() < 10 && (g - b).abs() < 10 && (r - b).abs() < 10 {
            let avg = (r + g + b) / 3;
            return if avg < 8 {
                Color::Indexed(16)
            } else if avg > 238 {
                Color::Indexed(231)
            } else {
                Color::Indexed(232 + ((avg - 8) * 23 / 230) as u8)
            };
        }
        let q = |v: i32| ((v as f32 / 255.0) * 5.0).round() as u8;
        Color::Indexed(16 + 36 * q(r) + 6 * q(g) + q(b))
    }
}

#[derive(Debug, Clone)]
pub struct Gradient {
    stops: Vec<Rgb>,
}

impl Gradient {
    pub fn new(mut stops: Vec<Rgb>) -> Gradient {
        if stops.is_empty() {
            stops.push(Rgb(255, 255, 255));
        }
        Gradient { stops }
    }

    pub fn at(&self, t: f32) -> Rgb {
        let n = self.stops.len();
        if n == 1 {
            return self.stops[0];
        }
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        let pos = t * (n - 1) as f32;
        let i = (pos.floor() as usize).min(n - 2);
        Rgb::lerp(self.stops[i], self.stops[i + 1], pos - i as f32)
    }
}

/// Built-in gradient presets, listed low -> high.
pub const PRESETS: &[(&str, &[&str])] = &[
    ("spectrum", &["#00e5ff", "#7c4dff", "#ff4081"]),
    ("classic", &["#00e676", "#c6ff00", "#ffea00", "#ff9100", "#ff1744"]),
    ("rainbow", &["#ff1744", "#ff9100", "#ffea00", "#00e676", "#2979ff", "#d500f9"]),
    ("sunset", &["#ffd166", "#ff7b54", "#ff3f7a", "#9d4edd"]),
    ("ocean", &["#a0f0ff", "#00c9ff", "#0072ff", "#4b1fa8"]),
    ("fire", &["#4a0000", "#ff2a00", "#ff9a00", "#ffe97a", "#ffffff"]),
    ("ice", &["#0b1e3a", "#1f6fb2", "#62c6ff", "#dff6ff"]),
    ("matrix", &["#003b00", "#00b300", "#3fff3f", "#c8ffc8"]),
    ("neon", &["#ff00ff", "#00ffff"]),
    ("synthwave", &["#2b1055", "#7303c0", "#ec38bc", "#fdeff9"]),
    ("pastel", &["#a8e6cf", "#dcedc1", "#ffd3b6", "#ffaaa5"]),
    ("mono", &["#6e6e6e", "#ffffff"]),
    ("gruvbox", &["#b8bb26", "#fabd2f", "#fe8019", "#fb4934"]),
    ("nord", &["#88c0d0", "#81a1c1", "#5e81ac", "#b48ead"]),
    ("dracula", &["#50fa7b", "#f1fa8c", "#ffb86c", "#ff79c6", "#bd93f9"]),
    ("catppuccin", &["#89dceb", "#89b4fa", "#cba6f7", "#f5c2e7"]),
];

pub fn preset(name: &str) -> Option<&'static [&'static str]> {
    PRESETS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, g)| *g)
}

/// Name of the preset `steps` positions after `current` (negative steps go backwards).
/// An unknown or "custom" theme starts from the first preset.
pub fn cycle_preset(current: &str, steps: i32) -> &'static str {
    let n = PRESETS.len() as i32;
    let Some(idx) = PRESETS
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case(current))
    else {
        // Unknown or "custom": start from the first preset going forwards, the last going backwards.
        return if steps >= 0 { PRESETS[0].0 } else { PRESETS[PRESETS.len() - 1].0 };
    };
    let next = ((idx as i32 + steps) % n + n) % n;
    PRESETS[next as usize].0
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub gradient: Gradient,
    pub direction: GradientDirection,
    pub background: Option<Rgb>,
    pub peak: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    pub truecolor: bool,
}

impl Theme {
    pub fn from_config(c: &ColorConfig, truecolor: bool) -> Theme {
        let stops: Vec<Rgb> = if c.theme.eq_ignore_ascii_case("custom") {
            c.gradient.iter().filter_map(|s| Rgb::parse(s)).collect()
        } else {
            preset(&c.theme)
                .or_else(|| preset("spectrum"))
                .unwrap()
                .iter()
                .filter_map(|s| Rgb::parse(s))
                .collect()
        };
        let background = if c.background.eq_ignore_ascii_case("default") || c.background.is_empty() {
            None
        } else {
            Rgb::parse(&c.background)
        };
        Theme {
            gradient: Gradient::new(stops),
            direction: c.direction,
            background,
            peak: Rgb::parse(&c.peak).unwrap_or(Rgb(255, 255, 255)),
            text: Rgb::parse(&c.text).unwrap_or(Rgb(200, 200, 200)),
            accent: Rgb::parse(&c.accent).unwrap_or(Rgb(124, 77, 255)),
            truecolor,
        }
    }

    pub fn color(&self, t: f32) -> Color {
        self.gradient.at(t).to_color(self.truecolor)
    }

    pub fn rgb(&self, t: f32) -> Rgb {
        self.gradient.at(t)
    }

    /// Terminal colour for the background (`Reset` when the terminal default is kept).
    pub fn bg(&self) -> Color {
        self.background
            .map(|c| c.to_color(self.truecolor))
            .unwrap_or(Color::Reset)
    }

    /// Concrete RGB to blend towards when fading; black when the terminal default is used.
    pub fn bg_rgb(&self) -> Rgb {
        self.background.unwrap_or(Rgb(0, 0, 0))
    }

    /// Colour of a bar cell given its frequency position, height position and level, all 0..1.
    pub fn bar_color(&self, freq_t: f32, row_t: f32, level: f32) -> Color {
        let t = match self.direction {
            GradientDirection::Vertical => row_t,
            GradientDirection::Horizontal => freq_t,
            GradientDirection::Level => level,
            GradientDirection::Solid => 0.5,
        };
        self.color(t)
    }

    /// Gradient colour at `t`, blended towards the background by `1 - strength`.
    pub fn faded(&self, t: f32, strength: f32) -> Color {
        Rgb::lerp(self.bg_rgb(), self.gradient.at(t), strength).to_color(self.truecolor)
    }

    pub fn peak(&self) -> Color {
        self.peak.to_color(self.truecolor)
    }

    pub fn text(&self) -> Color {
        self.text.to_color(self.truecolor)
    }

    pub fn dim_text(&self) -> Color {
        Rgb::lerp(self.bg_rgb(), self.text, 0.55).to_color(self.truecolor)
    }

    pub fn accent(&self) -> Color {
        self.accent.to_color(self.truecolor)
    }

    pub fn convert(&self, c: Rgb) -> Color {
        c.to_color(self.truecolor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex() {
        assert_eq!(Rgb::parse("#ff8000"), Some(Rgb(255, 128, 0)));
        assert_eq!(Rgb::parse("0F0"), Some(Rgb(0, 255, 0)));
        assert_eq!(Rgb::parse("nope"), None);
    }

    #[test]
    fn gradient_endpoints() {
        let g = Gradient::new(vec![Rgb(0, 0, 0), Rgb(255, 255, 255)]);
        assert_eq!(g.at(0.0), Rgb(0, 0, 0));
        assert_eq!(g.at(1.0), Rgb(255, 255, 255));
        assert_eq!(g.at(0.5), Rgb(128, 128, 128));
        assert_eq!(g.at(f32::NAN), Rgb(0, 0, 0));
    }

    #[test]
    fn presets_parse() {
        for (name, stops) in PRESETS {
            for s in stops.iter() {
                assert!(Rgb::parse(s).is_some(), "bad colour {s} in {name}");
            }
        }
    }

    #[test]
    fn cycling() {
        assert_eq!(cycle_preset("spectrum", 1), "classic");
        assert_eq!(cycle_preset("spectrum", -1), PRESETS.last().unwrap().0);
        assert_eq!(cycle_preset("custom", 1), "spectrum");
        assert_eq!(cycle_preset("custom", -1), PRESETS.last().unwrap().0);
    }

    #[test]
    fn indexed_fallback() {
        assert_eq!(Rgb(255, 0, 0).to_color(false), Color::Indexed(196));
        assert_eq!(Rgb(0, 0, 0).to_color(false), Color::Indexed(16));
        assert_eq!(Rgb(128, 128, 128).to_color(false), Color::Indexed(244));
    }
}
