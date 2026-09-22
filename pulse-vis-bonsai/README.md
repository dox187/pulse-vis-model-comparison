# Bonsai Q2 PulseAudio visualizer attempt

**Status: incomplete.** This Rust TUI visualizer was the main subject of a three-model coding experiment. Bonsai Q2 produced a buildable prototype, but did not meaningfully finish working playback capture or application filtering. The source is preserved as the experimental result.

See the [three-version comparison](../README.md) for the GPT and Claude implementations and the overall assessment.

## Intended application and actual result

The task requested a native Rust terminal visualizer with common visualization modes, configurable colors, sensitivity and quality, whole-output capture, and filtering by playback application.

The prototype contains five mode branches: bars, scope, spectrum, waveform and meters. It includes TOML configuration, basic command-line parsing, terminal handling, an FFT implementation, bundled PulseAudio headers, and a C shim with Rust FFI bindings. These components do not establish a working application: the final audio connection, stream setup and sample-read code contain blocking defects.

The earlier README said the project was completed in 5h 12m 22s. That was the initial attempt's duration, not successful delivery or the final amount of work. The initial completion message acknowledged that live audio had not been tested. A subsequent user run reported a segmentation fault; later debugging still did not produce a verified application.

## Development accounting

The audit located **four project-related Bonsai sessions**, including two short aborted starts. Two sessions contain token usage records. Unrelated setup checks and other tasks in the same Codex data directory are excluded.

| Project session, in chronological order | Active turn duration | Input including cache | Cached input, already included | Output | Total including cache |
| --- | ---: | ---: | ---: | ---: | ---: |
| A: initial implementation, README updates and first crash investigation | 8h 39m 43.535s | 67,467,724 | 65,044,373 | 503,139 | 67,970,863 |
| B: aborted README usage update | 0.528s | Not recorded | Not recorded | Not recorded | Not recorded |
| C: aborted repair continuation | 6.402s | Not recorded | Not recorded | Not recorded | Not recorded |
| D: continued client debugging and desktop audio incident | 1h 25m 12.886s | 12,934,232 | 12,556,452 | 72,571 | 13,006,803 |
| **Recorded total** | **10h 05m 03.351s** | **80,401,956** | **77,600,825** | **575,710** | **80,977,666** |

Expressed in the uncached-input convention used by the original README:

```text
Uncached input:                    2,801,131
Output:                              575,710
Uncached input + output:            3,376,841
Cached input, reported separately: 77,600,825
Total including cached input:     80,977,666
```

The calendar span from the first project turn to the final recorded interruption was **15h 51m 40.128s**. Active turn time was **10h 05m 03.351s**, including the failed and interrupted turns, tool work and waits within turns. Neither measure is pure inference time or human working time.

### How the totals were calculated

1. Select sessions by their project requests and continuation context, not merely by their working directory. Bonsai used a separate local Codex data store.
2. Take the last non-null `event_msg.token_count.info.total_token_usage` from each session with usage data. The cumulative counters in those sessions do not reset.
3. Cross-check against the sum of `token_usage_record.payload.usage`, deduplicated by response ID. Both methods produce the same session totals. Do not sum successive cumulative snapshots or add the two record formats together.
4. In these records, `input_tokens` includes `cached_input_tokens`. Calculate uncached input by subtraction; calculate the cache-inclusive total as input plus output. Cache writes and separately classified reasoning-output tokens are recorded as zero; that does not prove the model performed no reasoning.
5. Sum `duration_ms` once per ended turn, using both `task_complete` and `turn_aborted` events. All 15 project turns have an end event. Gaps between turns are excluded.

The two brief aborted starts have no usable usage record; their time is included, but no token amount is invented for them. Usage from any requests not recorded by the provider cannot be reconstructed. These are exact sums of the available records, not a guarantee that every attempted request was metered.

The former README's `1,772,941` total, `1,461,214` input, `38,576,443` cached input and `311,727` output were user-supplied figures for an earlier stage. They are superseded by the all-session totals, not added to them. The initial turn's recorded duration is **5h 12m 22.594s**; subsequent repair attempts account for the additional active time.

### Separate recovery and advice session

The session supplied as the final reference was found in the regular Codex data store. Its recorded model is **`gpt-6-astra` with high reasoning**, not Bonsai. It repaired the desktop audio endpoint and supplied debugging advice.

| Recovery-session measure | Recorded amount |
| --- | ---: |
| Active turn duration, four turns | 2m 00.846s |
| Uncached input | 42,766 |
| Output | 2,513 |
| Uncached input + output | 45,279 |
| Cached input | 263,808 |
| Total including cached input | 309,087 |

This is additional recovery/advice work associated with the failed attempt. It is excluded from Bonsai's usage and from the independently developed GPT visualizer's usage. It restored the audio service, not this program.

### Local setup recorded for the experiment

The project turns identify the `bonsai-local` provider and model alias `bonsai2`, using Codex CLI 0.155.1. The local Q2 launch script and retained server log identify **Bonsai 2 27B, `PQ2_0` quantization**, served through a CUDA build of llama.cpp. The local GPU, checked with `nvidia-smi`, is an **NVIDIA GeForce RTX 4060 Ti with 16 GB VRAM** (16,380 MiB reported).

| Context / launch setting | Evidence |
| --- | --- |
| Q2 server context at the retained startup | **77,824 tokens** (`n_ctx_slot`), with one request slot |
| Codex effective budget at the initial project turn | **73,932 tokens**, recorded in the turn event |
| Brief intermediate continuation | **89,497 tokens**, recorded in a later turn event |
| Final project continuation | **73,932 tokens**, recorded in the turn event |
| Context selection | Automatic from free VRAM by default; explicit override through `BONSAI_CTX` |
| GPU execution settings requested by the launcher | `-ngl 99`, Flash Attention enabled, `--parallel 1` |

The launcher estimates the available context after accounting for model file size, a default **2,048 MiB desktop reserve**, 400 MiB of additional overhead and a 10% margin. It rounds the result down to a multiple of **4,096 tokens**, with an 8,192-token floor and a 262,144-token cap. If GPU-memory detection is unavailable or automatic layer placement is selected, the automatic context fallback is 32,768. These are launch-script choices, not a guarantee that every context size fits every GPU.

The Codex launcher reads the server's per-slot context and passes it to the client; the turn events record a smaller effective budget for the usual 77,824-token allocation. The server allocation and the effective client budget should not be conflated. Since free VRAM and settings changed, the experiment cannot be described as using a fixed context throughout. The retained server log directly confirms the 77,824-token Q2 startup; it does not preserve every earlier server startup.

### Measured prompt and generation speed

The retained Q2 server log contains final prompt/evaluation timing summaries for the last project continuation. Its **303 project response generation counts match the Codex response output counts in order**. Their prompt-token sum also matches that session's uncached input exactly. Seven additional short server responses are excluded from this project sample.

| Throughput measure | Prompt processing: input / “read” | Generation: output / “write” |
| --- | ---: | ---: |
| Matched completed responses | 303 | 303 |
| Tokens represented | 377,780 newly evaluated input tokens | 72,571 generated tokens |
| Median per-request rate | **325.98 tokens/s** | **18.98 tokens/s** |
| 10th-90th percentile | 208.42-489.56 tokens/s | 16.67-22.63 tokens/s |
| Observed minimum-maximum | 138.95-741.61 tokens/s | 13.49-26.16 tokens/s |
| Sum of reported evaluation time | 857.867 seconds | 3,895.832 seconds |

Method: parse each request's final `prompt eval time` and `eval time` summary, associate the pair by server task ID, retain the responses matched to the project continuation, and calculate medians and percentiles from the server-reported tokens-per-second values. Intermediate progress lines and unfinished generations are excluded. Percentiles use Python's `statistics.quantiles(..., n=10)` default method. The source logs remain private.

“Read” here means prompt evaluation/prefill and “write” means autoregressive generation; neither is disk throughput. Cached input is already present in the model's context and is not all processed again. The **12,556,452 cached input tokens** in this continuation must therefore not be divided by prompt-evaluation time to claim an inflated input rate.

These are observed workload speeds on this local setup, not a fresh synthetic benchmark. They vary with context occupancy, prompt length and caching. The retained timing log covers the final continuation, so these rates are not asserted for every earlier session. Evaluation time also excludes tool execution, user pauses and other agent overhead; it is not the same as the project's active turn time.

No raw conversations, private filesystem paths, machine identifiers or session IDs are included here.

## Failure analysis

| Finding in the saved source | Consequence | Evidence |
| --- | --- | --- |
| A mainloop pointer is cast directly to an API pointer | Invalid FFI/API use; a plausible crash source | `pulse_viz_context_new()` in [shim.c](shim.c); `pa_mainloop_get_api()` in [mainloop.h](pulseheaders/mainloop.h) |
| Connection state is checked immediately, using `3` as the connected state | Asynchronous initialization is not awaited; `3` is the naming stage and ready is `4` | [src/main.rs](src/main.rs), [def.h](pulseheaders/def.h) |
| Streams are created without `pa_stream_connect_record()` | No recording connection is established | `pulse_viz_stream_new()` in [shim.c](shim.c) |
| Source devices are enumerated instead of playback streams; no per-stream monitor is attached | Source-name matching does not implement application filtering | Source enumeration in [shim.c](shim.c), filtering in [src/main.rs](src/main.rs) |
| `pa_stream_peek()` receives the byte buffer where it expects a pointer-to-pointer | Audio is not copied into the Rust buffer; the fragment is dropped | `pulse_viz_stream_read()` in [shim.c](shim.c), [stream.h](pulseheaders/stream.h) |
| PCM decoding assumes incorrect sample-format numbers | Common formats can be decoded incorrectly or reach invalid indexing | `pcm_to_f32()` in [src/audio.rs](src/audio.rs), [sample.h](pulseheaders/sample.h) |
| Renderers append occupied glyphs without padding their x positions; color is reset before the glyph | The intended geometry and colors are not reliably rendered | Renderer functions and `rgb_fg()` in [src/audio.rs](src/audio.rs) |
| Frame rate is interpreted as milliseconds; spectrum FFT size is fixed at 256 | FPS and quality controls do not have the advertised semantics | [src/main.rs](src/main.rs), `render_spectrum()` in [src/audio.rs](src/audio.rs) |

There are also configuration inconsistencies: the former lowercase palette examples do not match the `ColorScheme` enum's TOML representation, parse errors silently select defaults, and the `save()` method is not exposed by the keyboard controls. These are implementation gaps, not changes made during this audit.

### What happened during debugging

Bonsai repeatedly investigated environment and server failures without establishing a correct minimal client. Its shim comments assert a libpulse bug, but the invalid mainloop cast and incomplete connection sequence remain in the saved source. That assertion is not a verified root cause.

The repair session records attempts to stop/restart the desktop audio service, remove the live PulseAudio socket path, and create an ordinary file at the same path using `touch`. The separate recovery session subsequently found a zero-byte regular file where the Unix socket should have been. Connectivity was restored by repairing the endpoint and restarting the relevant service and socket unit.

After recovery, fresh `pactl info` processes connected successfully. Bonsai nevertheless continued pursuing server-side explanations and ad hoc handshake probes. Detailed corrective information was supplied, but the saved client still contains the fundamental API mistakes. Multiple requests for an explanation of the experimental shim variants also did not receive a timely direct answer.

The central failure was the combination of incomplete API understanding, weak end-to-end validation, unsupported diagnosis, and changes to the live environment while debugging a broken client. The initial build and FFT successes were real but insufficient for the task's acceptance criteria.

## What was verified

During the documentation audit on 2026-09-22:

```sh
cargo test --offline --locked
```

All **three FFT tests passed**, with compiler warnings. This verifies only those synthetic FFT cases and the local test build. No live audio capture or interactive success is claimed. Source code, build scripts, lockfiles and bundled headers were left unchanged.

## Prototype layout and interface

| File | Role |
| --- | --- |
| `src/main.rs` | Argument parsing, initialization and frame loop |
| `src/audio.rs` | Buffers, PCM conversion and visualization branches |
| `src/config.rs` | TOML configuration and defaults |
| `src/fft.rs` | FFT and its three tests |
| `src/tui.rs` | Terminal setup, input and rendering |
| `src/ffi.rs`, `shim.c` | Rust/C PulseAudio bridge |
| `build.rs`, `pulseheaders/` | Native build integration and bundled headers |

The parser accepts `--mode` / `-m`, `--source`, and `--help` / `-h`. The intended keys are `1`-`5` for modes, `,` / `.` for sources, and `q`, Escape or Ctrl+C to quit. Configuration is loaded from `~/.config/pulse-viz.toml`. These describe the prototype's interface, not validated working audio functionality.

The build uses Rust/Cargo, a C compiler and a system libpulse runtime. Its hard-coded library location limits portability. Generated targets, temporary files and native library artifacts are ignored by Git; the source and headers are retained for review.
