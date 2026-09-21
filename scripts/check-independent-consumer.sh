#!/usr/bin/env sh
set -eu
ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
mkdir -p "$ROOT_DIR/consumer-test-tmp"
TASK_DIR="$(mktemp -d "$ROOT_DIR/consumer-test-tmp/check.XXXXXX")"
trap 'rm -rf "$TASK_DIR"' EXIT HUP INT TERM
mkdir -p "$TASK_DIR/src"
cat > "$TASK_DIR/Cargo.toml" <<EOF
[package]
name = "smartaudio-consumer-validation"
version = "0.0.0"
edition = "2024"
[dependencies]
smartaudio = { path = "../.." }
EOF
cat > "$TASK_DIR/src/main.rs" <<'EOF'
use smartaudio::{Control,Input,Processor};
fn main() {
    let input=Input::new(std::env::args_os().nth(1).unwrap());
    let analysis=Processor.analyze(&input,&Control::default()).unwrap();
    assert!(!analysis.speech_intervals.is_empty());
}
EOF
cargo run --offline --manifest-path "$TASK_DIR/Cargo.toml" -- "$ROOT_DIR/tests/fixtures/jfk.flac"
