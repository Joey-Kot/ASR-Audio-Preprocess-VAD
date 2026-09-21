use crate::{
    Config, Error, Input, Interval, Mode, Result,
    backend::{AudioInfo, Reader, Writer},
    intervals,
    mapping::scale,
    planner::{self, Plan},
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Event {
    Analysis,
    Planning,
    Export { pass: u32, files: usize },
    Decoding { source_frames: u64 },
    Publishing,
    Diagnostic { message: String },
}
#[derive(Default)]
pub struct Control<'a> {
    pub cancellation: Cancellation,
    pub progress: Option<&'a dyn Fn(Event)>,
}
impl Control<'_> {
    pub(crate) fn check(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    pub(crate) fn emit(&self, event: Event) -> Result<()> {
        self.check()?;
        if let Some(callback) = self.progress {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event)))
                .map_err(|_| Error::Conversion("progress callback panicked".into()))?;
        }
        self.check()
    }
}
#[derive(Debug, Clone)]
struct Fingerprint {
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
    stream: Option<u32>,
    raw: String,
}
impl Fingerprint {
    fn read(input: &Input) -> Result<Self> {
        let metadata = fs::metadata(&input.path)?;
        if !metadata.is_file() {
            return Err(crate::error::invalid(
                "input must be a regular seekable file",
            ));
        }
        Ok(Self {
            path: fs::canonicalize(&input.path)?,
            len: metadata.len(),
            modified: metadata.modified().ok(),
            stream: input.stream,
            raw: serde_json::to_string(&input.raw)
                .map_err(|e| crate::error::invalid(e.to_string()))?,
        })
    }
    fn verify(&self, input: &Input) -> Result<()> {
        let new = Self::read(input)?;
        if self.path != new.path
            || self.len != new.len
            || self.modified != new.modified
            || self.stream != new.stream
            || self.raw != new.raw
        {
            Err(crate::error::invalid("input changed since analysis"))
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub protocol_version: u32,
    pub status: Status,
    pub input: AudioInfo,
    pub speech_intervals: Vec<Interval>,
    pub raw_interval_count: usize,
    pub elapsed_seconds: f64,
    #[serde(skip)]
    fingerprint: Fingerprint,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Completed,
    NoSpeech,
}
#[derive(Debug, Clone, Serialize)]
pub struct TimeMapping {
    pub source: Interval,
    /// Playback coordinates on the output encoder's sample-rate grid.
    pub output: Interval,
}
#[derive(Debug, Clone, Serialize)]
pub struct OutputFile {
    pub path: PathBuf,
    pub source_intervals: Vec<Interval>,
    pub mapping: Vec<TimeMapping>,
    pub hard_cut: bool,
    pub audio: AudioInfo,
    pub container: String,
    pub bytes: u64,
    /// Playback length, including padding not removed by the container/decoder.
    pub duration_seconds: f64,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Timings {
    pub analysis_seconds: f64,
    pub planning_seconds: f64,
    pub export_seconds: f64,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProcessingResult {
    pub protocol_version: u32,
    pub status: Status,
    pub input: AudioInfo,
    pub input_duration_seconds: f64,
    pub speech_duration_seconds: f64,
    pub retained_duration_seconds: f64,
    pub output_duration_seconds: f64,
    pub raw_interval_count: usize,
    pub output_file_count: usize,
    pub files: Vec<OutputFile>,
    pub timings: Timings,
    pub decode_passes: u32,
    /// Additional streaming decodes of encoded outputs for duration validation.
    pub output_validation_decodes: usize,
    pub extra_exports: usize,
}
#[derive(Default)]
pub struct Processor;
struct Pending {
    plan: Plan,
    path: PathBuf,
}
struct Exported {
    pending: Pending,
    info: AudioInfo,
    bytes: u64,
    playback_frames: u64,
    playback_rate: u32,
    validated: bool,
    start_offset: u64,
}

impl Processor {
    pub fn analyze(&self, input: &Input, control: &Control<'_>) -> Result<Analysis> {
        self.analyze_with_config(input, &crate::VadConfig::default(), control)
    }
    /// Analyze with custom VAD settings. Padding is applied only during export.
    pub fn analyze_with_config(
        &self,
        input: &Input,
        vad: &crate::VadConfig,
        control: &Control<'_>,
    ) -> Result<Analysis> {
        vad.validate()?;
        let start = Instant::now();
        let fingerprint = Fingerprint::read(input)?;
        control.emit(Event::Analysis)?;
        let mut reader = Reader::open(input, control).map_err(|e| match e {
            Error::Conversion(m) => Error::Detection(m),
            other => other,
        })?;
        let raw = reader.analyze(vad.start_threshold)?;
        let raw_interval_count = raw.len();
        let speech_intervals =
            intervals::from_analysis(&raw, reader.info.sample_rate, reader.info.frames)?;
        fingerprint.verify(input)?;
        Ok(Analysis {
            protocol_version: 1,
            status: if speech_intervals.is_empty() {
                Status::NoSpeech
            } else {
                Status::Completed
            },
            input: reader.info.clone(),
            speech_intervals,
            raw_interval_count,
            elapsed_seconds: start.elapsed().as_secs_f64(),
            fingerprint,
        })
    }
    pub fn run(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        config.validate()?;
        control.check()?;
        if config.mode == Mode::Preconvert {
            return self.preconvert_full(input, destination, config, control);
        }
        let analysis = self.analyze_with_config(input, &config.vad, control)?;
        self.export_analysis(input, destination, config, &analysis, control)
    }
    pub fn process(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        self.with_mode(input, destination, config, control, Mode::Process)
    }
    pub fn trim(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        self.with_mode(input, destination, config, control, Mode::Trim)
    }
    pub fn split(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        self.with_mode(input, destination, config, control, Mode::Split)
    }
    pub fn preconvert(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        self.with_mode(input, destination, config, control, Mode::Preconvert)
    }
    fn with_mode(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
        mode: Mode,
    ) -> Result<ProcessingResult> {
        let mut config = config.clone();
        config.mode = mode;
        self.run(input, destination, &config, control)
    }
    /// Reuses existing speech intervals; changing the onset threshold requires
    /// calling `analyze_with_config` again before export.
    pub fn export_analysis(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        analysis: &Analysis,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        config.validate()?;
        analysis.fingerprint.verify(input)?;
        control.emit(Event::Planning)?;
        if config.mode == Mode::Preconvert {
            return Err(crate::error::invalid(
                "preconvert does not consume VAD analysis",
            ));
        }
        let start = Instant::now();
        let selected = intervals::pad(
            &analysis.speech_intervals,
            config.vad.padding_ms,
            analysis.input.sample_rate,
            analysis.input.frames,
        )?;
        let plans = planner::plan(
            config,
            &selected,
            analysis.input.sample_rate,
            analysis.input.frames,
        )?;
        let mut result = base_result(analysis.input.clone());
        result.raw_interval_count = analysis.raw_interval_count;
        result.decode_passes = 1;
        result.speech_duration_seconds = duration(
            analysis.speech_intervals.iter().map(|i| i.len()).sum(),
            analysis.input.sample_rate,
        );
        result.timings.analysis_seconds = analysis.elapsed_seconds;
        result.timings.planning_seconds = start.elapsed().as_secs_f64();
        if plans.is_empty() {
            result.status = Status::NoSpeech;
            return Ok(result);
        }
        validate_destination(input, destination, config)?;
        let work = workdir(config)?;
        let mut serial = 0;
        let mut pending: Vec<_> = plans
            .into_iter()
            .map(|plan| {
                serial += 1;
                Pending {
                    plan,
                    path: work.path().join(format!(
                        "{serial}.{}",
                        crate::formats::container(&config.output.container)
                            .unwrap()
                            .extension
                    )),
                }
            })
            .collect();
        let mut done = Vec::new();
        let start = Instant::now();
        for pass in 0..=12 {
            control.emit(Event::Export {
                pass,
                files: pending.len(),
            })?;
            if pass > 0 {
                result.extra_exports += pending.len();
            }
            let exported = export_pass(input, pending, &config.output, control, &analysis.input)?;
            result.decode_passes += 1;
            result.output_validation_decodes += exported.iter().filter(|f| f.validated).count();
            pending = Vec::new();
            for file in exported {
                let too_long = config.mode.multiple()
                    && u128::from(file.playback_frames) * 1000
                        > u128::from(config.max_duration_ms) * u128::from(file.playback_rate);
                if too_long || config.max_bytes.is_some_and(|limit| file.bytes > limit) {
                    if pass == 12 {
                        return Err(Error::Constraint(
                            "duration or byte limit could not be met after 12 subdivision passes"
                                .into(),
                        ));
                    }
                    let (a, b) = planner::bisect(&file.pending.plan)?;
                    fs::remove_file(file.pending.path)?;
                    for plan in [a, b] {
                        serial += 1;
                        pending.push(Pending {
                            plan,
                            path: work.path().join(format!(
                                "{serial}.{}",
                                crate::formats::container(&config.output.container)
                                    .unwrap()
                                    .extension
                            )),
                        });
                    }
                } else {
                    done.push(file);
                }
            }
            if pending.is_empty() {
                break;
            }
        }
        analysis.fingerprint.verify(input)?;
        done.sort_by_key(|f| f.pending.plan.intervals[0].start_frame);
        result.retained_duration_seconds = duration(
            done.iter().map(|f| f.pending.plan.frames()).sum(),
            result.input.sample_rate,
        );
        result.files = publish(
            done,
            input,
            destination,
            config,
            control,
            result.input.sample_rate,
        )?;
        finish_result(&mut result, start);
        Ok(result)
    }
    fn preconvert_full(
        &self,
        input: &Input,
        destination: &Path,
        config: &Config,
        control: &Control<'_>,
    ) -> Result<ProcessingResult> {
        validate_destination(input, destination, config)?;
        let fingerprint = Fingerprint::read(input)?;
        let work = workdir(config)?;
        let start = Instant::now();
        let mut reader = Reader::open(input, control)?;
        let path = work.path().join(format!(
            "output.{}",
            crate::formats::container(&config.output.container)?.extension
        ));
        let mut writer = Writer::open(&reader, &path, &config.output)?;
        control.emit(Event::Export { pass: 0, files: 1 })?;
        while let Some((start, n)) = reader.next()? {
            writer.write(&reader, 0, n)?;
            if start / u64::from(reader.info.sample_rate)
                != (start + n) / u64::from(reader.info.sample_rate)
            {
                control.emit(Event::Decoding {
                    source_frames: start + n,
                })?;
            }
        }
        let (info, delay) = writer.finish()?;
        control.check()?;
        fingerprint.verify(input)?;
        let mut result = base_result(reader.info.clone());
        result.decode_passes = 1;
        let (playback_frames, playback_rate, validated, start_offset) =
            playback(&path, &config.output, &info, delay, control)?;
        result.output_validation_decodes = usize::from(validated);
        result.retained_duration_seconds = result.input_duration_seconds;
        let exported = Exported {
            playback_frames,
            playback_rate,
            validated,
            start_offset,
            bytes: fs::metadata(&path)?.len(),
            info,
            pending: Pending {
                path,
                plan: Plan {
                    intervals: vec![Interval::new(0, result.input.frames)],
                    hard_cut: false,
                },
            },
        };
        result.files = publish(
            vec![exported],
            input,
            destination,
            config,
            control,
            result.input.sample_rate,
        )?;
        finish_result(&mut result, start);
        Ok(result)
    }
}
fn duration(frames: u64, rate: u32) -> f64 {
    frames as f64 / f64::from(rate)
}
fn base_result(input: AudioInfo) -> ProcessingResult {
    ProcessingResult {
        protocol_version: 1,
        status: Status::Completed,
        input_duration_seconds: duration(input.frames, input.sample_rate),
        input,
        speech_duration_seconds: 0.0,
        retained_duration_seconds: 0.0,
        output_duration_seconds: 0.0,
        raw_interval_count: 0,
        output_file_count: 0,
        files: vec![],
        timings: Timings::default(),
        decode_passes: 0,
        output_validation_decodes: 0,
        extra_exports: 0,
    }
}
fn finish_result(result: &mut ProcessingResult, start: Instant) {
    result.output_file_count = result.files.len();
    result.output_duration_seconds = result.files.iter().map(|f| f.duration_seconds).sum();
    result.timings.export_seconds = start.elapsed().as_secs_f64();
}
fn workdir(config: &Config) -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("smartaudio-");
    Ok(match &config.work_dir {
        Some(p) => {
            fs::create_dir_all(p)?;
            builder.tempdir_in(p)?
        }
        None => builder.tempdir()?,
    })
}
fn validate_destination(input: &Input, destination: &Path, config: &Config) -> Result<()> {
    if !config.mode.multiple() {
        if destination.exists() {
            return Err(crate::error::invalid(
                "output already exists; overwriting is disabled",
            ));
        }
        let extension = crate::formats::container(&config.output.container)?.extension;
        if destination.extension().and_then(|s| s.to_str()) != Some(extension) {
            return Err(crate::error::invalid(format!(
                "output extension must be .{extension}"
            )));
        }
    } else if destination.exists() && !destination.is_dir() {
        return Err(crate::error::invalid("output directory is not a directory"));
    }
    if destination == input.path {
        return Err(crate::error::invalid("output cannot replace input"));
    }
    Ok(())
}
fn export_pass(
    input: &Input,
    pending: Vec<Pending>,
    output: &crate::Output,
    control: &Control<'_>,
    expected: &AudioInfo,
) -> Result<Vec<Exported>> {
    let mut reader = Reader::open(input, control)?;
    if reader.info.sample_rate != expected.sample_rate
        || reader.info.channels != expected.channels
        || reader.info.stream != expected.stream
    {
        return Err(Error::Conversion(
            "source format changed after analysis".into(),
        ));
    }
    let mut tasks = pending.into_iter();
    let mut current = tasks.next();
    let mut writer: Option<Writer> = None;
    let mut interval_index = 0;
    let mut result = Vec::new();
    while let Some((start, count)) = reader.next()? {
        let end = start + count;
        while let Some(task) = current.as_ref() {
            let interval = task.plan.intervals[interval_index];
            if interval.start_frame >= end {
                break;
            }
            if interval.end_frame <= start {
                return Err(Error::Conversion("selection fell behind decoding".into()));
            }
            if writer.is_none() {
                writer = Some(Writer::open(&reader, &task.path, output)?);
            }
            let left = start.max(interval.start_frame);
            let right = end.min(interval.end_frame);
            writer
                .as_mut()
                .unwrap()
                .write(&reader, left - start, right - left)?;
            if interval.end_frame > end {
                break;
            }
            interval_index += 1;
            if interval_index == task.plan.intervals.len() {
                let (info, delay) = writer.take().unwrap().finish()?;
                control.check()?;
                let task = current.take().unwrap();
                let bytes = fs::metadata(&task.path)?.len();
                let (playback_frames, playback_rate, validated, start_offset) =
                    playback(&task.path, output, &info, delay, control)?;
                result.push(Exported {
                    playback_frames,
                    playback_rate,
                    validated,
                    start_offset,
                    pending: task,
                    info,
                    bytes,
                });
                current = tasks.next();
                interval_index = 0;
            }
        }
        if start / u64::from(expected.sample_rate) != end / u64::from(expected.sample_rate) {
            control.emit(Event::Decoding { source_frames: end })?;
        }
    }
    if current.is_some() || reader.info.frames != expected.frames {
        return Err(Error::Conversion(
            "source ended at an unexpected frame position".into(),
        ));
    }
    Ok(result)
}
// PCM/lossless and Ogg Opus preserve the valid content grid.
// Other muxers may discard skip/discard metadata; count their decoded samples
// with bounded memory before enforcing hard limits or publishing any output.
fn playback(
    path: &Path,
    output: &crate::Output,
    info: &AudioInfo,
    delay: u64,
    control: &Control<'_>,
) -> Result<(u64, u32, bool, u64)> {
    let exact = output.encoder.starts_with("pcm_")
        || ["flac", "alac", "wavpack"].contains(&output.encoder.as_str())
        || (output.encoder == "libopus" && ["ogg", "opus"].contains(&output.container.as_str()));
    if exact {
        return Ok((info.frames, info.sample_rate, false, 0));
    }
    let mut reader = Reader::open(&Input::new(path), control)?;
    while reader.next()?.is_some() {}
    let skipped = scale(
        reader.start_skip(),
        info.sample_rate,
        reader.info.sample_rate,
        false,
    )?;
    // WMA priming and the first Vorbis overlap window are discarded internally,
    // without packet skip side data. Neither is audible leading padding.
    let start_offset = if ["wmav1", "wmav2", "libvorbis"].contains(&output.encoder.as_str()) {
        0
    } else {
        delay.saturating_sub(skipped)
    };
    let content_end = u128::from(info.frames) + u128::from(start_offset);
    if content_end * u128::from(reader.info.sample_rate)
        > (u128::from(reader.info.frames) + 1) * u128::from(info.sample_rate)
    {
        return Err(Error::Conversion(
            "encoded playback does not contain the complete mapped content".into(),
        ));
    }
    Ok((
        reader.info.frames,
        reader.info.sample_rate,
        true,
        start_offset,
    ))
}
fn publish(
    done: Vec<Exported>,
    input: &Input,
    destination: &Path,
    config: &Config,
    control: &Control<'_>,
    source_rate: u32,
) -> Result<Vec<OutputFile>> {
    control.emit(Event::Publishing)?;
    let directory = if config.mode.multiple() {
        destination
    } else {
        destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    };
    fs::create_dir_all(directory)?;
    let extension = crate::formats::container(&config.output.container)?.extension;
    let mut published: Vec<PathBuf> = Vec::new();
    let attempt = (|| -> Result<Vec<OutputFile>> {
        let mut result = Vec::new();
        for (index, file) in done.into_iter().enumerate() {
            control.check()?;
            let target = if config.mode.multiple() {
                directory.join(format!("{:06}.{extension}", index + 1))
            } else {
                destination.to_path_buf()
            };
            if target == input.path || target.exists() {
                return Err(crate::error::invalid(format!(
                    "output already exists: {}",
                    target.display()
                )));
            }
            let mut staged = tempfile::NamedTempFile::new_in(directory)?;
            use std::io::{Read, Write};
            let mut original = fs::File::open(&file.pending.path)?;
            let mut buffer = [0; 65536];
            loop {
                control.check()?;
                let n = original.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                staged.write_all(&buffer[..n])?;
            }
            staged.as_file().sync_all()?;
            control.check()?;
            staged
                .persist_noclobber(&target)
                .map_err(|e| Error::Io(e.error.to_string()))?;
            published.push(target.clone());
            let mut cursor = 0;
            let mut mapping = Vec::new();
            for source in &file.pending.plan.intervals {
                let start = scale(cursor, file.info.sample_rate, source_rate, false)?
                    .checked_add(file.start_offset)
                    .ok_or_else(|| Error::Constraint("mapping overflow".into()))?;
                cursor += source.len();
                let end = scale(cursor, file.info.sample_rate, source_rate, false)?
                    .min(file.info.frames)
                    .checked_add(file.start_offset)
                    .ok_or_else(|| Error::Constraint("mapping overflow".into()))?;
                mapping.push(TimeMapping {
                    source: *source,
                    output: Interval::new(start, end),
                });
            }
            if let Some(last) = mapping.last_mut() {
                last.output.end_frame = file
                    .info
                    .frames
                    .checked_add(file.start_offset)
                    .ok_or_else(|| Error::Constraint("mapping overflow".into()))?;
            }
            result.push(OutputFile {
                path: target,
                source_intervals: file.pending.plan.intervals,
                mapping,
                hard_cut: file.pending.plan.hard_cut,
                duration_seconds: duration(file.playback_frames, file.playback_rate),
                audio: file.info,
                container: config.output.container.clone(),
                bytes: file.bytes,
            });
        }
        control.check()?;
        Ok(result)
    })();
    if let Err(original_error) = &attempt {
        let mut cleanup_errors = Vec::new();
        for path in published {
            if let Err(error) = fs::remove_file(&path) {
                cleanup_errors.push(format!("{}: {error}", path.display()));
            }
        }
        if !cleanup_errors.is_empty() {
            return Err(Error::Io(format!(
                "{}; rollback errors: {}",
                original_error,
                cleanup_errors.join("; ")
            )));
        }
    }
    attempt
}

pub fn native_version() -> String {
    crate::backend::version()
}
pub fn format_available(container: &str, encoder: &str) -> bool {
    crate::backend::available(container, encoder)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_samples_splitting_mapping_and_byte_retries() {
        let t = tempfile::tempdir().unwrap();
        let input_path = t.path().join("input.wav");
        let mut writer = hound::WavWriter::create(
            &input_path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        let mut samples = Vec::new();
        for i in 0..44100u32 {
            for ch in 0..2 {
                let s = (i * 13 + ch * 17001) as i16;
                writer.write_sample(s).unwrap();
                samples.push(s);
            }
        }
        writer.finalize().unwrap();
        let input = Input::new(input_path);
        let control = Control::default();
        let reader = Reader::open(&input, &control).unwrap();
        let mut info = reader.info.clone();
        info.frames = 44100;
        drop(reader);
        let analysis = Analysis {
            protocol_version: 1,
            status: Status::Completed,
            input: info,
            speech_intervals: vec![
                Interval::new(101, 5101),
                Interval::new(10003, 22000),
                Interval::new(40000, 44100),
            ],
            raw_interval_count: 3,
            elapsed_seconds: 0.0,
            fingerprint: Fingerprint::read(&input).unwrap(),
        };
        for max_bytes in [None, Some(9000)] {
            let config = Config {
                max_duration_ms: 200,
                max_bytes,
                vad: crate::VadConfig {
                    padding_ms: 0,
                    ..Default::default()
                },
                output: crate::Output {
                    container: "wav".into(),
                    encoder: "pcm_s16le".into(),
                    sample_rate: 44100,
                    channels: 2,
                    bitrate: None,
                    ..crate::Output::default()
                },
                ..Config::default()
            };
            let out = t.path().join(if max_bytes.is_some() {
                "limited"
            } else {
                "normal"
            });
            let result = Processor
                .export_analysis(&input, &out, &config, &analysis, &control)
                .unwrap();
            assert_eq!(
                result.decode_passes,
                if max_bytes.is_some() { 4 } else { 2 }
            );
            let mut actual = Vec::new();
            for file in &result.files {
                assert!(file.audio.frames <= 8820);
                if let Some(limit) = max_bytes {
                    assert!(file.bytes <= limit);
                }
                actual.extend(
                    hound::WavReader::open(&file.path)
                        .unwrap()
                        .samples::<i16>()
                        .map(|s| s.unwrap()),
                );
                assert_eq!(
                    file.mapping.last().unwrap().output.end_frame,
                    file.audio.frames
                );
            }
            let expected: Vec<_> = analysis
                .speech_intervals
                .iter()
                .flat_map(|i| {
                    samples[i.start_frame as usize * 2..i.end_frame as usize * 2]
                        .iter()
                        .copied()
                })
                .collect();
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn publish_conflict_rolls_back_only_our_outputs() {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("out");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("000002.ogg"), b"user file").unwrap();
        let control = Control::default();
        let mut files = Vec::new();
        for i in 0..2 {
            let path = t.path().join(format!("tmp{i}"));
            fs::write(&path, b"data").unwrap();
            files.push(Exported {
                playback_frames: 10,
                playback_rate: 16000,
                validated: false,
                start_offset: 0,
                pending: Pending {
                    path,
                    plan: Plan {
                        intervals: vec![Interval::new(0, 10)],
                        hard_cut: false,
                    },
                },
                info: AudioInfo {
                    sample_rate: 16000,
                    channels: 1,
                    stream: 0,
                    frames: 10,
                    bitrate: Some(32000),
                    bits_per_sample: None,
                    codec: "libopus".into(),
                    sample_format: "s16".into(),
                },
                bytes: 4,
            });
        }
        assert!(
            publish(
                files,
                &Input::new("unused"),
                &dest,
                &Config::default(),
                &control,
                16000
            )
            .is_err()
        );
        assert!(!dest.join("000001.ogg").exists());
        assert_eq!(fs::read(dest.join("000002.ogg")).unwrap(), b"user file");
    }

    #[test]
    fn impossible_byte_limit_fails_without_outputs() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("one.wav");
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
        w.write_sample(1000i16).unwrap();
        w.finalize().unwrap();
        let input = Input::new(source);
        let control = Control::default();
        let mut info = Reader::open(&input, &control).unwrap().info.clone();
        info.frames = 1;
        let analysis = Analysis {
            protocol_version: 1,
            status: Status::Completed,
            input: info,
            speech_intervals: vec![Interval::new(0, 1)],
            raw_interval_count: 1,
            elapsed_seconds: 0.0,
            fingerprint: Fingerprint::read(&input).unwrap(),
        };
        let config = Config {
            max_bytes: Some(1),
            work_dir: Some(t.path().join("work")),
            ..Config::default()
        };
        let destination = t.path().join("outputs");
        assert!(matches!(
            Processor.export_analysis(&input, &destination, &config, &analysis, &control),
            Err(Error::Constraint(_))
        ));
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(t.path().join("work")).unwrap().count(), 0);
    }
}
