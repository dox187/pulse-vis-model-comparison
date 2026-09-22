//! pulse-vis-claude: a terminal audio visualizer for PulseAudio and PipeWire.

mod app;
mod audio;
mod config;
mod dsp;
mod theme;
mod ui;

use anyhow::Result;
use audio::{AudioHandle, Connection, Target};
use clap::Parser;
use config::{Channels, Config, DEFAULT_CONFIG_TOML, Mode, Quality};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "pulse-vis-claude",
    version,
    about = "Terminal audio visualizer for PulseAudio and PipeWire",
    after_help = "\
Command-line options override the configuration file for this run only.
Inside the program press ? for the key reference, s to pick an application, w to save settings."
)]
struct Cli {
    /// Configuration file (default: ~/.config/pulse-vis-claude/config.toml)
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Visualization mode to start with
    #[arg(short, long, value_enum)]
    mode: Option<Mode>,

    /// Capture only this application (case-insensitive substring of its name)
    #[arg(short, long, value_name = "NAME", conflicts_with_all = ["all", "sink", "source"])]
    app: Option<String>,

    /// Capture everything played on the default output
    #[arg(long, conflicts_with_all = ["sink", "source"])]
    all: bool,

    /// Capture everything played on a specific output (name or description substring)
    #[arg(long, value_name = "NAME", conflicts_with = "source")]
    sink: Option<String>,

    /// Capture an input device such as a microphone (source name)
    #[arg(long, value_name = "NAME")]
    source: Option<String>,

    /// List playing applications and outputs, then exit
    #[arg(short, long)]
    list: bool,

    /// Frames per second (10-240)
    #[arg(long)]
    fps: Option<u32>,

    /// FFT size (power of two, 256-32768)
    #[arg(long)]
    fft: Option<usize>,

    /// Number of bars (0 = fit to width)
    #[arg(long)]
    bars: Option<usize>,

    /// Colour theme (see --list-themes) or "custom"
    #[arg(short, long)]
    theme: Option<String>,

    /// Custom gradient as comma-separated hex colours, low to high (implies --theme custom)
    #[arg(long, value_name = "HEX,HEX,...")]
    gradient: Option<String>,

    /// Manual gain in dB
    #[arg(short, long)]
    gain: Option<f32>,

    /// Disable automatic gain
    #[arg(long)]
    no_auto_gain: bool,

    /// Quality preset (sets FFT size, frame rate and buffering)
    #[arg(short, long, value_enum)]
    quality: Option<Quality>,

    /// Show both channels separately
    #[arg(long, conflicts_with = "mono")]
    stereo: bool,

    /// Show the mixed-down signal only
    #[arg(long)]
    mono: bool,

    /// Hide the status bar
    #[arg(long)]
    no_status: bool,

    /// Use 256-colour output instead of 24-bit colour
    #[arg(long)]
    no_truecolor: bool,

    /// PulseAudio server address
    #[arg(long, value_name = "ADDR")]
    server: Option<String>,

    /// Print the fully commented default configuration and exit
    #[arg(long)]
    dump_config: bool,

    /// List the built-in colour themes and exit
    #[arg(long)]
    list_themes: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.dump_config {
        print_all(DEFAULT_CONFIG_TOML);
        return Ok(());
    }
    if cli.list_themes {
        let text: String = theme::PRESETS.iter().map(|(name, stops)| format!("{name:<12} {}\n", stops.join(" "))).collect();
        print_all(&text);
        return Ok(());
    }

    let path = cli.config.clone().unwrap_or_else(Config::default_path);
    let mut cfg = Config::load_or_default(&path)?;
    apply_cli(&mut cfg, &cli);
    cfg.validate()?;

    if cli.list {
        return list_sources(&cfg);
    }

    let target = Target::parse(&cfg.audio.source);
    let app = app::App::new(cfg, path, target);
    app::run(app)
}

/// Writes to stdout, silently stopping when the reader goes away (e.g. `| head`).
fn print_all(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

fn apply_cli(cfg: &mut Config, cli: &Cli) {
    if let Some(q) = cli.quality {
        cfg.apply_quality(q);
    }
    if let Some(m) = cli.mode {
        cfg.display.mode = m;
    }
    if let Some(a) = &cli.app {
        cfg.audio.source = format!("app:{a}");
    } else if let Some(s) = &cli.sink {
        cfg.audio.source = format!("sink:{s}");
    } else if let Some(s) = &cli.source {
        cfg.audio.source = format!("source:{s}");
    } else if cli.all {
        cfg.audio.source = "all".into();
    }
    if let Some(fps) = cli.fps {
        cfg.display.fps = fps;
    }
    if let Some(fft) = cli.fft {
        cfg.analysis.fft_size = fft;
    }
    if let Some(b) = cli.bars {
        cfg.analysis.bars = b;
    }
    if let Some(t) = &cli.theme {
        cfg.colors.theme = t.clone();
    }
    if let Some(g) = &cli.gradient {
        cfg.colors.gradient = g.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        cfg.colors.theme = "custom".into();
    }
    if let Some(g) = cli.gain {
        cfg.sensitivity.gain_db = g;
    }
    if cli.no_auto_gain {
        cfg.sensitivity.auto_gain = false;
    }
    if cli.stereo {
        cfg.display.channels = Channels::Stereo;
    }
    if cli.mono {
        cfg.display.channels = Channels::Mono;
    }
    if cli.no_status {
        cfg.display.status_bar = false;
    }
    if cli.no_truecolor {
        cfg.display.truecolor = false;
    }
    if let Some(s) = &cli.server {
        cfg.audio.server = s.clone();
    }
}

fn list_sources(cfg: &Config) -> Result<()> {
    let audio = AudioHandle::start(&cfg.audio, Target::None);
    let start = Instant::now();
    let status = loop {
        let st = audio.status();
        let done = st.apps_loaded && st.inputs_loaded && !st.sinks.is_empty() && !st.default_sink.is_empty();
        if done || matches!(st.connection, Connection::Failed(_)) || start.elapsed() > Duration::from_secs(4) {
            break st;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    if let Connection::Failed(e) = &status.connection {
        anyhow::bail!("cannot connect to the audio server: {e}");
    }
    use std::fmt::Write as _;
    let mut out = String::new();
    if !status.server_name.is_empty() {
        let _ = writeln!(out, "Server: {}", status.server_name);
    }
    let _ = writeln!(out, "Outputs (sinks):");
    for s in &status.sinks {
        let def = if s.name == status.default_sink { "  [default]" } else { "" };
        let _ = writeln!(out, "  {:<4} {}{}", s.index, if s.description.is_empty() { &s.name } else { &s.description }, def);
        let _ = writeln!(out, "       name: {}", s.name);
    }
    let _ = writeln!(out, "Inputs (sources):");
    for i in status.inputs.iter().filter(|i| !i.is_monitor) {
        let def = if i.name == status.default_source { "  [default]" } else { "" };
        let _ = writeln!(out, "  {:<4} {}{}", i.index, if i.description.is_empty() { &i.name } else { &i.description }, def);
        let _ = writeln!(out, "       name: {}", i.name);
    }
    let _ = writeln!(out, "Applications (sink inputs):");
    if status.apps.is_empty() {
        let _ = writeln!(out, "  (none playing)");
    }
    for a in &status.apps {
        let state = if a.corked { " (paused)" } else if a.muted { " (muted)" } else { "" };
        let _ = writeln!(out, "  {:<4} {}{}", a.index, a.app_name, state);
        if !a.media_name.is_empty() && a.media_name != a.app_name {
            let _ = writeln!(out, "       title: {}", a.media_name);
        }
        if !a.binary.is_empty() {
            let _ = writeln!(out, "       binary: {}   sink: {}", a.binary, a.sink);
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "Use --app NAME to follow one application, --sink NAME for one output, --source NAME for an input, --all for everything.");
    print_all(&out);
    Ok(())
}
