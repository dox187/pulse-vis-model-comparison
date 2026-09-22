use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Spectrum,
    Waveform,
    Spectrogram,
    Vectorscope,
    Vu,
    Radial,
}
impl Mode {
    pub const ALL: [Self; 6] = [
        Self::Spectrum,
        Self::Waveform,
        Self::Spectrogram,
        Self::Vectorscope,
        Self::Vu,
        Self::Radial,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Spectrum => "Spectrum",
            Self::Waveform => "Waveform",
            Self::Spectrogram => "Spectrogram",
            Self::Vectorscope => "Vectorscope",
            Self::Vu => "VU meters",
            Self::Radial => "Radial",
        }
    }
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|m| *m == self).unwrap() + 1) % 6]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
}
impl Quality {
    pub fn size(self) -> usize {
        match self {
            Self::Low => 1024,
            Self::Medium => 2048,
            Self::High => 4096,
            Self::Ultra => 8192,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Ultra,
            Self::Ultra => Self::Low,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mode: Mode,
    pub quality: Quality,
    pub fps: u16,
    pub sensitivity: f32,
    pub smoothing: f32,
    pub min_frequency: f32,
    pub max_frequency: f32,
    pub floor_db: f32,
    pub auto_gain: bool,
    pub bar_width: u16,
    pub bar_gap: u16,
    pub theme: String,
    pub background: String,
    pub foreground: String,
    pub gradient: Vec<String>,
    pub show_peaks: bool,
    pub apps: Vec<String>,
    pub sink: Option<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Spectrum,
            quality: Quality::High,
            fps: 60,
            sensitivity: 1.0,
            smoothing: 0.65,
            min_frequency: 30.0,
            max_frequency: 18000.0,
            floor_db: -65.0,
            auto_gain: true,
            bar_width: 2,
            bar_gap: 1,
            theme: "aurora".into(),
            background: "#090E1A".into(),
            foreground: "#D8E2F0".into(),
            gradient: vec!["#45E0B8".into(), "#55A7FF".into(), "#C58CFF".into()],
            show_peaks: true,
            apps: vec![],
            sink: None,
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let config = match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("Invalid config: {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if !(10..=144).contains(&self.fps) {
            bail!("fps must be between 10 and 144");
        }
        if !self.sensitivity.is_finite() || !(0.05..=20.0).contains(&self.sensitivity) {
            bail!("sensitivity must be 0.05..20");
        }
        if !self.smoothing.is_finite() || !(0.0..=0.98).contains(&self.smoothing) {
            bail!("smoothing must be 0..0.98");
        }
        if !self.min_frequency.is_finite()
            || !self.max_frequency.is_finite()
            || self.min_frequency < 20.0
            || self.max_frequency > 24000.0
            || self.max_frequency <= self.min_frequency
        {
            bail!("frequency range must be within 20..24000 Hz, min < max");
        }
        if !self.floor_db.is_finite() || !(-120.0..=-10.0).contains(&self.floor_db) {
            bail!("floor_db must be -120..-10");
        }
        if !(1..=8).contains(&self.bar_width) || self.bar_gap > 4 {
            bail!("bar_width must be 1..8 and bar_gap 0..4");
        }
        if !["aurora", "fire", "ocean", "mono", "custom"].contains(&self.theme.as_str()) {
            bail!("theme must be aurora, fire, ocean, mono, or custom");
        }
        if self.gradient.len() < 2 || self.gradient.len() > 16 {
            bail!("gradient must contain 2..16 colors");
        }
        for color in self
            .gradient
            .iter()
            .chain([&self.background, &self.foreground])
        {
            parse_color(color)?;
        }
        if self.apps.iter().any(|s| s.trim().is_empty()) {
            bail!("app filters cannot be empty");
        }
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(tmp, path).with_context(|| format!("Cannot save {}", path.display()))
    }
}
pub fn parse_color(s: &str) -> Result<(u8, u8, u8)> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.is_ascii() {
        bail!("Invalid color {s:?}: expected #RRGGBB");
    }
    let n = u32::from_str_radix(hex, 16).with_context(|| format!("Invalid color {s:?}"))?;
    Ok(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
pub fn default_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pulse-vis/config.toml")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_roundtrip() {
        let c = Config::default();
        let decoded: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        decoded.validate().unwrap();
    }
    #[test]
    fn rejects_invalid_values() {
        let mut c = Config {
            sensitivity: f32::NAN,
            ..Config::default()
        };
        assert!(c.validate().is_err());
        c.sensitivity = 1.0;
        c.max_frequency = 10.0;
        assert!(c.validate().is_err());
        assert!(parse_color("#ééé").is_err());
    }
}
