use clap::{Parser, Subcommand};
use smartaudio::{
    Config, Control, Input, Mode, Output, PausePolicy, Processor, VadConfig, config::RawInput,
};
use std::path::PathBuf;
mod signals;

#[derive(Parser)]
#[command(version, about = "Earshot audio preprocessing with embedded libav")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Process(Options),
    Trim(Options),
    Split(Options),
    Preconvert(Options),
    Analyze(InputOptions),
    /// List compiled container/encoder pairs. Final audio parameters are checked at export.
    Formats,
}
#[derive(clap::Args)]
struct InputOptions {
    /// VAD onset score (0.5..=1.0); ignored by preconvert.
    #[arg(long, default_value_t = 0.6)]
    vad_start_threshold: f32,
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    input_stream: Option<u32>,
    #[arg(long,requires_all=["input_rate","input_channels"])]
    input_format: Option<String>,
    #[arg(long, requires = "input_format")]
    input_rate: Option<u32>,
    #[arg(long, requires = "input_format")]
    input_channels: Option<u16>,
}
impl InputOptions {
    fn value(self) -> Input {
        Input {
            path: self.input,
            stream: self.input_stream,
            raw: self.input_format.map(|format| RawInput {
                format,
                sample_rate: self.input_rate.unwrap(),
                channels: self.input_channels.unwrap(),
            }),
        }
    }
}
#[derive(clap::Args)]
struct Options {
    #[command(flatten)]
    input: InputOptions,
    /// Single output file for trim/preconvert; output directory for process/split.
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    work_dir: Option<PathBuf>,
    #[arg(long,default_value="100ms",value_parser=milliseconds)]
    vad_padding: u64,
    #[arg(long,default_value="175s",value_parser=milliseconds)]
    max_duration: u64,
    #[arg(long)]
    max_bytes: Option<u64>,
    #[arg(long, value_enum, default_value = "mode-default")]
    pauses: PausePolicy,
    #[arg(long, default_value = "ogg")]
    container: String,
    #[arg(long, default_value = "libopus")]
    encoder: String,
    #[arg(long, default_value_t = 16000)]
    sample_rate: u32,
    #[arg(long, default_value_t = 1)]
    channels: u16,
    #[arg(long)]
    sample_format: Option<String>,
    #[arg(long,default_value="32000",value_parser=bitrate)]
    bitrate: u32,
    #[arg(long)]
    no_bitrate: bool,
    #[arg(long, default_value_t = 1)]
    native_threads: u16,
}
fn milliseconds(text: &str) -> Result<u64, String> {
    let (value, multiplier) = if let Some(v) = text.strip_suffix("ms") {
        (v, 1)
    } else if let Some(v) = text.strip_suffix('s') {
        (v, 1000)
    } else {
        return Err("duration needs ms or s suffix".into());
    };
    value
        .parse::<u64>()
        .ok()
        .and_then(|v| v.checked_mul(multiplier))
        .ok_or_else(|| "invalid duration".into())
}
fn bitrate(text: &str) -> Result<u32, String> {
    let (v, m) = text.strip_suffix('k').map_or((text, 1), |v| (v, 1000));
    v.parse::<u32>()
        .ok()
        .and_then(|v| v.checked_mul(m))
        .ok_or_else(|| "invalid bitrate".into())
}
fn run(command: Command) -> smartaudio::Result<serde_json::Value> {
    let processor = Processor;
    let control = Control::default();
    signals::install(control.cancellation.clone())?;
    let (options, mode) = match command {
        Command::Analyze(input) => {
            let vad = VadConfig {
                start_threshold: input.vad_start_threshold,
                ..Default::default()
            };
            return serde_json::to_value(processor.analyze_with_config(
                &input.value(),
                &vad,
                &control,
            )?)
            .map_err(|e| smartaudio::Error::Io(e.to_string()));
        }
        Command::Formats => {
            return Ok(
                serde_json::json!({"ffmpeg":smartaudio::processor::native_version(),"pairs":
            smartaudio::formats::CONTAINERS.iter().flat_map(|c|smartaudio::formats::ENCODERS.iter().filter_map(move |e|
                smartaudio::processor::format_available(c.name,e).then_some(serde_json::json!({"container":c.name,"muxer":c.muxer,"extension":c.extension,"encoder":e})))).collect::<Vec<_>>()}),
            );
        }
        Command::Process(o) => (o, Mode::Process),
        Command::Trim(o) => (o, Mode::Trim),
        Command::Split(o) => (o, Mode::Split),
        Command::Preconvert(o) => (o, Mode::Preconvert),
    };
    let config = Config {
        mode,
        vad: VadConfig {
            start_threshold: options.input.vad_start_threshold,
            padding_ms: u32::try_from(options.vad_padding)
                .map_err(|_| smartaudio::Error::InvalidConfig("padding overflow".into()))?,
        },
        output: Output {
            container: options.container,
            encoder: options.encoder,
            sample_rate: options.sample_rate,
            channels: options.channels,
            sample_format: options.sample_format,
            bitrate: if options.no_bitrate {
                None
            } else {
                Some(options.bitrate)
            },
            threads: options.native_threads,
        },
        max_duration_ms: options.max_duration,
        max_bytes: options.max_bytes,
        pauses: options.pauses,
        work_dir: options.work_dir,
    };
    serde_json::to_value(processor.run(
        &options.input.value(),
        &options.output,
        &config,
        &control,
    )?)
    .map_err(|e| smartaudio::Error::Io(e.to_string()))
}
fn main() -> std::process::ExitCode {
    let command = match Cli::try_parse() {
        Ok(cli) => cli.command,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            print!("{error}");
            return std::process::ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"protocol_version":1,"error":smartaudio::Error::InvalidConfig(error.to_string())})
            );
            return std::process::ExitCode::from(2);
        }
    };
    match run(command) {
        Ok(result) => {
            println!("{result}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"protocol_version":1,"error":error})
            );
            std::process::ExitCode::from(match error {
                smartaudio::Error::InvalidConfig(_) => 2,
                smartaudio::Error::Cancelled => 130,
                _ => 1,
            })
        }
    }
}
