# pulse-vis-claude

A terminal audio visualizer for **PulseAudio** and **PipeWire** (through `pipewire-pulse`),
written in Rust. It shows what your machine is playing, either all of it or one application
at a time, in seven classic visualization modes, with configurable colours, sensitivity
and quality.

```
 SPECTRUM BARS  ▶ Firefox · YouTube  │  gain +0.0 dB · auto +6  │  fft 4096  │  stereo  │  60 fps
```

## Features

- **Seven modes**: spectrum bars, mirrored spectrum, oscilloscope, VU meters, spectrogram,
  vectorscope (Lissajous / goniometer) and radial spectrum. Switch with `Tab` or `1`–`7`.
- **Source selection**: everything on the default output, one application, one specific
  output, or an input device such as a microphone. Pick live from a popup (`s`) or on the
  command line (`--app`, `--sink`, `--source`, `--all`).
- **Per-application capture without re-routing**: a monitor stream is bound to the
  application's playback stream with `pa_stream_set_monitor_stream`, the same mechanism
  pavucontrol uses for its per-stream meters. No null sinks, no loopbacks, no module loading,
  and the application keeps playing where it was.
- **Follows applications**: if the chosen application stops or restarts, the visualizer
  waits and re-attaches automatically. Default-output changes are followed too.
- **Colours**: 16 gradient presets plus custom gradients, four gradient mappings (by height,
  by frequency, by level, solid), optional fixed background, 24-bit or 256-colour output.
- **Sensitivity**: manual gain, automatic gain with adjustable release, dynamic range and noise
  floor, log / sqrt / linear scales.
- **Quality**: FFT size 256–32768, 10–240 fps, bar width and gap, attack/decay smoothing,
  neighbour ("monstercat") spreading, spectral tilt, peak-or-average band folding, dot marker
  style for the canvas modes, and `--quality low|medium|high|ultra` presets.
- **Configuration**: one TOML file with a fully commented default (`--dump-config`), command
  line overrides for a single run, and `w` to save the current state from inside the program.
- **Light**: about 2–4 % of one core and ~9 MB RSS in a 160×45 terminal at 60 fps (release
  build, measured on a Ryzen 7 5700X3D).

## Requirements

- Rust 1.88 or newer (edition 2024).
- The libpulse client library: `pulseaudio-libs` on Fedora/Nobara, `libpulse0` on Debian/Ubuntu.
  With PipeWire, `pipewire-pulseaudio` / `pipewire-pulse` must be running (it normally is).
  The development package (`pulseaudio-libs-devel` / `libpulse-dev`) is **not** required: when
  pkg-config does not know libpulse, the build links `libpulse.so.0` directly.
- A terminal with a font that has the Unicode block and braille glyphs, ideally with 24-bit
  colour (`--no-truecolor` for 256-colour terminals).

## Build and run

```sh
cargo build --release
./target/release/pulse-vis-claude            # visualize everything on the default output
cargo install --path .                       # optional: installs into ~/.cargo/bin
```

Examples:

```sh
pulse-vis-claude --list                      # what is playing, which outputs and inputs exist
pulse-vis-claude --app firefox               # follow one application (substring, case-insensitive)
pulse-vis-claude --app mpv --mode radial --theme fire
pulse-vis-claude --sink hdmi --mode spectrogram
pulse-vis-claude --source usb --mode vu      # a microphone or other input device
pulse-vis-claude --quality ultra --fps 144 --bars 64
pulse-vis-claude --gradient "#00ffff,#ff00ff" --no-auto-gain --gain 6
pulse-vis-claude --dump-config > ~/.config/pulse-vis-claude/config.toml
```

If several applications play at once, press `s` and pick one from the list; the list updates
live as streams appear and disappear.

## Keys

| Key | Action |
| --- | --- |
| `q`, `Esc`, `Ctrl-C` | quit (`Esc` closes a popup first) |
| `Tab` / `Shift-Tab`, `m` / `M` | next / previous mode |
| `1` … `7` | bars · mirror · wave · vu · spectrogram · vectorscope · radial |
| `s` | choose the audio source (applications, outputs, inputs) |
| `r` | refresh the source list |
| `+` / `-` | gain +2 dB / −2 dB |
| `a` | toggle automatic gain |
| `c` / `C` | next / previous colour theme |
| `d` | cycle gradient direction (vertical, horizontal, level, solid) |
| `t` | toggle stereo / mono |
| `p` | toggle peak markers |
| `[` / `]` | thinner / wider bars |
| `{` / `}` | smaller / larger gap between bars |
| `,` / `.` | faster / slower decay |
| `<` / `>` | smaller / larger FFT |
| `l` | cycle bar scale (log, sqrt, linear) |
| `v` | cycle oscilloscope style (line, filled, dots) |
| `o` | cycle spectrogram direction (up, down, left) |
| `x` | toggle vectorscope rotation (L/R vs mid/side) |
| `b` | toggle the status bar |
| `Space` | pause / resume |
| `w` | save the current settings to the config file |
| `?`, `h`, `F1` | key reference |

In the source popup: `↑`/`↓` or `j`/`k` move, `Enter` selects, `r` refreshes, `Esc` closes.

## Modes

| # | Mode | What it shows |
| --- | --- | --- |
| 1 | Spectrum bars | Log-frequency bars with 1/8-cell resolution and falling peak caps. In stereo the left channel grows leftwards from the centre and the right channel rightwards. |
| 2 | Mirrored spectrum | The same bars mirrored around the horizontal centre line. |
| 3 | Oscilloscope | The raw waveform of the last `wave_ms` milliseconds; line, filled or dots style; one trace per channel in stereo. |
| 4 | VU meters | RMS bars with peak-hold markers, dBFS read-outs, clip indicator, dB scale and a stereo correlation meter. Absolute levels, unaffected by gain. |
| 5 | Spectrogram | Scrolling time/frequency heat map at two rows per cell; scrolls up, down or left. |
| 6 | Vectorscope | Left against right (or mid/side when rotated) with older samples fading out, plus the correlation coefficient. |
| 7 | Radial spectrum | Bars arranged around a circle, right channel on the right half and left channel mirrored on the left in stereo. |

## Configuration

The file lives at `~/.config/pulse-vis-claude/config.toml` (`--config PATH` to override).
Every key is optional. `pulse-vis-claude --dump-config` prints the complete, commented default
(also checked in as [`config.example.toml`](config.example.toml)); `w` inside the program
writes the current settings back to the file.

Sections:

- `[display]` — mode, fps, mono/stereo, peak markers and their ballistics, status bar,
  oscilloscope style and time window, spectrogram direction, vectorscope rotation, canvas
  marker (`braille`, `octant`, `half-block`, `block`, `dot`), radial bar count, truecolor.
- `[audio]` — `source` (`all`, `app:NAME`, `sink:NAME`, `source:NAME`), capture sample rate,
  fragment size in milliseconds, server address.
- `[analysis]` — FFT size, frequency range, bar count/width/gap, attack and decay time
  constants, monstercat factor, spectral tilt (dB per octave), scale, band folding mode.
- `[sensitivity]` — manual gain, auto gain and its release speed, dynamic range, noise floor.
- `[colors]` — preset name or `custom` with a `gradient` list, gradient direction, background,
  peak, text and accent colours. Presets: `spectrum classic rainbow sunset ocean fire ice
  matrix neon synthwave pastel mono gruvbox nord dracula catppuccin` (`--list-themes`).

## How capture works

A dedicated thread owns a PulseAudio main loop, context and record stream and never blocks
the UI. It subscribes to sink-input, sink, source and server events, so the list of playing
applications and the default output stay current without polling.

- `all` records the monitor source of the default sink.
- `sink:NAME` records the monitor of that sink.
- `source:NAME` records that source (validated against the server's source list).
- `app:NAME` finds the application's sink input, opens a record stream on the monitor of the
  sink it plays to and calls `pa_stream_set_monitor_stream(sink_input_index)`, so the stream
  receives only that application's audio. When the sink input disappears the stream is
  dropped and the visualizer waits for a matching stream to come back.

Samples (32-bit float, stereo, 44.1 kHz by default) land in a ring buffer. Each frame the UI
thread takes the newest samples, runs a Hann-windowed real FFT per channel, folds the bins
into logarithmically spaced bands (peak or average per band, linear interpolation where a
band is narrower than a bin), applies tilt, gain and the chosen scale, spreads neighbours,
smooths in time and updates the peak markers. A 0 dBFS sine reads 1.0 before gain, so the
VU meters show true dBFS.

## Project layout

```
src/main.rs        command line, config loading, --list
src/config.rs      TOML model, defaults, validation, commented template
src/audio.rs       PulseAudio backend thread, targets, ring buffer, status
src/dsp.rs         FFT, band mapping, gain, smoothing, peaks, levels, spectrogram history
src/theme.rs       colours, gradients, presets, 256-colour fallback
src/app.rs         application state, key handling, frame loop
src/ui/            one renderer per mode plus status bar and popups
```

`cargo test` runs the unit tests (config template/roundtrip, DSP calibration and band
placement, ring buffer, target parsing, bar layout, colour handling).

## Development record

This program was written by **Claude Fable 5.1 (xhigh reasoning effort)** running in Claude Code,
from the initial prompt to the verified release build, in one session.

| | |
| --- | --- |
| Prompt submitted | 2026-09-22 01:11 CEST |
| Finished | 2026-09-22 01:52 CEST |
| Elapsed | **41 minutes** |
| Token usage | `claude-fable-5-1: 1.0k input, 197.6k output, 5.7m cache read, 289.4k cache write` |

The time covers design, implementation of all modules, unit tests, and end-to-end testing
in tmux against a live PipeWire server (a test tone played into a temporary null sink,
per-application capture, source switching, application restart, terminal resizing down to
2×1 cells, and CPU measurements of the release build).
