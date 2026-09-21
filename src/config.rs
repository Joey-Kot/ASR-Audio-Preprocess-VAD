use crate::{Result, error::invalid, formats};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Process,
    Trim,
    Split,
    Preconvert,
}
impl Mode {
    pub fn multiple(self) -> bool {
        matches!(self, Self::Process | Self::Split)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum PausePolicy {
    #[default]
    ModeDefault,
    Keep,
    Trim,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadConfig {
    pub padding_ms: u32,
    /// Speech onset score; must be finite and within 0.5..=1.0.
    #[serde(default = "default_start_threshold")]
    pub start_threshold: f32,
}
fn default_start_threshold() -> f32 {
    0.6
}
impl Default for VadConfig {
    fn default() -> Self {
        Self {
            padding_ms: 100,
            start_threshold: default_start_threshold(),
        }
    }
}
impl VadConfig {
    pub fn validate(&self) -> Result<()> {
        if self.padding_ms > 1000 {
            return Err(invalid("vad padding must be 0..=1000 ms"));
        }
        if !self.start_threshold.is_finite() || !(0.5..=1.0).contains(&self.start_threshold) {
            return Err(invalid(
                "vad start threshold must be finite and within 0.5..=1.0",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawInput {
    pub format: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Input {
    pub path: PathBuf,
    pub stream: Option<u32>,
    pub raw: Option<RawInput>,
}
impl Input {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            stream: None,
            raw: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub container: String,
    pub encoder: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// None uses the encoder's natural sample format; explicit values are strict.
    pub sample_format: Option<String>,
    pub bitrate: Option<u32>,
    pub threads: u16,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            container: "ogg".into(),
            encoder: "libopus".into(),
            sample_rate: 16000,
            channels: 1,
            sample_format: None,
            bitrate: Some(32000),
            threads: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub mode: Mode,
    pub vad: VadConfig,
    pub output: Output,
    pub pauses: PausePolicy,
    pub max_duration_ms: u64,
    pub max_bytes: Option<u64>,
    pub work_dir: Option<PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Process,
            vad: VadConfig::default(),
            output: Output::default(),
            pauses: PausePolicy::ModeDefault,
            max_duration_ms: 175000,
            max_bytes: None,
            work_dir: None,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        self.vad.validate()?;
        if self.max_duration_ms == 0 {
            return Err(invalid("maximum duration must be positive"));
        }
        if self.max_bytes == Some(0) {
            return Err(invalid("maximum bytes must be positive"));
        }
        if !self.mode.multiple()
            && (self.max_bytes.is_some() || self.pauses != PausePolicy::ModeDefault)
        {
            return Err(invalid(
                "byte limits and pause policy require process or split",
            ));
        }
        formats::validate(&self.output)
    }
    pub fn keep_pauses(&self) -> bool {
        self.pauses == PausePolicy::Keep
            || (self.pauses == PausePolicy::ModeDefault && self.mode == Mode::Split)
    }
}
