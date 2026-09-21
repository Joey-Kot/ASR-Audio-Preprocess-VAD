# Third-party notices

SmartAudio is GPL-3.0-or-later. Copyright (C) 2026 Joey Kot.
The native sample-copy approach and initial Earshot state-machine design were
informed by STT-for-Windows, by the same author under GPL-3.0-or-later.

## Statically linked audio libraries

| Component | Version | License text |
|---|---|---|
| FFmpeg | 8.1.2 | `THIRD_PARTY_LICENSES/FFmpeg-GPL-3.0-or-later.txt` |
| Opus | 1.5.2 | `THIRD_PARTY_LICENSES/Opus-BSD-3-Clause.txt` |
| LAME | 3.100 | `THIRD_PARTY_LICENSES/LAME-LGPL-2.0-only.txt` |
| libogg | 1.3.5 | `THIRD_PARTY_LICENSES/Xiph-BSD-3-Clause.txt` |
| libvorbis | 1.3.7 | `THIRD_PARTY_LICENSES/Xiph-BSD-3-Clause.txt` |
| OpenCore AMR | 0.1.6 | `THIRD_PARTY_LICENSES/opencore-amr-Apache-2.0.txt` |
| Speex | 1.2.1 | `THIRD_PARTY_LICENSES/Speex-BSD-3-Clause.txt` |
| vo-amrwbenc | 0.1.3 | `THIRD_PARTY_LICENSES/vo-amrwbenc-Apache-2.0.txt` |

Source download URLs are in `scripts/bootstrap-static-audio-deps.sh` and archive
SHA-256 digests in `scripts/audio-sources.sha256`. The FFmpeg build enables GPL
and version 3 and disables nonfree, libavfilter and network access. No PortAudio
or alternative VAD backend is linked.

## Earshot and Rust dependencies

Earshot **1.2.2** is licensed **MIT OR Apache-2.0**, with its detector weights
included in the crate. Source: https://github.com/pykeio/earshot and
https://crates.io/crates/earshot/1.2.2.
License texts are in `THIRD_PARTY_LICENSES/Earshot-MIT.txt` and
`THIRD_PARTY_LICENSES/Earshot-Apache-2.0.txt`, retrieved from the crate's recorded
source commit `c400f15bd232dda30a297eec90235bd09066cab7`.

Other direct dependencies are clap, serde, serde_json, thiserror, tempfile,
libc (Unix), cc and pkg-config; hound is used by tests. Exact transitive versions
are recorded in `Cargo.lock`. Native source versions and checksums are recorded
in `scripts/audio-sources.sha256`, with download URLs and build instructions in
`scripts/bootstrap-static-audio-deps.sh`. Binary archives include license notices;
the release packaging script does not generate separate source archives.

## Test fixture

The public-domain JFK speech excerpt comes from the MIT-licensed Whisper test
suite. Provenance, pinned commit, checksum and upstream license are retained in
`tests/fixtures/`. Private local test recordings are not redistributed.
