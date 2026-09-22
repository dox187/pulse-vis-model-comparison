# pulse-vis

A native Rust terminal audio visualizer for PulseAudio and PipeWire's PulseAudio compatibility server. Six visualizations, stereo analysis, live settings, and application filtering without moving or muting playback streams.

## Run

```sh
cargo run --release
```

Rust/Cargo and the PulseAudio command-line clients `pactl` and `parec` are required. The TUI and DSP run inside the Rust application; a supervised `parec` process captures each selected monitor or playback stream. No C headers, FFTW, or PulseAudio development package is needed.

On Fedora / Nobara, if the clients are missing:

```sh
sudo dnf install pulseaudio-utils
```

On Debian / Ubuntu: `sudo apt install pulseaudio-utils`. On Arch: `sudo pacman -S libpulse`. A running PulseAudio or `pipewire-pulse` server is required for live capture. A UTF-8 terminal with true color and Braille support gives the best rendering. No graphical environment is required.

```sh
cargo run --release -- --demo                 # offline stereo demonstration
cargo run --release -- --list-sources         # outputs and playback streams
cargo run --release -- --app Brave           # name/binary substring, case insensitive
cargo run --release -- --app Firefox --app Spotify
cargo run --release -- --mode spectrogram --quality ultra
cargo run --release -- --sink alsa_output.pci-0000_0c_00.4.analog-stereo
cargo run --release -- --check 3              # headless capture diagnostic
cargo run --release -- --demo --check 2
cargo install --path .                       # optional installation to ~/.cargo/bin
```

## Visualizations

| Key | Mode | Display |
| --- | --- | --- |
| 1 | Spectrum | Logarithmic FFT bars, gradient, decaying peak markers |
| 2 | Waveform | Triggered left and right channel oscilloscope |
| 3 | Spectrogram | Scrolling frequency history; bass left, treble right, newest at bottom |
| 4 | Vectorscope | Stereo phase plot: mono is vertical, anti-phase horizontal; correlation readout |
| 5 | VU meters | Left/right RMS, peak and clipping indicators in dBFS |
| 6 | Radial | Circular logarithmic spectrum |

Analysis uses a Hann window and independent channel FFT power averaging, so anti-phase stereo does not disappear. Spectrum levels use a configurable dB floor. Gain and smoothing apply to visualizations; VU values always report the original captured signal. AGC has a capped gain and a silence threshold. Smoothing and peak decay account for elapsed frame time.

## Controls

| Key | Action |
| --- | --- |
| `1`–`6`, `Tab` | Select / cycle visualization |
| `a` | Source picker: arrows navigate, Space / Enter select |
| `s` | Settings: arrows navigate and adjust |
| `+` / `-` | Sensitivity |
| `[` / `]` | Smoothing |
| `g` | Automatic gain on/off |
| `p` | Cycle aurora, fire, ocean, mono, custom palettes |
| `Q` | Cycle FFT quality |
| Space | Pause the picture; capture continues |
| `w` | Save current settings and source filters |
| `r` | Reload config |
| `?` | Help |
| Esc | Close overlay |
| `q`, Ctrl+C | Quit and clean up captures |

The source picker can combine multiple applications. A selected application includes all streams matching its case-insensitive name or binary substring. Stream IDs are rediscovered every two seconds, so filters follow restarts and changes of output device. Filters with no current streams remain selected and wait for the application to play again. Removing the last app filter returns to all outputs.

**All outputs** sums monitors from every output sink, including outputs that are not the default. Select a single output to monitor only that device. Mirrored audio routed through multiple sinks can be counted more than once in the combined view. Independently captured streams use their latest analysis windows; the combined display is approximate, not a sample-synchronized recording or a mastering meter. Application capture uses PulseAudio's per-stream monitor facility (`parec --monitor-stream`); servers that reject it show an error and retry. It never silently falls back to the full mix.

Discovery runs outside the rendering thread. Captures use bounded stereo sample rings and expire stale samples after 250 ms. Exited captures and disconnected servers are retried every two seconds. The application does not load audio modules, change the default device, record the microphone, or alter playback routing. Recording clients are terminated and reaped on normal exit, handled errors, SIGINT, SIGTERM, and SIGHUP. As with other terminal apps, SIGKILL cannot run cleanup.

## Configuration

The default file is `$XDG_CONFIG_HOME/pulse-vis/config.toml`, or `~/.config/pulse-vis/config.toml`. Missing config uses defaults. Invalid values report an error. Unknown config keys are rejected to catch typos.

```sh
cargo run -- --init-config
cargo run -- --config ./config.example.toml
```

`--init-config` refuses to overwrite an existing file. Changes in the TUI remain in memory until `w` is pressed; saves use a temporary file and rename. CLI options override the loaded config and are included if you save. `--app` overrides a saved output selection, and `--sink` overrides saved app filters. With no app filters, `sink` selects a single output; with filters, matching streams are followed across all outputs.

See [config.example.toml](config.example.toml) for every setting. For custom colors, set `theme = "custom"` and provide two or more `#RRGGBB` gradient stops. Background and foreground colors apply to every theme. Frequency range and dB floor are configured in the file; the live settings panel covers gain, smoothing, quality, FPS, AGC, themes, bar width/gap, and peaks.

Quality selects the FFT window at 48 kHz: low = 1024, medium = 2048, high = 4096, ultra = 8192 samples. Larger windows improve bass resolution but add temporal averaging and CPU work. The UI frame rate is independent (10–144 FPS); spectrogram history advances at up to 30 rows/second.

## Development and verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
python3 tests/pulse_smoke.py   # optional live test; requires an audio server
python3 tests/tui_smoke.py     # real PTY, keyboard controls, config save, cleanup
```

The live test creates a temporary null sink and two synthetic playback applications, verifies whole-output and per-application capture, then removes only its own streams and sink. Test audio is sent only to that null sink, so it is inaudible.

PulseAudio interface reference: [per-stream monitors](https://wiki.freedesktop.org/www/Software/PulseAudio/Documentation/Developer/Clients/WritingVolumeControlUIs/). Rendering uses [Ratatui](https://ratatui.rs/) and analysis uses [RustFFT](https://docs.rs/rustfft/latest/rustfft/).

## Verified development record

The original Codex sessions identify **gpt-6-astra with high reasoning effort**. The successful application-building turn took **13m 28.064s**. Across both located project sessions, including an earlier interrupted start and two later README updates, recorded active turn time was **14m 10.390s**.

The [original application prompt](../PROMPT.md) is preserved with the comparison.

| Recorded turn | Duration | New input | Cached input | Output | Total including cache |
| --- | ---: | ---: | ---: | ---: | ---: |
| Interrupted initial start | 12.500s | 3,661 | 12,800 | 177 | 16,638 |
| Successful implementation | 13m 28.064s | 56,003 | 889,088 | 24,565 | 969,656 |
| README timing update | 11.543s | 1,050 | 130,944 | 254 | 132,248 |
| README token-usage update | 18.283s | 54,032 | 78,976 | 181 | 133,189 |
| **All project turns** | **14m 10.390s** | **114,746** | **1,111,808** | **25,177** | **1,251,731** |

New input plus output totals **139,923 tokens**. The recorded **2,665 reasoning-output tokens** are a subset of output, not an additional charge to add to that total. Cache-write input is recorded as zero.

### Accounting method

The final cumulative `total_token_usage` from each session was checked against the sum of `token_usage_record.payload.usage`, deduplicated by response ID. The two methods agree. Recorded input includes cache reads, so new input is input minus cached input. Turn durations come from `task_complete` and `turn_aborted` events; idle gaps are excluded.

The old README's 81,872-token line was supplied during a later documentation request and is superseded by this complete breakdown. This audit, screenshot capture, repository publishing and the separate session that repaired the bonsai2 Q2-related desktop audio incident are excluded. Raw logs, personal paths and session identifiers are not published.
