//! Generated samples only: resampler startup, codec frames and duration limits.
use smartaudio::{Config, Control, Input, Mode, Output, Processor};
use std::path::Path;

fn source(path: &Path, rate: u32, channels: u16, frames: u32) {
    let mut w = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..frames {
        let value = (14000.0 * (0.7 + f64::from(n) * 0.05).sin()) as i16;
        for ch in 0..channels {
            w.write_sample(if ch == 0 { value } else { -value })
                .unwrap();
        }
    }
    w.finalize().unwrap();
}
fn pcm(rate: u32, channels: u16) -> Config {
    Config {
        mode: Mode::Preconvert,
        output: Output {
            container: "wav".into(),
            encoder: "pcm_s16le".into(),
            sample_rate: rate,
            channels,
            bitrate: None,
            ..Output::default()
        },
        ..Config::default()
    }
}
#[test]
fn very_short_resampling_keeps_the_source_duration_and_signal() {
    let t = tempfile::tempdir().unwrap();
    for (input_rate, output_rate) in [(48000, 16000), (44100, 48000), (48000, 8000)] {
        for channels in [1, 2] {
            for frames in [1, 8, 16, 32, 48, 80, 128, 256, 1000] {
                let name = format!("{input_rate}-{output_rate}-{channels}-{frames}");
                let input = t.path().join(format!("{name}.wav"));
                source(&input, input_rate, channels, frames);
                let output = t.path().join(format!("{name}-out.wav"));
                let r = Processor
                    .run(
                        &Input::new(input),
                        &output,
                        &pcm(output_rate, channels),
                        &Control::default(),
                    )
                    .unwrap();
                let expected =
                    (u64::from(frames) * u64::from(output_rate)).div_ceil(u64::from(input_rate));
                let mut reader = hound::WavReader::open(output).unwrap();
                assert_eq!(u64::from(reader.duration()), expected, "{name}");
                assert_eq!(r.files[0].audio.frames, expected, "{name}");
                assert!(
                    reader.samples::<i16>().any(|s| s.unwrap() != 0),
                    "{name}: lost signal"
                );
            }
        }
    }
}

#[test]
fn compressed_boundaries_report_decoded_length_and_bound_mappings() {
    let t = tempfile::tempdir().unwrap();
    for (container, encoder, extension) in [
        ("ogg", "libopus", "ogg"),
        ("webm", "libopus", "webm"),
        ("matroska", "libopus", "mka"),
        ("ogg", "libvorbis", "ogg"),
        ("webm", "libvorbis", "webm"),
        ("matroska", "libvorbis", "mka"),
        ("mp3", "libmp3lame", "mp3"),
        ("matroska", "libmp3lame", "mka"),
    ] {
        for frames in [1, 16, 128, 256, 480, 512, 1024, 48000] {
            let name = format!("{container}-{encoder}-{frames}");
            let input = t.path().join(format!("{name}.wav"));
            source(&input, 48000, 1, frames);
            let output = t.path().join(format!("{name}.{extension}"));
            let config = Config {
                mode: Mode::Preconvert,
                output: Output {
                    container: container.into(),
                    encoder: encoder.into(),
                    sample_rate: 48000,
                    bitrate: Some(64000),
                    ..Output::default()
                },
                ..Config::default()
            };
            let r = Processor
                .run(&Input::new(input), &output, &config, &Control::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let d = Processor
                .run(
                    &Input::new(output),
                    &t.path().join(format!("{name}-decoded.wav")),
                    &pcm(48000, 1),
                    &Control::default(),
                )
                .unwrap();
            assert_eq!(
                r.output_duration_seconds, d.input_duration_seconds,
                "{name}"
            );
            let mapped = r.files[0].mapping.last().unwrap().output;
            assert_eq!(mapped.len(), u64::from(frames), "{name}");
            assert!(mapped.end_frame <= d.input.frames, "{name}");
            assert!(d.input.frames > 0, "{name}");
        }
    }
}

#[test]
fn short_resampled_opus_is_playable_and_amr_defaults_are_actual_rates() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("short.wav");
    source(&input, 48000, 1, 48);
    let output = t.path().join("short.ogg");
    let config = Config {
        mode: Mode::Preconvert,
        ..Config::default()
    };
    let r = Processor
        .run(&Input::new(&input), &output, &config, &Control::default())
        .unwrap();
    assert_eq!(r.files[0].audio.frames, 16);
    let d = Processor
        .run(
            &Input::new(output),
            &t.path().join("decoded.wav"),
            &pcm(48000, 1),
            &Control::default(),
        )
        .unwrap();
    assert_eq!(d.input.frames, 48);
    source(&input, 48000, 1, 4800);
    for (encoder, rate, bitrate, header, mode) in [
        ("libopencore_amrnb", 8000, 12200, b"#!AMR\n".as_slice(), 7),
        ("libvo_amrwbenc", 16000, 23850, b"#!AMR-WB\n".as_slice(), 8),
    ] {
        let output = t.path().join(format!("{encoder}.amr"));
        let config = Config {
            mode: Mode::Preconvert,
            output: Output {
                container: "amr".into(),
                encoder: encoder.into(),
                sample_rate: rate,
                bitrate: None,
                ..Output::default()
            },
            ..Config::default()
        };
        let r = Processor
            .run(&Input::new(&input), &output, &config, &Control::default())
            .unwrap();
        assert_eq!(r.files[0].audio.bitrate, Some(bitrate));
        let bytes = std::fs::read(output).unwrap();
        assert!(bytes.starts_with(header));
        assert_eq!((bytes[header.len()] >> 3) & 15, mode);
    }
}
