use smartaudio::{Config, Control, Input, Mode, Output, Processor, Status};
use std::path::Path;

#[test]
fn delayed_encoders_preserve_bursts_at_first_and_last_frame_boundaries() {
    let t = tempfile::tempdir().unwrap();
    for (encoder, container, rate, frame, burst, bitrate) in [
        ("libspeex", "ogg", 16000, 320, 64, 24000),
        ("wmav1", "wma", 44100, 2048, 441, 64000),
        ("wmav2", "wma", 44100, 2048, 441, 64000),
    ] {
        for (delta, at_start) in [-1i32, 0, 1]
            .into_iter()
            .flat_map(|d| [(d, false), (d, true)])
        {
            let frames = (frame * 10 + delta) as u32;
            let source = t.path().join(format!("{encoder}-{delta}-{at_start}.wav"));
            let mut writer = hound::WavWriter::create(
                &source,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for i in 0..frames {
                let sample = if (at_start && i < burst) || (!at_start && i >= frames - burst) {
                    (20000.0
                        * (f64::from(i) * 2.0 * std::f64::consts::PI * 440.0 / f64::from(rate))
                            .sin()) as i16
                } else {
                    0
                };
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
            let encoded = t
                .path()
                .join(format!("{encoder}-{delta}-{at_start}.{container}"));
            let config = Config {
                mode: Mode::Preconvert,
                output: Output {
                    encoder: encoder.into(),
                    container: container.into(),
                    sample_rate: rate,
                    channels: 1,
                    bitrate: Some(bitrate),
                    ..Output::default()
                },
                ..Config::default()
            };
            Processor
                .run(&Input::new(source), &encoded, &config, &Control::default())
                .unwrap();
            let decoded = t
                .path()
                .join(format!("decoded-{encoder}-{delta}-{at_start}.wav"));
            let pcm = Config {
                output: Output {
                    sample_rate: rate,
                    channels: 1,
                    ..pcm_config().output
                },
                ..pcm_config()
            };
            Processor
                .run(&Input::new(encoded), &decoded, &pcm, &Control::default())
                .unwrap();
            let mut reader = hound::WavReader::open(decoded).unwrap();
            assert!(
                reader.duration() >= frames,
                "{encoder}/{delta}: missing tail samples"
            );
            let peak = reader
                .samples::<i16>()
                .map(|s| i32::from(s.unwrap()).abs())
                .max()
                .unwrap();
            assert!(
                peak > 1000,
                "{encoder}/{delta}/{at_start}: boundary burst disappeared (peak {peak})"
            );
        }
    }
}

fn fixture(path: &Path, rate: u32, channels: u16, frames: u32) {
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
    for i in 0..frames {
        for ch in 0..channels {
            w.write_sample(((i * 17 + u32::from(ch) * 7001) % 60001) as i16)
                .unwrap();
        }
    }
    w.finalize().unwrap();
}
fn pcm_config() -> Config {
    Config {
        mode: Mode::Preconvert,
        output: Output {
            container: "wav".into(),
            encoder: "pcm_s16le".into(),
            sample_rate: 44100,
            channels: 2,
            sample_format: Some("s16".into()),
            bitrate: None,
            threads: 1,
        },
        ..Config::default()
    }
}
#[test]
fn exact_original_samples_and_independent_channels() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source.wav");
    let out = t.path().join("out.wav");
    fixture(&source, 44100, 2, 44117);
    let result = Processor
        .run(
            &Input::new(&source),
            &out,
            &pcm_config(),
            &Control::default(),
        )
        .unwrap();
    assert_eq!(result.status, Status::Completed);
    assert_eq!(result.decode_passes, 1);
    assert_eq!(result.files[0].audio.frames, 44117);
    let read = |p: &Path| {
        hound::WavReader::open(p)
            .unwrap()
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(read(&source), read(&out));
}
#[test]
fn silence_no_speech_and_no_output_for_all_vad_modes() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("silence.wav");
    let mut w = hound::WavWriter::create(
        &source,
        hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..8101 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
    for mode in [Mode::Process, Mode::Trim, Mode::Split] {
        let out = t.path().join(format!("{mode:?}.ogg"));
        let c = Config {
            mode,
            ..Config::default()
        };
        let r = Processor
            .run(&Input::new(&source), &out, &c, &Control::default())
            .unwrap();
        assert_eq!(r.status, Status::NoSpeech);
        assert_eq!(r.output_file_count, 0);
        assert_eq!(r.input.frames, 8101);
        assert!(!out.exists());
    }
}

#[test]
fn deterministic_noise_is_not_speech() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("noise.wav");
    let mut w = hound::WavWriter::create(
        &source,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    let mut seed = 0x19283746u32;
    for _ in 0..32000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        w.write_sample((seed as i16) / 4).unwrap();
    }
    w.finalize().unwrap();
    let result = Processor
        .analyze(&Input::new(source), &Control::default())
        .unwrap();
    assert_eq!(result.status, Status::NoSpeech);
}
#[test]
fn conflict_cancel_corruption_and_invalid_format() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source.wav");
    fixture(&source, 44100, 2, 4000);
    let config = pcm_config();
    let input = Input::new(&source);
    let control = Control::default();
    assert!(Processor.run(&input, &source, &config, &control).is_err());
    let bad = t.path().join("bad.wav");
    std::fs::write(&bad, b"invalid audio").unwrap();
    assert!(
        Processor
            .run(
                &Input::new(bad),
                &t.path().join("error.wav"),
                &config,
                &control
            )
            .is_err()
    );
    assert!(!t.path().join("error.wav").exists());
    control.cancellation.cancel();
    assert!(matches!(
        Processor.run(&input, &t.path().join("cancel.wav"), &config, &control),
        Err(smartaudio::Error::Cancelled)
    ));
    let mut config = config;
    config.output.sample_format = Some("fltp".into());
    assert!(
        Processor
            .run(
                &input,
                &t.path().join("invalid.wav"),
                &config,
                &Control::default()
            )
            .is_err()
    );
    assert!(!t.path().join("invalid.wav").exists());
}
#[test]
fn opus_roundtrip_and_rate_conversion() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source.wav");
    fixture(&source, 44100, 2, 88201);
    let output = t.path().join("out.ogg");
    let config = Config {
        mode: Mode::Preconvert,
        ..Config::default()
    };
    let result = Processor
        .run(&Input::new(source), &output, &config, &Control::default())
        .unwrap();
    assert_eq!(result.files[0].audio.sample_rate, 16000);
    assert_eq!(result.files[0].audio.channels, 1);
    let decoded = t.path().join("decoded.wav");
    let c = Config {
        output: Output {
            sample_rate: 48000,
            channels: 1,
            ..pcm_config().output
        },
        ..pcm_config()
    };
    let r = Processor
        .run(&Input::new(output), &decoded, &c, &Control::default())
        .unwrap();
    assert!((r.input.frames as i64 - 96003).abs() <= 4);
}
