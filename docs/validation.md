# Rust rewrite validation

This file records checks actually run; CI definitions alone are not evidence of
successful target builds. No formal release has been made.

## Security update — 2026-10-23

Rust/cargo **1.99.1**, FFmpeg **8.1.3 plus PR 24593**, Earshot **1.2.2**.
Linux x86_64: rebuilt all static dependencies, passed fmt/clippy, 33 tests
(including 117 container/encoder pairs), the independent consumer check, Cargo
package verification and the release build. FFmpeg encryption-info, long-list
free, MOV `pssh` and patch-application regressions also passed.
The 48-dependency RustSec scan found zero vulnerabilities or warnings.
See the [security audit](security-audit-2026-10-23.md) for sources and scope.
Windows/macOS/ARM64 validation remains the responsibility of CI.

## Original rewrite environment

- Rust/cargo **1.98.1**, Earshot **1.2.2**, FFmpeg **8.1.2**.
- Linux x86_64, static libav and external audio libraries.
- Native dependencies built through `scripts/bootstrap-static-audio-deps.sh`;
  both bootstrap and direct FFmpeg builds use the same component list.
- No system `ffmpeg`/`ffprobe` subprocess is used by the library or CLI.

## Checks

- Deterministic stereo PCM: original sample values, channel independence,
  selected intervals, concatenation, output mapping, duration and byte limits.
- Padding: zero, odd sample allocation, boundary clamping, overlap, large
  interval lists, overflow and 44.1 kHz rounding.
- Cancellation before/during decoding, corrupt input, invalid sample format,
  output conflicts and rollback preserving existing user files.
- Public speech: four modes, reusable analysis and a reduced-gain variant.
- 117 compiled container/encoder pairs: actual encoding and decoding using
  generated PCM and appropriate codec parameters. This is not an exhaustive
  test of every bitrate/rate/channel/sample-format permutation.
- The S64BE encoder is compiled, but none of the requested FFmpeg 8.1.2 muxers
  advertises a valid pairing. No S64BE output pair or fictitious raw muxer is
  exposed. S64LE/WAV is included in the verified matrix.
- Independent Rust consumer project compiled and called `Processor::analyze`.
- Cargo package verification and local binary/corresponding-source packaging
  succeeded; source archives include vendored Rust dependencies and checksummed
  native dependency archives. Package contents were inspected to exclude local
  working directories and private recordings.
- Packaging was repeated with an isolated Cargo cache containing only Linux
  dependencies. The new fetch step downloaded the missing Windows crates and
  offline vendoring plus both archive builds completed successfully.
- `ldd` shows only platform runtime libraries (libc, libm, libmvec, libgcc_s),
  not shared FFmpeg or external audio codecs.
- 29 test functions passed, including the 117-pair matrix, a CLI conversion with
  an otherwise empty PATH, deterministic noise, and an unsatisfiable byte cap.

## Private manual checks

The two user-provided recordings were used **only for this local refactor**.
They are ignored by Git, excluded from the Cargo package, and not referenced
by automated tests, CI or release scripts.

- Reading: decoded duration 188.650667 s, 63 speech intervals, detected speech
  132.8 s. Default `process`: 139.2 s retained including padding, one output.
  Default `split`: two outputs, 173.83 s and 8.134 s; two decode passes.
  `trim`: one 139.2 s output, two passes. `preconvert`: one 188.6506875 s
  output, one pass (the final resampling rounds to the output sample grid).
- Rain: decoded duration 3599.9935 s, zero speech intervals, `no_speech`, no
  exported files. One analysis pass, about 13 s on this machine.
  The final diagnostic run took 11.47 s with a 6640 KiB address-space peak RSS
  (`/proc/self/status` VmHWM); no full analysis PCM or temporary audio was stored.
- The unmodified reference threshold misclassified rain. Internal onset
  confirmation in that validation used three consecutive scores >= 0.9; continuation uses
  0.5, a bounded six-frame onset history, four voiced frames minimum, and
  ten-frame end hysteresis. Frame counts and continuation remain internal constants;
  the onset threshold is now configurable through `VadConfig.start_threshold`.
  This test does not establish accuracy for all speakers/noises.
- The current onset threshold is **0.6** (three consecutive frames), lowered
  after reports of ordinary speech being clipped. Continuation remains 0.5.
  The recording/rain results above were measured at 0.9 and have not been
  revalidated at 0.6. A score-boundary regression test covers the new threshold.

## Platform completion gate

| Target | Local build/run verification |
|---|---|
| Linux x86_64 | Verified |
| Linux ARM64 | Not yet run |
| Windows x86_64 | Not yet run |
| Windows ARM64 | Not yet run |

The final deletion of the Go source is gated on successful target builds and
runtime tests. At the user's request, the old Go module, CLI, tests and cgo shim
have been moved to `Example/Go-Legacy/` for local reference. `/Example/` is
ignored by Git and excluded from Rust packages and release archives. The old
implementation is not a Rust runtime fallback.

## Compressed duration and alignment checks

The 117-pair matrix now compares reported playback duration against an
independent decode within one sample. Only PCM/lossless and Ogg/Opus use direct
sample accounting; other formats are stream-decoded before publication, and
oversized duration plans
are subdivided with the same bounded retry path as oversized byte counts.
These output checks are counted separately from original-input decode passes.
AAC, WMA and Speex public-speech tests verify a strict one-second cap against
independently decoded files.

Review regressions also cover short WebM/Opus chunks (a 15 ms cap succeeds with
accurate durations; an impossible 10 ms cap fails before publication), and
1/16/128/256/480/512/1024/48000-sample Opus, MP3 and Vorbis outputs. Ogg/Vorbis
keeps its first audio packets on early pages so the initial granule is available
even for short recordings; subsequent pages use the normal page duration.

Resampling tracks the selected source frames and rounds their duration up to
the output sample grid. Bounded zero extension completes the filter startup/tail
without counting extension as content. Generated mono/stereo inputs of 1–1000
frames cover downsampling and upsampling; a 48-frame 48 kHz source produces
16 valid 16 kHz frames and a playable Opus file, rather than an empty output.

AMR-NB/WB reject unsupported explicit bitrates in both the API and CLI. Legal
discrete rates are checked; unspecified rates select 12200/23850 bps. Tests
compare returned bitrate metadata with the actual AMR frame mode.

Encoders without delayed flush, and Speex (whose flush only closes packets),
now receive lookahead silence without counting it as valid source content.
Speex and WMA v1/v2 tests retain generated opening and terminal bursts at
frame-size-minus-one, exact-frame and frame-size-plus-one boundaries. WMA input
is primed with its extra transform block to compensate for the decoder's
two-block skip; Speex accounting includes the decoder's 5 ms lookahead.

`audio.frames` describes valid resampled content. Mappings include remaining
leading delay after decoder skip metadata, on the encoder sample-rate grid.
The format matrix checks mapping bounds and correlates a generated chirp with
an independently resampled reference, allowing 2 ms for lossy filter phase
effects. PCM remains sample-exact. This covers the matrix's documented rates
and codec settings, not every possible parameter permutation or every player's
handling of gapless metadata. No additional original-input pass or repeated
VAD is introduced by output duration verification.
