//! Configuration for pulse-viz: colors, sensitivity, quality, frame rate, mode.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self { r: 0, g: 255, b: 255 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColorScheme {
    Cyan,
    Green,
    Purple,
    Rainbow,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Cyan
    }
}

impl ColorScheme {
    pub fn color(&self, pos: f32, total: f32) -> Color {
        let v = pos / total.max(1.0f32);
        match self {
            Self::Cyan => Color {
                r: 0,
                g: (255.0f32 * v).clamp(0.0f32, 255.0f32) as u8,
                b: (255.0f32 * (1.0f32 - v)).clamp(0.0f32, 255.0f32) as u8,
            },
            Self::Green => Color {
                r: 0,
                g: (255.0f32 * v).clamp(0.0f32, 255.0f32) as u8,
                b: (255.0f32 * (1.0f32 - v)).clamp(0.0f32, 255.0f32) as u8,
            },
            Self::Purple => Color {
                r: (255.0f32 * (1.0f32 - v)).clamp(0.0f32, 255.0f32) as u8,
                g: 0,
                b: (255.0f32 * (0.5f32 + 0.5f32 * v)).clamp(0.0f32, 255.0f32) as u8,
            },
            Self::Rainbow => {
                let h = v * 360.0f32;
                let (r, g, b) = hsv2rgb(h);
                Color {
                    r: (r * 255.0f32).clamp(0.0f32, 255.0f32) as u8,
                    g: (g * 255.0f32).clamp(0.0f32, 255.0f32) as u8,
                    b: (b * 255.0f32).clamp(0.0f32, 255.0f32) as u8,
                }
            }
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Cyan => "cyan",
            Self::Green => "green",
            Self::Purple => "purple",
            Self::Rainbow => "rainbow",
        }
    }
}

impl FromStr for ColorScheme {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "green" => Self::Green,
            "purple" => Self::Purple,
            "rainbow" => Self::Rainbow,
            _ => Self::Cyan,
        })
    }
}

fn hsv2rgb(h: f32) -> (f32, f32, f32) {
    let h = (h % 360.0f32).max(0.0f32);
    let i = (h / 60.0f32) as u32;
    let k = (h / 60.0f32).fract();
    let (r, g, b): (f32, f32, f32) = match i {
        0 => (1.0, k, 0.0),
        1 => (0.0, 1.0, k),
        2 => (0.0, 1.0 - k, 1.0),
        3 => (k, 0.0, 1.0),
        4 => (1.0, 0.0, 1.0 - k),
        _ => (1.0 - k, 0.0, 1.0),
    };
    (r, g, b)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: String,
    pub color_scheme: ColorScheme,
    pub sensitivity: f32,
    pub quality: u32,
    pub framerate: u32,
    pub source_filter: Option<String>,
    pub window_width: u16,
    pub window_height: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: "bars".to_string(),
            color_scheme: ColorScheme::Cyan,
            sensitivity: 1.0,
            quality: 64,
            framerate: 30,
            source_filter: None,
            window_width: 80,
            window_height: 24,
        }
    }
}

impl Config {
    pub fn load(path: &PathBuf) -> Self {
        let mut cfg = Self::default();
        if path.exists() {
            if let Ok(toml) = std::fs::read_to_string(path) {
                if let Ok(v) = toml::from_str::<Config>(&toml) {
                    cfg = v;
                }
            }
        }
        cfg
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        let toml = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        std::fs::write(path, toml)
    }

    pub fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or("/home/dox187".to_string());
        PathBuf::from(home).join(".config/pulse-viz.toml")
    }
}
