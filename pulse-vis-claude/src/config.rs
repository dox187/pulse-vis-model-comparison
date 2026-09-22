//! Configuration model: TOML file, built-in defaults and the CLI overrides that map onto it.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Visualization modes. The order here is the order of the `Tab` key and the digit keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Bars,
    Mirror,
    Wave,
    Vu,
    Spectrogram,
    Lissajous,
    Radial,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::Bars,
        Mode::Mirror,
        Mode::Wave,
        Mode::Vu,
        Mode::Spectrogram,
        Mode::Lissajous,
        Mode::Radial,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Mode::Bars => "Spectrum bars",
            Mode::Mirror => "Mirrored spectrum",
            Mode::Wave => "Oscilloscope",
            Mode::Vu => "VU meters",
            Mode::Spectrogram => "Spectrogram",
            Mode::Lissajous => "Vectorscope",
            Mode::Radial => "Radial spectrum",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    pub fn next(self) -> Mode {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Mode {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn from_digit(d: u32) -> Option<Mode> {
        Self::ALL.get(d.checked_sub(1)? as usize).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Channels {
    Mono,
    Stereo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum WaveStyle {
    Line,
    Filled,
    Dots,
}

impl WaveStyle {
    pub fn next(self) -> Self {
        match self {
            WaveStyle::Line => WaveStyle::Filled,
            WaveStyle::Filled => WaveStyle::Dots,
            WaveStyle::Dots => WaveStyle::Line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SpectrogramDirection {
    Up,
    Down,
    Left,
}

impl SpectrogramDirection {
    pub fn next(self) -> Self {
        match self {
            SpectrogramDirection::Up => SpectrogramDirection::Down,
            SpectrogramDirection::Down => SpectrogramDirection::Left,
            SpectrogramDirection::Left => SpectrogramDirection::Up,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum MarkerKind {
    Braille,
    Octant,
    HalfBlock,
    Block,
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Scale {
    Log,
    Linear,
    Sqrt,
}

impl Scale {
    pub fn next(self) -> Self {
        match self {
            Scale::Log => Scale::Sqrt,
            Scale::Sqrt => Scale::Linear,
            Scale::Linear => Scale::Log,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BandMode {
    Peak,
    Average,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum GradientDirection {
    Vertical,
    Horizontal,
    Level,
    Solid,
}

impl GradientDirection {
    pub fn next(self) -> Self {
        match self {
            GradientDirection::Vertical => GradientDirection::Horizontal,
            GradientDirection::Horizontal => GradientDirection::Level,
            GradientDirection::Level => GradientDirection::Solid,
            GradientDirection::Solid => GradientDirection::Vertical,
        }
    }
}

/// Coarse quality presets exposed on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayConfig {
    pub mode: Mode,
    pub fps: u32,
    pub channels: Channels,
    pub peaks: bool,
    pub peak_hold_ms: f32,
    pub peak_gravity: f32,
    pub status_bar: bool,
    pub wave_style: WaveStyle,
    pub wave_ms: f32,
    pub xy_ms: f32,
    pub spectrogram_direction: SpectrogramDirection,
    pub lissajous_rotate: bool,
    pub marker: MarkerKind,
    pub radial_bars: usize,
    pub truecolor: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Bars,
            fps: 60,
            channels: Channels::Stereo,
            peaks: true,
            peak_hold_ms: 250.0,
            peak_gravity: 3.0,
            status_bar: true,
            wave_style: WaveStyle::Line,
            wave_ms: 40.0,
            xy_ms: 40.0,
            spectrogram_direction: SpectrogramDirection::Up,
            lissajous_rotate: true,
            marker: MarkerKind::Braille,
            radial_bars: 0,
            truecolor: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    pub source: String,
    pub sample_rate: u32,
    pub buffer_ms: u32,
    pub server: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source: "all".into(),
            sample_rate: 44100,
            buffer_ms: 10,
            server: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnalysisConfig {
    pub fft_size: usize,
    pub freq_min: f32,
    pub freq_max: f32,
    pub bars: usize,
    pub bar_width: u16,
    pub bar_gap: u16,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub monstercat: f32,
    pub tilt_db_per_oct: f32,
    pub scale: Scale,
    pub band_mode: BandMode,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            fft_size: 4096,
            freq_min: 40.0,
            freq_max: 16000.0,
            bars: 0,
            bar_width: 2,
            bar_gap: 1,
            attack_ms: 20.0,
            decay_ms: 180.0,
            monstercat: 1.5,
            tilt_db_per_oct: 2.5,
            scale: Scale::Log,
            band_mode: BandMode::Peak,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SensitivityConfig {
    pub gain_db: f32,
    pub auto_gain: bool,
    pub dynamic_range_db: f32,
    pub auto_gain_release_db_per_s: f32,
    pub noise_floor_db: f32,
}

impl Default for SensitivityConfig {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            auto_gain: true,
            dynamic_range_db: 50.0,
            auto_gain_release_db_per_s: 4.0,
            noise_floor_db: -90.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorConfig {
    pub theme: String,
    pub gradient: Vec<String>,
    pub direction: GradientDirection,
    pub background: String,
    pub peak: String,
    pub text: String,
    pub accent: String,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            theme: "spectrum".into(),
            gradient: vec!["#00e5ff".into(), "#7c4dff".into(), "#ff4081".into()],
            direction: GradientDirection::Vertical,
            background: "default".into(),
            peak: "#ffffff".into(),
            text: "#c8c8c8".into(),
            accent: "#7c4dff".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub display: DisplayConfig,
    pub audio: AudioConfig,
    pub analysis: AnalysisConfig,
    pub sensitivity: SensitivityConfig,
    pub colors: ColorConfig,
}

impl Config {
    /// `$XDG_CONFIG_HOME/pulse-vis-claude/config.toml`
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("pulse-vis-claude")
            .join("config.toml")
    }

    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Loads the file if it exists, otherwise returns the defaults.
    pub fn load_or_default(path: &Path) -> Result<Config> {
        if path.is_file() {
            Self::load(path)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let a = &self.analysis;
        anyhow::ensure!(
            a.fft_size.is_power_of_two() && (256..=32768).contains(&a.fft_size),
            "analysis.fft_size must be a power of two between 256 and 32768"
        );
        anyhow::ensure!(a.freq_min >= 1.0 && a.freq_min < a.freq_max, "analysis.freq_min/freq_max are inconsistent");
        anyhow::ensure!(a.bar_width >= 1, "analysis.bar_width must be at least 1");
        anyhow::ensure!((10..=240).contains(&self.display.fps), "display.fps must be between 10 and 240");
        anyhow::ensure!(
            (8000..=192000).contains(&self.audio.sample_rate),
            "audio.sample_rate must be between 8000 and 192000"
        );
        anyhow::ensure!(self.sensitivity.dynamic_range_db > 1.0, "sensitivity.dynamic_range_db must be positive");
        Ok(())
    }

    pub fn apply_quality(&mut self, q: Quality) {
        match q {
            Quality::Low => {
                self.analysis.fft_size = 1024;
                self.display.fps = 30;
                self.analysis.bar_width = 3;
                self.audio.buffer_ms = 20;
            }
            Quality::Medium => {
                self.analysis.fft_size = 2048;
                self.display.fps = 45;
                self.audio.buffer_ms = 15;
            }
            Quality::High => {
                self.analysis.fft_size = 4096;
                self.display.fps = 60;
                self.audio.buffer_ms = 10;
            }
            Quality::Ultra => {
                self.analysis.fft_size = 8192;
                self.display.fps = 120;
                self.analysis.bar_width = 1;
                self.audio.buffer_ms = 5;
            }
        }
    }
}

/// Fully commented default configuration; `--dump-config` prints this.
pub const DEFAULT_CONFIG_TOML: &str = r##"# pulse-vis-claude configuration
# Default location: ~/.config/pulse-vis-claude/config.toml   (override with --config PATH)
# Every key is optional; anything missing falls back to the value shown here.
# Press `w` inside the program to write the current settings to this file.

[display]
# Start-up mode: bars | mirror | wave | vu | spectrogram | lissajous | radial
mode = "bars"
# Target frame rate (10-240)
fps = 60
# mono | stereo  (stereo: left channel grows leftwards from the centre, right channel rightwards)
channels = "stereo"
# Falling peak markers above the bars
peaks = true
peak_hold_ms = 250.0
# Peak fall acceleration, in screen heights per second squared
peak_gravity = 3.0
# Status bar at the bottom
status_bar = true
# Oscilloscope drawing style: line | filled | dots
wave_style = "line"
# Time window of the oscilloscope and of the vectorscope, in milliseconds
wave_ms = 40.0
xy_ms = 40.0
# Spectrogram scroll direction: up | down | left
spectrogram_direction = "up"
# Rotate the vectorscope by 45 degrees (mid/side goniometer view)
lissajous_rotate = true
# Dot marker used by the canvas based modes: braille | octant | half-block | block | dot
marker = "braille"
# Number of bars in radial mode (0 = automatic)
radial_bars = 0
# Use 24-bit colours; set to false on terminals limited to 256 colours
truecolor = true

[audio]
# What to capture:
#   "all"             everything that is played on the default output
#   "app:Firefox"     one application only (case-insensitive substring of its name)
#   "sink:NAME"       everything that is played on a specific output
#   "source:NAME"     a capture device such as a microphone
source = "all"
# Capture sample rate; the server resamples when needed
sample_rate = 44100
# Capture fragment length in milliseconds (lower = less latency, more wake-ups)
buffer_ms = 10
# PulseAudio server address ("" = default)
server = ""

[analysis]
# FFT window length in samples (power of two, 256-32768). Larger = finer bass resolution, more latency.
fft_size = 4096
freq_min = 40.0
freq_max = 16000.0
# Number of bars (0 = as many as fit on screen)
bars = 0
bar_width = 2
bar_gap = 1
# Temporal smoothing time constants
attack_ms = 20.0
decay_ms = 180.0
# Neighbour smoothing ("monstercat" style); 1.0 disables it, 1.3-2.0 is typical
monstercat = 1.5
# Spectral tilt in dB per octave relative to 1 kHz (music has less energy at high frequencies)
tilt_db_per_oct = 2.5
# Bar height scale: log | linear | sqrt
scale = "log"
# How FFT bins are folded into one bar: peak | average
band_mode = "peak"

[sensitivity]
# Manual gain in dB (the + and - keys change it)
gain_db = 0.0
# Adapt the gain automatically to the loudest recent band
auto_gain = true
# Displayed dynamic range in dB (log scale)
dynamic_range_db = 50.0
# How fast auto gain recovers after a loud passage
auto_gain_release_db_per_s = 4.0
# Bands quieter than this (dBFS) are treated as silence
noise_floor_db = -90.0

[colors]
# Preset name (see --list-themes) or "custom"
theme = "spectrum"
# Custom gradient, low -> high, used when theme = "custom"
gradient = ["#00e5ff", "#7c4dff", "#ff4081"]
# Gradient mapping: vertical (by height) | horizontal (by frequency) | level (by loudness) | solid
direction = "vertical"
# Background: "default" keeps the terminal background, or give a hex colour such as "#000000"
background = "default"
peak = "#ffffff"
text = "#c8c8c8"
accent = "#7c4dff"
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_matches_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG_TOML).expect("template parses");
        assert_eq!(parsed, Config::default());
        parsed.validate().unwrap();
    }

    #[test]
    fn roundtrip() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let cfg: Config = toml::from_str("[display]\nfps = 30\n").unwrap();
        assert_eq!(cfg.display.fps, 30);
        assert_eq!(cfg.analysis.fft_size, 4096);
    }

    #[test]
    fn mode_cycle() {
        assert_eq!(Mode::Radial.next(), Mode::Bars);
        assert_eq!(Mode::Bars.prev(), Mode::Radial);
        assert_eq!(Mode::from_digit(3), Some(Mode::Wave));
        assert_eq!(Mode::from_digit(0), None);
    }
}
