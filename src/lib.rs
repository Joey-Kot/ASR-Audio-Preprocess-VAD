//! Synchronous audio preprocessing. Analysis PCM is never used for export.
mod backend;
pub mod config;
pub mod error;
pub mod formats;
pub mod intervals;
pub mod mapping;
pub mod planner;
pub mod processor;
mod vad;

pub use config::{Config, Input, Mode, Output, PausePolicy, VadConfig};
pub use error::{Error, Result};
pub use intervals::Interval;
pub use processor::{Analysis, Cancellation, Control, Event, ProcessingResult, Processor, Status};
