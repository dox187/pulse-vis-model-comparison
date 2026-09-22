//! Entry point for the pulse-viz TUI audio visualizer.

mod audio;
mod config;
mod fft;
mod ffi;
mod tui;

use crate::audio::VizEngine;
use crate::config::Config;
use crate::ffi::{Context, Mainloop, SourceInfo, SrcOp, get_source_list};
use crate::tui::Tui;

use std::io::Write;
use std::time::{Duration, Instant};

const PA_CONTEXT_CONNECTED: i32 = 3;
const PA_OPERATION_DONE: i32 = 1;
const PA_OPERATION_CANCELLED: i32 = 2;
const PA_STREAM_READY: i32 = 2;

fn parse_args() -> (Config, Vec<String>) {
    let cfg_path = Config::config_path();
    let mut config = Config::load(&cfg_path);
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" | "-m" => {
                if let Some(mode) = args.next() {
                    config.mode = mode;
                }
            }
            "--source" => {
                if let Some(src) = args.next() {
                    config.source_filter = Some(src);
                }
            }
            "--help" | "-h" => {
                return (config, vec!["help".to_string()]);
            }
            _ => {}
        }
    }
    (config, vec!["run".to_string()])
}

fn main() {
    let (config, action) = parse_args();
    if action[0] == "help" {
        print_help();
        return;
    }

    // Set up TUI.
    let tui = match Tui::new() {
        Ok(t) => t,
        Err(e) => { eprintln!("TUI init failed: {}", e); return; }
    };
    let term_width = (tui.width() as usize - 2).max(20);
    let term_height = (tui.height() as usize - 3).max(8);
    let buffer_size = (term_height.max(8) as usize).max(256);
    let capacity = config.quality.max(16) as usize;

    // Connect to PulseAudio.
    let mainloop = Mainloop::new();
    let ctx = Context::new(&mainloop);
    if ctx.connect() != 0 {
        let errno = ctx.errno();
        eprintln!("Failed to connect to PulseAudio (errno={})", errno);
        return;
    }
    if ctx.state() != PA_CONTEXT_CONNECTED {
        let errno = ctx.errno();
        eprintln!("PulseAudio connection failed (errno={})", errno);
        return;
    }

    // Enumerate sources.
    let mut sources = enumerate_sources(&mainloop, &ctx);
    if sources.is_empty() {
        eprintln!("No playback sources found.");
        let _ = tui.write_frame(&simple_frame("No sources", term_width, term_height));
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(2));
        return;
    }

    // Apply source filter.
    if let Some(filter) = &config.source_filter {
        let fl = filter.to_lowercase();
        let before = sources.len();
        sources.retain(|s| {
            s.name.to_lowercase().contains(&fl) || s.alias.to_lowercase().contains(&fl)
        });
        if sources.len() < before {
            eprintln!("Filtered sources (filter: {})", filter);
        }
    }

    if sources.is_empty() {
        eprintln!("No matching sources (filter: {})",
                 config.source_filter.as_deref().unwrap_or("none"));
        return;
    }

    // Create a stream for each source.
    let mut streams = Vec::new();
    for src in &sources {
        let stream = ctx.stream_new(&src.name, src.rate, src.channels, src.sample_format);
        streams.push(stream);
    }

    // Extract values we still need after the move.
    let framerate = config.framerate.max(1) as u32;
    let source_aliases: Vec<String> = sources.iter().map(|s| s.alias.clone()).collect();
    let n_sources = sources.len();

    // Build the VizEngine (takes config and sources by value).
    let mut engine = VizEngine::new(config, sources, capacity);

    // Wait for streams to become READY.
    let mut ready = 0;
    for _ in 0..200 {
        if ready == streams.len() { break; }
        for s in &streams {
            if s.get_state() == PA_STREAM_READY {
                ready += 1;
            }
        }
        mainloop.iterate(false);
        std::thread::sleep(Duration::from_millis(10));
    }
    if ready == 0 {
        eprintln!("No streams were ready.");
        return;
    }

    let mut source_idx = 0usize;
    let mut last_render = Instant::now();

    // Main render loop.
    loop {
        // Poll stdin for keypresses (non-blocking).
        while let Some(key) = tui.poll_key() {
            match key {
                b'q' | 3 | b'\x1b' => { return; }
                b'1' => { engine.set_mode_name("bars"); }
                b'2' => { engine.set_mode_name("scope"); }
                b'3' => { engine.set_mode_name("spectrum"); }
                b'4' => { engine.set_mode_name("waveform"); }
                b'5' => { engine.set_mode_name("meters"); }
                b'\x15' => { /* cycle modes (if needed) */ }
                b'.' => {
                    source_idx = (source_idx + 1) % (n_sources + 1);
                    engine.set_selected_source(source_idx);
                }
                b',' => {
                    source_idx = if source_idx == 0 { n_sources } else { source_idx - 1 };
                    engine.set_selected_source(source_idx);
                }
                _ => {}
            }
        }

        // Read audio from all streams.
        engine.read_all(&streams, buffer_size as usize);
        mainloop.iterate(false);

        // Render frame.
        let now = Instant::now();
        if now - last_render >= Duration::from_millis(framerate as u64) {
            let source_name = if source_idx == 0 {
                "all".to_string()
            } else if source_idx > 0 && source_idx <= n_sources {
                source_aliases[source_idx - 1].clone()
            } else {
                "unknown".to_string()
            };
            let lines = engine.render(term_width, term_height);
            let mut frame = vec![String::new(); term_height + 3];
            frame[0].push_str(&format!(
                "  pulse-viz  [{}]  Source: {}",
                engine.get_mode_name(), source_name
            ));
            frame[1].push_str(&"=".repeat(term_width));
            for i in 0..term_height {
                frame[2 + i].push_str(&lines[i]);
            }
            frame[term_height + 2].push_str(&format!(
                "[1] bars [2] scope [3] spectrum [4] waveform [5] meters  [, .] source  [q] quit  ({} sources)",
                n_sources
            ));
            let _ = tui.write_frame(&frame);
            last_render = now;
        }
    }
}

fn enumerate_sources(mainloop: &Mainloop, ctx: &Context) -> Vec<SourceInfo> {
    let op = SrcOp::new(unsafe { crate::ffi::deps::pulse_viz_start_source_enum(ctx.0) });
    let mut sources = Vec::new();
    for _ in 0..100 {
        match op.get_state() {
            PA_OPERATION_DONE => { sources = get_source_list(&op); break; }
            PA_OPERATION_CANCELLED => break,
            _ => { mainloop.iterate(true); }
        }
    }
    sources
}

fn simple_frame(title: &str, _w: usize, h: usize) -> Vec<String> {
    let mut rows = vec![String::new(); h.max(1) + 2];
    rows[0] = "  pulse-viz".to_string();
    if h >= 1 {
        rows[1] = title.to_string();
    }
    rows
}

fn print_help() {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "Usage: pulse-viz [OPTIONS]");
    let _ = writeln!(out, "");
    let _ = writeln!(out, "TUI audio visualizer for PulseAudio.");
    let _ = writeln!(out, "");
    let _ = writeln!(out, "  -m, --mode MODE     Visualization mode: bars | scope | spectrum | waveform | meters (default: bars)");
    let _ = writeln!(out, "      --source NAME   Filter by source name (substring match)");
    let _ = writeln!(out, "  -h, --help          Show this help and exit");
    let _ = writeln!(out, "");
    let _ = writeln!(out, "Keyboard shortcuts:");
    let _ = writeln!(out, "  [1]-[5]    Switch mode");
    let _ = writeln!(out, "  [.,]     Cycle through sources");
    let _ = writeln!(out, "  [q]       Quit");
}
