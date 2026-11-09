//! Plays a generated tone and, with `--wav PATH`, a WAV file on the default sound device.
//! `app_audio_sample [--millis N] [--wav PATH]`. Exit code 3 when there is no sound device (the
//! program says so and does nothing else), 2 for any other failure.

use adapter_audio_miniaudio::MiniaudioDevice;
use app_audio_sample::{MAX_WAV_BYTES, Options, exit_code, parse_args, play, read_wav};
use std::process::ExitCode;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(10);
const EXIT_FAILURE: u8 = 2;
/// The options from the command line, reading the WAV file if one is named.
fn options() -> Result<Options, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse_args(&args)?;
    let wav = parsed
        .wav_path
        .as_deref()
        .map(|path| read_wav(path, MAX_WAV_BYTES))
        .transpose()?;
    Ok(Options {
        tone_millis: parsed.tone_millis,
        wav,
    })
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let mut device = MiniaudioDevice::new();
    match play(&mut device, &options, |_| std::thread::sleep(POLL)) {
        Ok(outcome) => {
            println!(
                "played {} voices, {} finished",
                outcome.voices_started, outcome.voices_finished
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(exit_code(&error))
        }
    }
}
