#[allow(unused)]
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
    eprintln!("[DEBUG] main: parsing args...");
    let (config, action) = parse_args();
    if action[0] == "help" {
        print_help();
        return;
    }

    eprintln!("[DEBUG] main: setting up TUI...");
    let tui = match Tui::new() {
        Ok(t) => t,
        Err(e) => { 
            eprintln!("[DEBUG] main: TUI init failed: {}", e);
            eprintln!("[DEBUG] main: exiting gracefully");
            return; 
        }
    };
    eprintln!("[DEBUG] main: TUI init OK, width={}", tui.width());
    let term_width = (tui.width() as usize - 2).max(20);
    let term_height = (tui.height() as usize - 3).max(8);
    let buffer_size = (term_height.max(8) as usize).max(256);
    let capacity = config.quality.max(16) as usize;

    eprintln!("[DEBUG] main: connecting to PulseAudio...");
    let mainloop = Mainloop::new();
    eprintln!("[DEBUG] main: mainloop created, ptr={}", mainloop.0 as usize);
    let ctx = Context::new(&mainloop);
    eprintln!("[DEBUG] main: context created, ptr={}", ctx.0 as usize);
    let conn = ctx.connect();
    eprintln!("[DEBUG] main: connect returned {}", conn);
    if conn != 0 {
        let errno = ctx.errno();
        eprintln!("[DEBUG] main: Failed to connect (errno={})", errno);
        return;
    }
    if ctx.state() != PA_CONTEXT_CONNECTED {
        let errno = ctx.errno();
        eprintln!("[DEBUG] main: PulseAudio connection failed (errno={})", errno);
        return;
    }
    eprintln!("[DEBUG] main: Connected to PulseAudio");

    eprintln!("[DEBUG] main: enumerating sources...");
    let mut sources = enumerate_sources(&mainloop, &ctx);
    eprintln!("[DEBUG] main: found {} sources", sources.len());
    if sources.is_empty() {
        eprintln!("[DEBUG] main: No sources, exiting");
        eprintln!("No playback sources found.");
        let _ = tui.write_frame(&simple_frame("No sources", term_width, term_height));
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(2));
        return;
    }

    if let Some(filter) = &config.source_filter {
        let fl = filter.to_lowercase();
        let before = sources.len();
        sources.retain(|s| {
            s.name.to_lowercase().contains(&fl) || s.alias.to_lowercase().contains(&fl)
        });
        if sources.len() < before {
            eprintln!("[DEBUG] main: Filtered sources (filter: {}), now {} sources", filter, sources.len());
        }
    }

    if sources.is_empty() {
        eprintln!("[DEBUG] main: No matching sources, exiting");
        return;
    }

    eprintln!("[DEBUG] main: creating streams for {} sources", sources.len());
    let mut streams = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        eprintln!("[DEBUG] main: creating stream {} for source '{}', rate={}, channels={}, format={}", i, src.name, src.rate, src.channels, src.sample_format);
        let stream = ctx.stream_new(&src.name, src.rate, src.channels, src.sample_format);
        let state = stream.get_state();
        eprintln!("[DEBUG] main: stream {} state={}", i, state);
        streams.push(stream);
    }

    let framerate = config.framerate.max(1) as u32;
    let source_aliases: Vec<String> = sources.iter().map(|s| s.alias.clone()).collect();
    let n_sources = sources.len();
    eprintln!("[DEBUG] main: creating VizEngine with {} buffers, capacity={}", n_sources, capacity);
    let mut engine = VizEngine::new(config, sources, capacity);

    eprintln!("[DEBUG] main: waiting for streams to become READY...");
    let mut ready = 0;
    let mut iterations = 0;
    for _ in 0..200 {
        iterations += 1;
        if ready == streams.len() { break; }
        let mut new_ready = 0;
        for (i, s) in streams.iter().enumerate() {
            let state = s.get_state();
            if state == PA_STREAM_READY {
                new_ready += 1;
            }
        }
        // Track only newly ready streams
        ready = new_ready;
        mainloop.iterate(false);
        std::thread::sleep(Duration::from_millis(10));
    }
    eprintln!("[DEBUG] main: after {} iterations, {} streams ready", iterations, ready);
    if ready == 0 {
        eprintln!("[DEBUG] main: No streams were ready, exiting");
        return;
    }

    eprintln!("[DEBUG] main: entering render loop...");
    let mut source_idx = 0usize;
    let mut last_render = Instant::now();
    let mut render_count = 0;

    loop {
        while let Some(key) = tui.poll_key() {
            eprintln!("[DEBUG] main: key={:02x}", key);
            match key {
                b'q' | 3 | b'\x1b' => {
                    eprintln!("[DEBUG] main: quitting");
                    return; 
                }
                b'1' => { engine.set_mode_name("bars"); }
                b'2' => { engine.set_mode_name("scope"); }
                b'3' => { engine.set_mode_name("spectrum"); }
                b'4' => { engine.set_mode_name("waveform"); }
                b'5' => { engine.set_mode_name("meters"); }
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

        render_count += 1;
        if render_count % 100 == 0 {
            eprintln!("[DEBUG] main: render iteration {}...", render_count);
        }
        engine.read_all(&streams, buffer_size as usize);
        mainloop.iterate(false);

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
    eprintln!("[DEBUG] enumerate: starting source enumeration...");
    let op = SrcOp::new(unsafe { crate::ffi::deps::pulse_viz_start_source_enum(ctx.0) });
    let mut sources = Vec::new();
    for iter in 0..100 {
        let state = op.get_state();
        if iter == 0 {
            eprintln!("[DEBUG] enumerate: iteration {}, state={}", iter, state);
        }
        match state {
            PA_OPERATION_DONE => { 
                eprintln!("[DEBUG] enumerate: done");
                sources = get_source_list(&op); 
                break; 
            }
            PA_OPERATION_CANCELLED => { 
                eprintln!("[DEBUG] enumerate: cancelled at iteration {}", iter);
                break; 
            }
            _ => { mainloop.iterate(true); }
        }
    }
    eprintln!("[DEBUG] enumerate: found {} sources", sources.len());
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
