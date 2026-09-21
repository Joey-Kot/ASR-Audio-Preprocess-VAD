use smartaudio::{Config, Mode, PausePolicy};
#[test]
fn vad_threshold_defaults_and_validation() {
    let mut config = Config::default();
    assert_eq!(config.vad.start_threshold, 0.6);
    let legacy: smartaudio::VadConfig = serde_json::from_str(r#"{"padding_ms":100}"#).unwrap();
    assert_eq!(legacy.start_threshold, 0.6);
    for threshold in [0.5, 0.6, 0.9, 1.0] {
        config.vad.start_threshold = threshold;
        config.validate().unwrap();
    }
    for threshold in [0.49, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        config.vad.start_threshold = threshold;
        assert!(matches!(
            config.validate(),
            Err(smartaudio::Error::InvalidConfig(_))
        ));
        assert!(matches!(
            smartaudio::Processor.analyze_with_config(
                &smartaudio::Input::new("missing.wav"),
                &config.vad,
                &smartaudio::Control::default()
            ),
            Err(smartaudio::Error::InvalidConfig(_))
        ));
    }
}
#[test]
fn defaults_padding_and_constraints() {
    let mut c = Config::default();
    assert_eq!(c.max_duration_ms, 175000);
    assert_eq!(c.vad.padding_ms, 100);
    assert_eq!(c.output.container, "ogg");
    assert_eq!(c.output.encoder, "libopus");
    assert_eq!(c.output.bitrate, Some(32000));
    assert_eq!(c.output.sample_rate, 16000);
    assert_eq!(c.output.channels, 1);
    for padding in [0, 1, 999, 1000] {
        c.vad.padding_ms = padding;
        c.validate().unwrap();
    }
    c.vad.padding_ms = 1001;
    assert!(c.validate().is_err());
    c.vad.padding_ms = 100;
    c.max_duration_ms = 0;
    assert!(c.validate().is_err());
    c.max_duration_ms = 175000;
    c.max_bytes = Some(0);
    assert!(c.validate().is_err());
    c.max_bytes = None;
    c.output.sample_format = Some("impossible".into());
    assert!(c.validate().is_err());
    c.output.sample_format = None;
    c.mode = Mode::Trim;
    c.pauses = PausePolicy::Keep;
    assert!(c.validate().is_err());
}

#[test]
fn amr_rejects_rates_that_native_encoders_would_round() {
    for (encoder, sample_rate, legal) in [
        (
            "libopencore_amrnb",
            8000,
            &[4750, 5150, 5900, 6700, 7400, 7950, 10200, 12200][..],
        ),
        (
            "libvo_amrwbenc",
            16000,
            &[6600, 8850, 12650, 14250, 15850, 18250, 19850, 23050, 23850][..],
        ),
    ] {
        let mut config = Config::default();
        config.output.container = "amr".into();
        config.output.encoder = encoder.into();
        config.output.sample_rate = sample_rate;
        for bitrate in legal {
            config.output.bitrate = Some(*bitrate);
            config.validate().unwrap();
        }
        config.output.bitrate = None;
        config.validate().unwrap();
        for bitrate in [1, 4500, 12345, 32000] {
            config.output.bitrate = Some(bitrate);
            assert!(
                matches!(config.validate(), Err(smartaudio::Error::InvalidConfig(_))),
                "{encoder}/{bitrate}"
            );
        }
    }
}
