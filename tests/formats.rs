//! Generated PCM only. No private/manual reference audio is used here.
use smartaudio::{Config, Control, Input, Mode, Output, Processor, config::RawInput, formats};

#[test]
fn every_registered_compiled_pair_encodes_and_decodes() {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source.wav");
    let mut w = hound::WavWriter::create(
        &source,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..96013 {
        let time = f64::from(n) / 48000.0;
        let phase = 2.0 * std::f64::consts::PI * (300.0 * time + 700.0 * time * time);
        w.write_sample((16000.0 * phase.sin()) as i16).unwrap();
    }
    w.finalize().unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut covered_encoders = std::collections::BTreeSet::new();
    let mut covered_containers = std::collections::BTreeSet::new();
    let mut references = std::collections::BTreeMap::new();
    for container in formats::CONTAINERS {
        for &encoder in formats::ENCODERS {
            if !formats::accepts(container.name, encoder) {
                continue;
            }
            if !smartaudio::processor::format_available(container.name, encoder) {
                continue;
            }
            checked += 1;
            covered_encoders.insert(encoder);
            covered_containers.insert(container.name);
            let (rate, bitrate) = match encoder {
                "libopencore_amrnb" => (8000, Some(12200)),
                "libvo_amrwbenc" => (16000, Some(23850)),
                "libspeex" => (16000, Some(24000)),
                "libopus" => (48000, Some(32000)),
                "libvorbis" | "wmav1" | "wmav2" => (44100, Some(64000)),
                e if e.starts_with("pcm_")
                    || ["flac", "alac", "wavpack", "adpcm_ms"].contains(&e) =>
                {
                    (48000, None)
                }
                _ => (48000, Some(128000)),
            };
            let mut rate = rate;
            if container.name == "flv" && encoder != "libspeex" && encoder != "aac" {
                rate = 44100;
            }
            if container.name == "flv" && ["pcm_alaw", "pcm_mulaw"].contains(&encoder) {
                rate = 8000;
            }
            let name = format!("{}-{encoder}", container.name);
            let path = t.path().join(format!("{name}.{}", container.extension));
            let config = Config {
                mode: Mode::Preconvert,
                output: Output {
                    container: container.name.into(),
                    encoder: encoder.into(),
                    sample_rate: rate,
                    channels: 1,
                    bitrate,
                    ..Output::default()
                },
                ..Config::default()
            };
            let encoded = Processor.run(&Input::new(&source), &path, &config, &Control::default());
            if let Err(error) = encoded {
                failures.push(format!("{name}: encode: {error}"));
                continue;
            }
            let mut input = Input::new(&path);
            if formats::raw(container.name) {
                input.raw = Some(RawInput {
                    format: container.name.into(),
                    sample_rate: rate,
                    channels: 1,
                });
            }
            let decoded = t.path().join(format!("decoded-{name}.wav"));
            let decode_config = Config {
                mode: Mode::Preconvert,
                output: Output {
                    container: "wav".into(),
                    encoder: "pcm_s16le".into(),
                    sample_rate: rate,
                    channels: 1,
                    bitrate: None,
                    ..Output::default()
                },
                ..Config::default()
            };
            let reference = references.entry(rate).or_insert_with(|| {
                let reference_path = t.path().join(format!("reference-{rate}.wav"));
                Processor
                    .run(
                        &Input::new(&source),
                        &reference_path,
                        &decode_config,
                        &Control::default(),
                    )
                    .unwrap();
                hound::WavReader::open(reference_path)
                    .unwrap()
                    .samples::<i16>()
                    .map(|s| f64::from(s.unwrap()))
                    .collect::<Vec<_>>()
            });
            match Processor.run(&input, &decoded, &decode_config, &Control::default()) {
                Err(error) => failures.push(format!("{name}: decode: {error}")),
                Ok(result) => {
                    let reported = encoded.as_ref().unwrap().output_duration_seconds;
                    let file = &encoded.as_ref().unwrap().files[0];
                    let end = file.mapping.last().unwrap().output.end_frame as f64
                        / f64::from(file.audio.sample_rate);
                    if end > result.input_duration_seconds + 1.0 / f64::from(rate) {
                        failures.push(format!(
                            "{name}: mapping ends at {end}, playback ends at {}",
                            result.input_duration_seconds
                        ));
                    }
                    if (reported - result.input_duration_seconds).abs() > 1.0 / f64::from(rate) {
                        failures.push(format!(
                            "{name}: reported {reported}, decoded {}",
                            result.input_duration_seconds
                        ));
                    }
                    let samples: Vec<_> = hound::WavReader::open(&decoded)
                        .unwrap()
                        .samples::<i16>()
                        .map(|s| f64::from(s.unwrap()))
                        .collect();
                    let expected = file.mapping[0].output.start_frame as i64;
                    let mut best = (f64::NEG_INFINITY, 0);
                    for lag in -(i64::from(rate) / 20)..=i64::from(rate) / 20 {
                        let mut dot = 0.0;
                        let mut energy = 0.0;
                        for i in (rate as usize / 5..rate as usize * 2 / 5).step_by(17) {
                            let sample = samples[(i as i64 + lag) as usize];
                            dot += reference[i] * sample;
                            energy += sample * sample;
                        }
                        let score = dot / energy.max(1.0).sqrt();
                        if score > best.0 {
                            best = (score, lag);
                        }
                    }
                    // Speech codecs' filtering can shift a chirp correlation
                    // peak slightly; the mapping itself uses codec delay metadata.
                    if (best.1 - expected).abs() > i64::from(rate) / 500 {
                        failures.push(format!(
                            "{name}: mapped delay {expected}, measured chirp delay {}",
                            best.1
                        ));
                    }
                    if (result.input_duration_seconds - 2.00027).abs() > 0.25 {
                        failures.push(format!(
                            "{name}: invalid decoded duration {}",
                            result.input_duration_seconds
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(
        checked, 117,
        "the complete native build must expose all validated pairs"
    );
    let unavailable: Vec<_> = formats::ENCODERS
        .iter()
        .copied()
        .filter(|e| !covered_encoders.contains(e))
        .collect();
    // FFmpeg has this encoder, but none of the requested muxers accepts S64BE.
    assert_eq!(unavailable, ["pcm_s64be"]);
    assert_eq!(
        covered_containers.len(),
        formats::CONTAINERS.len(),
        "missing container coverage"
    );
    assert!(
        failures.is_empty(),
        "{checked} pairs checked, {} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
