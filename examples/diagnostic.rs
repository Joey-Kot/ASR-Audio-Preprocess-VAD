//! Manual diagnostics for a caller-supplied local file; no bundled fixtures.
use smartaudio::{Control, Event, Input, Processor};
fn main() -> smartaudio::Result<()> {
    let input = Input::new(std::env::args_os().nth(1).expect("input path"));
    let events = |event| {
        if let Event::Diagnostic { message } = event {
            eprintln!("{message}");
        }
    };
    let analysis = Processor.analyze(
        &input,
        &Control {
            progress: Some(&events),
            ..Control::default()
        },
    )?;
    let peak_rss_kib: Option<i64> = {
        #[cfg(target_os = "linux")]
        {
            // VmHWM belongs to this executable's address space; ru_maxrss can
            // include the launcher process's peak before exec on Linux.
            std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| {
                    s.lines().find_map(|line| {
                        line.strip_prefix("VmHWM:")
                            .and_then(|v| v.split_whitespace().next())
                            .and_then(|v| v.parse().ok())
                    })
                })
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    };
    println!(
        "{}",
        serde_json::json!({"input":analysis.input,"interval_count":analysis.raw_interval_count,
        "speech_seconds":analysis.speech_intervals.iter().map(|i|i.len()).sum::<u64>() as f64/f64::from(analysis.input.sample_rate),
        "elapsed_seconds":analysis.elapsed_seconds,"peak_rss_kib":peak_rss_kib})
    );
    Ok(())
}
