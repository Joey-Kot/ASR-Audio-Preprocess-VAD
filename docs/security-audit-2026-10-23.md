# Dependency security audit — 2026-10-23

Baseline: `b6a3d881a0c4aab9eaa5192b974e1ea4f6489762`.

This is a historical audit of the 2026-10-23 build. Current builds use
unmodified FFmpeg 9.0.2; the local backport, application script and
patch-specific checks described below have been removed.

## Result and updates

The enabled MOV demuxer exposes **CVE-2026-107678** when opening a crafted
MP4/M4A/MOV file. The original FFmpeg 8.1.2 and the unmodified 8.1.3 release
both recursively free encryption-init linked lists and repeatedly deserialize
and serialize accumulated `pssh` records. Audio-only decoding does not prevent
the demuxer from parsing this metadata.

Updated FFmpeg to **8.1.3 plus the vendored PR 24593 backport** and Rust from
**1.98.1 to 1.99.1**. Updated the toolchain, Cargo requirement, build-time version
assertion, CI, runtime FFmpeg version check and CLI test together. No Rust crate
version change was required; `Cargo.lock` is unchanged.

## Rust dependency scan

`cargo-audit` 0.22.2 scanned all 48 dependencies in `Cargo.lock`, including build,
dev and platform-specific dependencies without a target filter. Result:
**zero vulnerabilities and zero warnings**, including the yanked-crate check.

RustSec database: 1,349 advisories, commit
`640bf5a9f06e66d8721e8772766531fe77c8630d`, updated 2026-10-22 10:49:47 UTC.
Earshot remains pinned to 1.2.2. Rust 1.99.1 also fixes an x86_64 FFI `bool`
return-value regression; see the
[official release announcement](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/).

## Native-library triage

Queried NVD's FFmpeg 8.1.2 CPE/version candidates (35 records), recent FFmpeg
keyword records (2026-07-01 through 2026-10-23), and all seven external audio libraries.
Checked the generated `config.h` / `config_components.h`, upstream fixes, and
the relevant source paths. Keyword results for applications that merely call
FFmpeg were excluded after checking their product identity.

| Finding | Result for this build |
|---|---|
| CVE-2026-107678 | Applicable; fixed by the local three-commit backport. |
| CVE-2026-96611, MOV/HEIF dimension overflow | The cited fix is already in 8.1.2 and 8.1.3; the upstream patch reverse-checks against 8.1.3. |
| CVE-2026-52296, WMA extradata padding | WMA encoders are enabled, but the padding fix is already present in both releases; the upstream patch reverse-checks against 8.1.3. |
| CVE-2026-52295, CVE-2026-52297, CVE-2026-66037 | IAMF and its MOV parsing/writing paths are disabled (`CONFIG_IAMF`, `CONFIG_IAMFDEC`, `CONFIG_IAMFENC` are 0). |
| CVE-2025-25468, CVE-2025-25469 | The referenced `d5873be` IAMF failure-path cleanup is present; IAMF is disabled. The NVD description is broader than the referenced change. |
| CVE-2026-107660, CVE-2026-107675, CVE-2026-107677, CVE-2026-107695–107698 | TLS/SFTP/DASH/HLS/RTSP paths are disabled. |
| CVE-2026-107676, HDR10+ serialization | The Matroska muxer invokes the serializer only for video streams. This application creates audio output streams and re-encodes PCM; it does not remux video/HDR packets. |
| CVE-2026-64830–64835, CVE-2026-65703–65706, CVE-2026-66036, CVE-2026-66038–66041, CVE-2026-70628–70632 | Their subtitle, hardware/video filter/decoder, S/PDIF, RTP/ASF, ADX, MACE6, TY or Shorten components are disabled. Enabling concat alone does not enable TY/Shorten. |
| CVE-2025-52290, H.264 reorder-frame handling | The demuxer call is guarded by `CONFIG_H264_DECODER`, which is 0. |
| Opus 1.5.2 | No applicable CVE found; opusfile, PJSIP and other products' Opus integration defects do not describe this library. |

External libraries retained after version and feature checks:

| Library | Version | Finding |
|---|---|---|
| Opus | 1.5.2 | No applicable CVE found. |
| libogg | 1.3.5 | No applicable CVE found; liboggplay/liboggz are separate libraries. |
| libvorbis | 1.3.7 | Includes the known 2017/2018 fixes; confirmed against upstream 1.3.7 release notes. |
| LAME | 3.100 | Reviewed CVEs affect older versions or disabled frontend/decoder paths. Only the encoder is enabled here. |
| Speex | 1.2.1 | Known older library issue is fixed; WAV CLI issues affect the disabled `speexenc` frontend. |
| OpenCore AMR | 0.1.6 | No applicable CVE found under `opencore-amr`/`OpenCORE`; the old OpenCORE MP3 decoder issue is outside this AMR library. |
| vo-amrwbenc | 0.1.3 | No applicable published CVE identified under `vo-amrwbenc`/`AMR-WB`. |

NVD keyword queries were cross-checked with product identity, version ranges,
source configuration, upstream release notes and the Debian source-package
trackers for [libvorbis](https://security-tracker.debian.org/tracker/source-package/libvorbis),
[LAME](https://security-tracker.debian.org/tracker/source-package/lame) and
[Speex](https://security-tracker.debian.org/tracker/source-package/speex).
Newer feature releases alone did not justify a dependency upgrade for this CVE task.

The upgrade also includes upstream fixes in enabled Ogg, MOV, WavPack and
audio conversion paths listed in the 8.1.3 release `Changelog`.

## Source integrity and backport

Archive: [ffmpeg-8.1.3.tar.gz](https://ffmpeg.org/releases/ffmpeg-8.1.3.tar.gz).

SHA-256: `bd458826a039b48a9606e794554c75eb4c4984b84173f7afa3128eae89336f2b`.

The archive's detached signature was verified against the pinned release-key
fingerprint `FCF986EA15E6E293A5644F10B4322F04D67658D8`. The new FFmpeg checksum is pinned in `scripts/audio-sources.sha256`;
the other seven archive hashes are unchanged.

Vendored patch: `scripts/patches/ffmpeg-8.1.3-cve-2026-107678.patch`.
It backports all three commits from
[upstream PR 24593](https://code.ffmpeg.org/FFmpeg/FFmpeg/pulls/24593):

- `969c7eb091989a664cb7bf66a1f02051cc46cb52`: iterative linked-list free.
- `d1434518fbd03789e3e5dc0657a7a9f1123ce074`: serialized append API and unit test.
- `71403a21b02a92b925e26e609e9bd3e48df451cf`: amortized linear MOV `pssh` append.

The master-branch API version bump and APIchanges entry are omitted for this
private static 8.1.3 build. The implementation is unchanged; the unit test adds
a 100,000-node free case. Both build entry points apply and verify the patch,
accept a fully patched source tree on rebuild, and reject partial/mismatched
sources. CI also tests these three cases and the MOV regression.

## Validation performed

Linux x86_64, Rust/cargo 1.99.1:

- Built all eight native dependencies from pinned archives.
- `make fate-encryption-info`: passed, including upstream append and long-list tests.
- MOV regression: 32,768 + 7 `pssh` records across two tracks; values, counts and order verified.
- Patch application tests: fresh apply, repeat apply, and fail-without-mutation on partial patches passed.
- `cargo fmt --all --check` and `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: 33 tests passed, including the 117 container/encoder pair matrix.
- Independent Rust consumer test, `cargo package --locked --allow-dirty`, and release build: passed.
- Workflow checked with actionlint; shell syntax and Git whitespace checks passed.

Windows/macOS/ARM64 builds require the repository CI; they were not executed
locally. These results cover the pinned application dependencies and configured
native features, using the advisory database snapshots above.

Primary references: [CVE-2026-107678](https://nvd.nist.gov/vuln/detail/CVE-2026-107678),
[MOV dimension fix](https://code.ffmpeg.org/FFmpeg/FFmpeg/commit/059eb2e853c1f62b8df1b73be9318061a7a21b0b),
[WMA padding fix](https://code.ffmpeg.org/FFmpeg/FFmpeg/commit/23227a444de4a8f7696f46660cdd044b460f7e47),
[Opus 1.5.2](https://opus-codec.org/release/stable/2024/04/12/libopus-1_5_2.html).
