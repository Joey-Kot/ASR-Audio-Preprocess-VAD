use smartaudio::{Config, Control, Input, Processor};
fn main() -> smartaudio::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let input = Input::new(args.next().expect("input path"));
    let directory = std::path::PathBuf::from(args.next().expect("output directory"));
    let result = Processor.process(&input, &directory, &Config::default(), &Control::default())?;
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    Ok(())
}
