//! One format registry shared by the Rust API and CLI. Runtime names are explicit.
use crate::{Output, Result, error::invalid};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Container {
    pub name: &'static str,
    pub muxer: &'static str,
    pub extension: &'static str,
}
pub const CONTAINERS: &[Container] = &[
    Container {
        name: "wav",
        muxer: "wav",
        extension: "wav",
    },
    Container {
        name: "mp3",
        muxer: "mp3",
        extension: "mp3",
    },
    Container {
        name: "ogg",
        muxer: "ogg",
        extension: "ogg",
    },
    Container {
        name: "opus",
        muxer: "opus",
        extension: "opus",
    },
    Container {
        name: "speex",
        muxer: "spx",
        extension: "spx",
    },
    Container {
        name: "aac",
        muxer: "adts",
        extension: "aac",
    },
    Container {
        name: "amr",
        muxer: "amr",
        extension: "amr",
    },
    Container {
        name: "avi",
        muxer: "avi",
        extension: "avi",
    },
    Container {
        name: "flac",
        muxer: "flac",
        extension: "flac",
    },
    Container {
        name: "flv",
        muxer: "flv",
        extension: "flv",
    },
    Container {
        name: "m4a",
        muxer: "ipod",
        extension: "m4a",
    },
    Container {
        name: "matroska",
        muxer: "matroska",
        extension: "mka",
    },
    Container {
        name: "mov",
        muxer: "mov",
        extension: "mov",
    },
    Container {
        name: "mp4",
        muxer: "mp4",
        extension: "mp4",
    },
    Container {
        name: "mpeg",
        muxer: "mpeg",
        extension: "mpg",
    },
    Container {
        name: "webm",
        muxer: "webm",
        extension: "webm",
    },
    Container {
        name: "wma",
        muxer: "asf",
        extension: "wma",
    },
    Container {
        name: "aiff",
        muxer: "aiff",
        extension: "aiff",
    },
    Container {
        name: "wavpack",
        muxer: "wv",
        extension: "wv",
    },
    Container {
        name: "ac3",
        muxer: "ac3",
        extension: "ac3",
    },
    Container {
        name: "eac3",
        muxer: "eac3",
        extension: "eac3",
    },
    Container {
        name: "s8",
        muxer: "s8",
        extension: "s8",
    },
    Container {
        name: "s16le",
        muxer: "s16le",
        extension: "s16le",
    },
    Container {
        name: "s16be",
        muxer: "s16be",
        extension: "s16be",
    },
    Container {
        name: "s24le",
        muxer: "s24le",
        extension: "s24le",
    },
    Container {
        name: "s24be",
        muxer: "s24be",
        extension: "s24be",
    },
    Container {
        name: "s32le",
        muxer: "s32le",
        extension: "s32le",
    },
    Container {
        name: "s32be",
        muxer: "s32be",
        extension: "s32be",
    },
    Container {
        name: "f32le",
        muxer: "f32le",
        extension: "f32le",
    },
    Container {
        name: "f32be",
        muxer: "f32be",
        extension: "f32be",
    },
    Container {
        name: "f64le",
        muxer: "f64le",
        extension: "f64le",
    },
    Container {
        name: "f64be",
        muxer: "f64be",
        extension: "f64be",
    },
    Container {
        name: "alaw",
        muxer: "alaw",
        extension: "alaw",
    },
    Container {
        name: "mulaw",
        muxer: "mulaw",
        extension: "mulaw",
    },
];
pub const ENCODERS: &[&str] = &[
    "libopus",
    "libmp3lame",
    "mp2",
    "aac",
    "libvorbis",
    "libspeex",
    "libopencore_amrnb",
    "libvo_amrwbenc",
    "flac",
    "alac",
    "wavpack",
    "ac3",
    "eac3",
    "wmav1",
    "wmav2",
    "adpcm_ms",
    "pcm_s8",
    "pcm_s16le",
    "pcm_s16be",
    "pcm_s24le",
    "pcm_s24be",
    "pcm_s32le",
    "pcm_s32be",
    "pcm_s64le",
    "pcm_s64be",
    "pcm_f32le",
    "pcm_f32be",
    "pcm_f64le",
    "pcm_f64be",
    "pcm_alaw",
    "pcm_mulaw",
];
pub fn container(name: &str) -> Result<&'static Container> {
    CONTAINERS
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| invalid(format!("unknown container: {name}")))
}
pub fn raw(name: &str) -> bool {
    CONTAINERS.iter().skip(21).any(|c| c.name == name)
}
pub fn accepts(name: &str, encoder: &str) -> bool {
    let pcm = encoder.starts_with("pcm_");
    match name {
        "wav" => {
            pcm && encoder.ends_with("le")
                || matches!(
                    encoder,
                    "pcm_alaw" | "pcm_mulaw" | "adpcm_ms" | "libmp3lame" | "mp2" | "ac3"
                )
        }
        "mp3" => encoder == "libmp3lame",
        "ogg" => matches!(encoder, "libopus" | "libvorbis" | "libspeex" | "flac"),
        "opus" => encoder == "libopus",
        "speex" => encoder == "libspeex",
        "aac" => encoder == "aac",
        "amr" => matches!(encoder, "libopencore_amrnb" | "libvo_amrwbenc"),
        "flac" => encoder == "flac",
        "wavpack" => encoder == "wavpack",
        "ac3" => encoder == "ac3",
        "eac3" => encoder == "eac3",
        "webm" => matches!(encoder, "libopus" | "libvorbis"),
        "wma" => matches!(encoder, "wmav1" | "wmav2"),
        "m4a" => matches!(encoder, "aac" | "alac"),
        "mp4" => matches!(
            encoder,
            "aac" | "alac" | "libmp3lame" | "ac3" | "eac3" | "libopus" | "flac"
        ),
        "mov" => pcm || matches!(encoder, "aac" | "alac" | "libmp3lame" | "ac3" | "eac3"),
        "mpeg" => matches!(encoder, "mp2" | "libmp3lame" | "ac3"),
        "flv" => matches!(
            encoder,
            "libmp3lame" | "aac" | "libspeex" | "pcm_s16le" | "pcm_alaw" | "pcm_mulaw"
        ),
        "aiff" => {
            pcm && !matches!(
                encoder,
                "pcm_s24le" | "pcm_s32le" | "pcm_s64le" | "pcm_s64be" | "pcm_f32le" | "pcm_f64le"
            )
        }
        "avi" => matches!(
            encoder,
            "pcm_s16le"
                | "pcm_s24le"
                | "pcm_s32le"
                | "pcm_f32le"
                | "pcm_f64le"
                | "pcm_alaw"
                | "pcm_mulaw"
                | "adpcm_ms"
                | "libmp3lame"
                | "mp2"
                | "ac3"
        ),
        "matroska" => {
            ENCODERS.contains(&encoder)
                && !matches!(
                    encoder,
                    "libopencore_amrnb"
                        | "libvo_amrwbenc"
                        | "libspeex"
                        | "wmav1"
                        | "wmav2"
                        | "adpcm_ms"
                )
        }
        other => raw(other) && encoder == format!("pcm_{other}"),
    }
}
pub fn validate(o: &Output) -> Result<()> {
    container(&o.container)?;
    if o.container == "flv"
        && matches!(o.encoder.as_str(), "pcm_alaw" | "pcm_mulaw")
        && o.sample_rate != 8000
    {
        return Err(invalid("FLV G.711 requires 8000 Hz"));
    }
    if o.container == "flv"
        && !["aac", "libspeex", "pcm_alaw", "pcm_mulaw"].contains(&o.encoder.as_str())
        && ![11025, 22050, 44100].contains(&o.sample_rate)
    {
        return Err(invalid(
            "FLV requires 11025, 22050 or 44100 Hz for this encoder",
        ));
    }
    if !ENCODERS.contains(&o.encoder.as_str()) {
        return Err(invalid("unknown encoder"));
    }
    if !accepts(&o.container, &o.encoder) {
        return Err(invalid("illegal container/encoder pair"));
    }
    if !crate::backend::available(&o.container, &o.encoder) {
        return Err(invalid(
            "container/encoder pair is unavailable in this native build",
        ));
    }
    if o.bitrate.is_some()
        && (o.encoder.starts_with("pcm_")
            || ["flac", "alac", "wavpack", "adpcm_ms"].contains(&o.encoder.as_str()))
    {
        return Err(invalid(
            "bitrate does not apply to PCM/lossless/ADPCM output; use None or --no-bitrate",
        ));
    }
    if o.sample_rate == 0 || o.sample_rate > 768000 || o.channels == 0 || o.channels > 64 {
        return Err(invalid("invalid sample rate or channel count"));
    }
    if o.bitrate == Some(0) || o.bitrate.is_some_and(|b| b > i32::MAX as u32) {
        return Err(invalid("invalid bitrate"));
    }
    if let Some(bitrate) = o.bitrate {
        let rates: &[u32] = match o.encoder.as_str() {
            "libopencore_amrnb" => &[4750, 5150, 5900, 6700, 7400, 7950, 10200, 12200],
            "libvo_amrwbenc" => &[6600, 8850, 12650, 14250, 15850, 18250, 19850, 23050, 23850],
            _ => &[],
        };
        if !rates.is_empty() && !rates.contains(&bitrate) {
            return Err(invalid(format!(
                "{} bitrate must be one of {rates:?}",
                o.encoder
            )));
        }
    }
    if let Some(fmt) = &o.sample_format
        && ![
            "u8", "s16", "s32", "s64", "flt", "dbl", "u8p", "s16p", "s32p", "s64p", "fltp", "dblp",
        ]
        .contains(&fmt.as_str())
    {
        return Err(invalid(
            "sample format must be a libav sample format (24-bit PCM uses s32)",
        ));
    }
    match o.encoder.as_str() {
        "libopus" if ![8000, 12000, 16000, 24000, 48000].contains(&o.sample_rate) => {
            return Err(invalid("Opus requires 8, 12, 16, 24 or 48 kHz"));
        }
        "libopencore_amrnb" if o.sample_rate != 8000 || o.channels != 1 => {
            return Err(invalid("AMR-NB requires 8 kHz mono"));
        }
        "libvo_amrwbenc" if o.sample_rate != 16000 || o.channels != 1 => {
            return Err(invalid("AMR-WB requires 16 kHz mono"));
        }
        "libspeex" if ![8000, 16000, 32000].contains(&o.sample_rate) || o.channels > 2 => {
            return Err(invalid(
                "Speex requires 8, 16 or 32 kHz and at most two channels",
            ));
        }
        _ => {}
    }
    Ok(())
}
