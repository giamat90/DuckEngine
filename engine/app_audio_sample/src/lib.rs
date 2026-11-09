//! Layer 4 sample (card 5-8): plays a generated tone and, if given, a WAV file through the audio
//! engine on any [`IAudioDevice`]. The binary uses the miniaudio device; the tests use the Offline
//! device. The session is a small loop: decode, open, start, play, pump until every voice has been
//! reported finished, stop.

use audio::decoder::{DecodeError, IAudioDecoder};
use audio::device::IAudioDevice;
use audio::engine::{EngineConfig, EngineError, engine};
use audio::error::AudioError;
use audio::format::StreamRequest;
use audio::mixer::PlayParams;
use audio::sound::{Sound, SoundError};
use audio::wav::WavDecoder;
use std::fmt;

/// Longest tone the command line accepts, in milliseconds.
pub const MAX_TONE_MILLIS: u32 = 30_000;
/// Tone length when none is given.
const DEFAULT_TONE_MILLIS: u32 = 1000;
/// The tone is made at this rate; the mixer resamples it to the device rate.
const TONE_RATE: u32 = 48_000;
const TONE_FRAMES_PER_MILLI: u32 = TONE_RATE / 1000;
/// Phase step of a 440 Hz sine at [`TONE_RATE`] (2 pi * 440 / 48000).
const TONE_STEP: f32 = 0.057_59;
const TONE_LEVEL: f32 = 0.2;
/// The time one call of the pump stands for: the binary sleeps this long, a test renders about
/// this much audio. The session gives up after twice the longest sound plus [`PUMP_MARGIN`].
const PUMP_MILLIS: u64 = 10;
const PUMP_MARGIN: u64 = 1000;

/// What to play.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Length of the generated tone.
    pub tone_millis: u32,
    /// The bytes of a WAV file to play after starting the tone, if any.
    pub wav: Option<Vec<u8>>,
}

/// The command line: the tone length and the path of a WAV file.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    /// Length of the generated tone.
    pub tone_millis: u32,
    /// The WAV file to read.
    pub wav_path: Option<String>,
}

/// What a session did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Voices started.
    pub voices_started: u32,
    /// Voices whose end was reported.
    pub voices_finished: u32,
}

/// Why a session failed.
#[derive(Debug, PartialEq)]
pub enum SampleError {
    /// The WAV bytes are not a valid file.
    Wav(DecodeError),
    /// The device refused or failed.
    Device(AudioError),
    /// The engine refused.
    Engine(EngineError),
    /// A sound could not be made.
    Sound(SoundError),
    /// The voices did not end in time.
    TimedOut,
}

impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wav(e) => write!(f, "the WAV file is not valid: {e}"),
            Self::Device(e) => write!(f, "the audio device failed: {e}"),
            Self::Engine(e) => write!(f, "the engine refused: {e}"),
            Self::Sound(e) => write!(f, "the sound is not valid: {e}"),
            Self::TimedOut => f.write_str("the sounds did not end in time"),
        }
    }
}

impl std::error::Error for SampleError {}

impl From<AudioError> for SampleError {
    fn from(e: AudioError) -> Self {
        Self::Device(e)
    }
}

impl From<EngineError> for SampleError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e)
    }
}

impl From<SoundError> for SampleError {
    fn from(e: SoundError) -> Self {
        Self::Sound(e)
    }
}

/// The exit code of a failed session: 3 when there is no sound device, 2 for anything else.
#[must_use]
pub fn exit_code(error: &SampleError) -> u8 {
    match error {
        SampleError::Device(AudioError::NoDevice) => 3,
        _ => 2,
    }
}

/// Reads `--millis N` (1 to [`MAX_TONE_MILLIS`]) and `--wav PATH`.
///
/// # Errors
/// A message for an unknown flag, a missing value or a number out of range.
pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        tone_millis: DEFAULT_TONE_MILLIS,
        wav_path: None,
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or("a flag needs a value")?;
        match flag.as_str() {
            "--millis" => {
                parsed.tone_millis = value
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=MAX_TONE_MILLIS).contains(n))
                    .ok_or(format!(
                        "--millis needs 1 to {MAX_TONE_MILLIS}, got {value}"
                    ))?;
            }
            "--wav" => parsed.wav_path = Some(value.clone()),
            _ => return Err("usage: app_audio_sample [--millis N] [--wav PATH]".to_owned()),
        }
    }
    Ok(parsed)
}

/// How long a sound lasts at its own sample rate, in milliseconds.
fn sound_millis(sound: &Sound) -> u64 {
    u64::try_from(sound.frame_count())
        .unwrap_or(u64::MAX)
        .saturating_mul(1000)
        / u64::from(sound.sample_rate())
}

/// How many pumps a session waits for sounds of at most `longest_millis`.
fn pump_limit(longest_millis: u64) -> u64 {
    longest_millis / PUMP_MILLIS * 2 + PUMP_MARGIN
}

/// The largest WAV file the sample reads: the file and its decoded copy are both held in memory.
pub const MAX_WAV_BYTES: u64 = 64 * 1024 * 1024;

/// The bytes of the WAV file at `path`, refused above `max_bytes`.
///
/// # Errors
/// A message when the file cannot be read or is too large.
pub fn read_wav(path: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("cannot read {path}: {error}"))?
        .len();
    if size > max_bytes {
        return Err(format!("{path} is larger than {max_bytes} bytes"));
    }
    std::fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))
}

/// A mono 440 Hz sine of `millis` milliseconds.
fn tone(millis: u32) -> Result<Sound, SoundError> {
    let frames = millis.saturating_mul(TONE_FRAMES_PER_MILLI);
    let mut phase = 0.0_f32;
    let samples = (0..frames)
        .map(|_| {
            phase = (phase + TONE_STEP) % std::f32::consts::TAU;
            phase.sin() * TONE_LEVEL
        })
        .collect();
    Sound::new(TONE_RATE, 1, samples)
}

/// Plays the tone (and the WAV) on `device` and returns when every voice has ended. `pump` is
/// called between polls: the binary sleeps a little, a test renders blocks.
///
/// # Errors
/// [`SampleError`]; a bad WAV is found before the device is touched.
pub fn play<D: IAudioDevice>(
    device: &mut D,
    options: &Options,
    mut pump: impl FnMut(&mut D),
) -> Result<Outcome, SampleError> {
    let wav = options
        .wav
        .as_deref()
        .map(|bytes| WavDecoder::new().decode(bytes))
        .transpose()
        .map_err(SampleError::Wav)?;
    let format = device.open(StreamRequest::native_stereo())?;
    let config = EngineConfig {
        max_voices: 4,
        max_sounds: 4,
        command_capacity: 16,
    };
    let (mut game, processor) = engine(format, config)?;
    // Everything that can fail is done before the device starts, so an error never leaves a
    // started device behind. The queue holds the play commands until the first callback.
    let mut started = 0;
    let mut longest_millis = 0;
    for sound in std::iter::once(tone(options.tone_millis)?).chain(wav) {
        longest_millis = longest_millis.max(sound_millis(&sound));
        let handle = game.register_sound(sound)?;
        game.play(handle, PlayParams::default())?;
        started += 1;
    }
    device.start(Box::new(processor))?;
    let max_pumps = pump_limit(longest_millis);
    let mut finished = 0;
    let mut pumps = 0;
    while finished < started {
        if pumps == max_pumps {
            let _stopped = device.stop();
            return Err(SampleError::TimedOut);
        }
        pumps += 1;
        pump(device);
        while game.poll_finished().is_some() {
            finished += 1;
        }
    }
    device.stop()?;
    Ok(Outcome {
        voices_started: started,
        voices_finished: finished,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_is_a_440_hz_sine_at_48_khz_of_the_asked_length() {
        let Ok(sound) = tone(100) else {
            panic!("a tone is made");
        };
        let samples = sound.samples();
        assert_eq!(samples.len(), 4800);
        assert!((samples[0] - TONE_STEP.sin() * TONE_LEVEL).abs() < 1e-6);
        let peak = samples.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
        assert!((peak - TONE_LEVEL).abs() < 1e-3, "peak {peak}");
        let rises = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        assert!(
            (43..=45).contains(&rises),
            "44 periods in 100 ms, got {rises}"
        );
    }

    #[test]
    fn a_sound_lasts_its_frames_over_its_rate_and_the_session_waits_twice_as_long() {
        let Ok(second) = Sound::new(48_000, 1, vec![0.0; 48_000]) else {
            panic!("a sound is made");
        };
        assert_eq!(sound_millis(&second), 1000);
        let Ok(half) = Sound::new(8_000, 2, vec![0.0; 8_000]) else {
            panic!("a sound is made");
        };
        assert_eq!(sound_millis(&half), 500);
        assert_eq!(pump_limit(0), PUMP_MARGIN);
        assert_eq!(pump_limit(1000), 1000 / PUMP_MILLIS * 2 + PUMP_MARGIN);
        assert_eq!(pump_limit(1000), 1200);
    }

    #[test]
    fn a_wav_file_is_read_up_to_the_limit_only() {
        let path = std::env::temp_dir().join(format!("duck-sample-{}.wav", std::process::id()));
        let name = path.to_string_lossy().into_owned();
        assert!(std::fs::write(&path, [7_u8; 10]).is_ok());
        assert_eq!(read_wav(&name, 10), Ok(vec![7_u8; 10]));
        assert!(read_wav(&name, 9).is_err_and(|m| m.contains("larger")));
        assert!(read_wav("no-such-file.wav", 10).is_err_and(|m| m.contains("cannot read")));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn every_error_has_a_message_and_the_exit_code_tells_no_device_apart() {
        let no_device = SampleError::Device(AudioError::NoDevice);
        assert_eq!(exit_code(&no_device), 3);
        assert_eq!(exit_code(&SampleError::TimedOut), 2);
        assert_eq!(exit_code(&SampleError::Engine(EngineError::QueueFull)), 2);
        for error in [
            no_device,
            SampleError::Engine(EngineError::QueueFull),
            SampleError::Sound(SoundError::PartialFrame),
            SampleError::Wav(DecodeError::Truncated),
            SampleError::TimedOut,
        ] {
            assert!(error.to_string().len() > 10, "{error:?}");
        }
    }
}
