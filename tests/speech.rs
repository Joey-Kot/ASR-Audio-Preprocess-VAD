use smartaudio::{Config, Control, Input, Mode, Output, Processor, Status};
fn fixture() -> Input {
    Input::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jfk.flac"))
}

#[test]
fn webm_opus_short_chunks_obey_cap_or_fail_without_publishing() {
    let t = tempfile::tempdir().unwrap();
    let input = fixture();
    let control = Control::default();
    let mut analysis = Processor.analyze(&input, &control).unwrap();
    // Isolate a short piece of the public speech to exercise codec frame limits.
    let start = analysis.speech_intervals[0].start_frame;
    let frames = u64::from(analysis.input.sample_rate) / 50;
    analysis.speech_intervals = vec![smartaudio::Interval::new(start, start + frames)];
    analysis.raw_interval_count = 1;
    let mut config = Config {
        max_duration_ms: 15,
        vad: smartaudio::VadConfig {
            padding_ms: 0,
            ..Default::default()
        },
        output: Output {
            container: "webm".into(),
            sample_rate: 48000,
            ..Output::default()
        },
        ..Config::default()
    };
    let r = Processor
        .export_analysis(
            &input,
            &t.path().join("parts"),
            &config,
            &analysis,
            &control,
        )
        .unwrap();
    assert_eq!(r.output_validation_decodes, r.output_file_count);
    assert_eq!(
        r.files
            .iter()
            .flat_map(|f| &f.source_intervals)
            .map(|i| i.len())
            .sum::<u64>(),
        frames
    );
    let pcm = Config {
        mode: Mode::Preconvert,
        output: Output {
            container: "wav".into(),
            encoder: "pcm_s16le".into(),
            sample_rate: 48000,
            bitrate: None,
            ..Output::default()
        },
        ..Config::default()
    };
    for (n, file) in r.files.iter().enumerate() {
        let decoded = Processor
            .run(
                &Input::new(&file.path),
                &t.path().join(format!("{n}.wav")),
                &pcm,
                &control,
            )
            .unwrap();
        assert_eq!(file.duration_seconds, decoded.input_duration_seconds);
        assert!(file.duration_seconds <= 0.015);
    }
    config.max_duration_ms = 10;
    analysis.speech_intervals[0].end_frame = start + 2;
    let destination = t.path().join("impossible");
    assert!(matches!(
        Processor.export_analysis(&input, &destination, &config, &analysis, &control),
        Err(smartaudio::Error::Constraint(_))
    ));
    assert!(!destination.exists());
}
#[test]
fn compressed_chunks_obey_decoded_duration_cap() {
    let t = tempfile::tempdir().unwrap();
    let input = fixture();
    let control = Control::default();
    let analysis = Processor.analyze(&input, &control).unwrap();
    for (container, encoder, rate, bitrate) in [
        ("aac", "aac", 48000, 64000),
        ("wma", "wmav2", 44100, 64000),
        ("ogg", "libspeex", 16000, 24000),
    ] {
        let config = Config {
            max_duration_ms: 1000,
            output: Output {
                container: container.into(),
                encoder: encoder.into(),
                sample_rate: rate,
                bitrate: Some(bitrate),
                ..Output::default()
            },
            ..Config::default()
        };
        let result = Processor
            .export_analysis(
                &input,
                &t.path().join(encoder),
                &config,
                &analysis,
                &control,
            )
            .unwrap();
        assert!(result.output_validation_decodes >= result.output_file_count);
        assert!(result.extra_exports > 0);
        for (i, file) in result.files.iter().enumerate() {
            let pcm = Config {
                mode: Mode::Preconvert,
                output: Output {
                    container: "wav".into(),
                    encoder: "pcm_s16le".into(),
                    sample_rate: rate,
                    bitrate: None,
                    ..Output::default()
                },
                ..Config::default()
            };
            let decoded = Processor
                .run(
                    &Input::new(&file.path),
                    &t.path().join(format!("{encoder}-{i}.wav")),
                    &pcm,
                    &control,
                )
                .unwrap();
            assert!(decoded.input_duration_seconds <= 1.0);
            assert_eq!(file.duration_seconds, decoded.input_duration_seconds);
        }
    }
}
#[test]
fn public_speech_four_modes_reuse_and_quiet_voice() {
    let t = tempfile::tempdir().unwrap();
    let input = fixture();
    let control = Control::default();
    let analysis = Processor.analyze(&input, &control).unwrap();
    assert!(analysis.raw_interval_count > 0);
    let speech = analysis
        .speech_intervals
        .iter()
        .map(|i| i.len())
        .sum::<u64>() as f64
        / f64::from(analysis.input.sample_rate);
    assert!(speech > 5.0, "too little public speech retained: {speech}");
    for mode in [Mode::Process, Mode::Trim, Mode::Split, Mode::Preconvert] {
        let config = Config {
            mode,
            max_duration_ms: 3000,
            ..Config::default()
        };
        let out = t.path().join(format!("{mode:?}.ogg"));
        let result = if mode == Mode::Preconvert {
            Processor.run(&input, &out, &config, &control)
        } else {
            Processor.export_analysis(&input, &out, &config, &analysis, &control)
        }
        .unwrap();
        assert_eq!(result.status, Status::Completed);
        assert!(!result.files.is_empty());
        if mode.multiple() {
            assert!(result.files.iter().all(|f| f.duration_seconds <= 3.001));
        }
        for file in result.files {
            assert!(file.bytes > 0);
            assert_eq!(
                file.mapping.last().unwrap().output.end_frame,
                file.audio.frames
            );
        }
    }
    // Decode public speech once, then create a quiet version with deterministic gain.
    let decoded = t.path().join("decoded.wav");
    let pcm = Config {
        mode: Mode::Preconvert,
        output: Output {
            container: "wav".into(),
            encoder: "pcm_s16le".into(),
            sample_rate: 16000,
            channels: 1,
            bitrate: None,
            ..Output::default()
        },
        ..Config::default()
    };
    Processor.run(&input, &decoded, &pcm, &control).unwrap();
    let quiet = t.path().join("quiet.wav");
    let mut reader = hound::WavReader::open(decoded).unwrap();
    let mut writer = hound::WavWriter::create(&quiet, reader.spec()).unwrap();
    for s in reader.samples::<i16>() {
        writer.write_sample(s.unwrap() / 8).unwrap();
    }
    writer.finalize().unwrap();
    assert!(
        !Processor
            .analyze(&Input::new(quiet), &control)
            .unwrap()
            .speech_intervals
            .is_empty()
    );
}
#[test]
fn cancellation_during_analysis_and_export_cleans_files() {
    let t = tempfile::tempdir().unwrap();
    let token = smartaudio::Cancellation::default();
    let events = |event| {
        if matches!(event, smartaudio::Event::Decoding { .. }) {
            token.cancel();
        }
    };
    let control = Control {
        cancellation: token.clone(),
        progress: Some(&events),
    };
    assert!(matches!(
        Processor.analyze(&fixture(), &control),
        Err(smartaudio::Error::Cancelled)
    ));
    let token = smartaudio::Cancellation::default();
    let events = |event| {
        if matches!(event, smartaudio::Event::Decoding { .. }) {
            token.cancel();
        }
    };
    let control = Control {
        cancellation: token.clone(),
        progress: Some(&events),
    };
    let out = t.path().join("cancel.ogg");
    let config = Config {
        mode: Mode::Preconvert,
        work_dir: Some(t.path().join("work")),
        ..Config::default()
    };
    assert!(matches!(
        Processor.run(&fixture(), &out, &config, &control),
        Err(smartaudio::Error::Cancelled)
    ));
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(t.path().join("work")).unwrap().count(), 0);
}

#[test]
fn independent_concurrent_tasks_and_repeated_detector_state() {
    let handles: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                let a = Processor.analyze(&fixture(), &Control::default()).unwrap();
                let b = Processor.analyze(&fixture(), &Control::default()).unwrap();
                assert_eq!(a.speech_intervals, b.speech_intervals);
                a.speech_intervals
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.iter().all(|r| r == &results[0]));
}
