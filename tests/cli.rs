use std::process::Command;
#[test]
fn configurable_vad_threshold() {
    let bin = env!("CARGO_BIN_EXE_smartaudio");
    for mode in ["analyze", "process", "trim", "split"] {
        let mut cmd = Command::new(bin);
        cmd.args([
            mode,
            "--input",
            "missing.wav",
            "--vad-start-threshold",
            "NaN",
        ]);
        if mode != "analyze" {
            cmd.args(["--output", "unused-output"]);
        }
        let result = cmd.output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        let json: serde_json::Value = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(json["error"]["kind"], "invalid_config");
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap()
                .contains("threshold")
        );
    }
    let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jfk.flac");
    let result = Command::new(bin)
        .args(["analyze", "--vad-start-threshold", "0.7", "--input"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cli: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let analysis = smartaudio::Processor
        .analyze_with_config(
            &smartaudio::Input::new(&input),
            &smartaudio::VadConfig {
                start_threshold: 0.7,
                ..Default::default()
            },
            &smartaudio::Control::default(),
        )
        .unwrap();
    assert_eq!(
        cli["speech_intervals"],
        serde_json::to_value(analysis.speech_intervals).unwrap()
    );
}
#[test]
fn help_version_errors_and_json() {
    let bin = env!("CARGO_BIN_EXE_smartaudio");
    for arg in ["--help", "--version"] {
        let r = Command::new(bin).arg(arg).output().unwrap();
        assert!(r.status.success());
        assert!(!r.stdout.is_empty());
    }
    let error = Command::new(bin)
        .args([
            "trim",
            "--input",
            "missing.wav",
            "--output",
            "out.ogg",
            "--vad-padding",
            "1001ms",
        ])
        .output()
        .unwrap();
    assert_eq!(error.status.code(), Some(2));
    assert!(error.stdout.is_empty());
    let parsed: serde_json::Value = serde_json::from_slice(&error.stderr).unwrap();
    assert_eq!(parsed["error"]["kind"], "invalid_config");
    let list = Command::new(bin).arg("formats").output().unwrap();
    assert!(list.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(parsed["ffmpeg"], "8.1.2");
    assert!(!parsed["pairs"].as_array().unwrap().is_empty());
}

#[test]
fn converts_with_no_external_programs_on_path() {
    let t = tempfile::tempdir().unwrap();
    let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jfk.flac");
    let output = t.path().join("speech.ogg");
    let result = Command::new(env!("CARGO_BIN_EXE_smartaudio"))
        .env("PATH", t.path())
        .arg("preconvert")
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(json["status"], "completed");
    assert!(output.exists());
}

#[test]
fn unsupported_amr_bitrate_is_a_json_configuration_error() {
    let t = tempfile::tempdir().unwrap();
    let output = t.path().join("bad.amr");
    let r = Command::new(env!("CARGO_BIN_EXE_smartaudio"))
        .args([
            "preconvert",
            "--input",
            "missing.wav",
            "--container",
            "amr",
            "--encoder",
            "libopencore_amrnb",
            "--sample-rate",
            "8000",
            "--bitrate",
            "32000",
            "--output",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&r.stderr).unwrap();
    assert_eq!(json["error"]["kind"], "invalid_config");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("bitrate")
    );
    assert!(!output.exists());
}
