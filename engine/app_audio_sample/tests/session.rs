//! The sample's session on the Offline device: no sound hardware, no clock. The pump renders
//! blocks the way a device thread would.

use app_audio_sample::{Options, SampleError, parse_args, play};
use audio::device::IAudioDevice;
use audio::error::AudioError;
use audio::format::StreamRequest;
use audio::offline::OfflineDevice;
use std::error::Error;

const BLOCK: usize = 512;

/// A minimal mono 16-bit PCM WAV of `frames` frames of silence-ish ramp at 48 kHz.
fn wav_bytes(frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..frames {
        let sample = i16::try_from(n % 2000).unwrap_or(0);
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn pump(device: &mut OfflineDevice) {
    let mut out = vec![0.0_f32; BLOCK * 2];
    let _ = device.render_into(&mut out);
}

#[test]
fn a_tone_plays_to_its_end() -> Result<(), Box<dyn Error>> {
    let mut device = OfflineDevice::new();
    let options = Options {
        tone_millis: 250,
        wav: None,
    };
    let outcome = play(&mut device, &options, pump)?;
    assert_eq!(outcome.voices_started, 1);
    assert_eq!(outcome.voices_finished, 1);
    assert!(device.status().frames_rendered > 0);
    Ok(())
}

#[test]
fn a_tone_and_a_wav_both_play_to_their_end() -> Result<(), Box<dyn Error>> {
    let mut device = OfflineDevice::new();
    let options = Options {
        tone_millis: 100,
        wav: Some(wav_bytes(12_000)),
    };
    let outcome = play(&mut device, &options, pump)?;
    assert_eq!(outcome.voices_started, 2);
    assert_eq!(outcome.voices_finished, 2);
    Ok(())
}

#[test]
fn a_wav_that_is_not_a_wav_is_an_error_and_nothing_plays() {
    let mut device = OfflineDevice::new();
    let options = Options {
        tone_millis: 100,
        wav: Some(b"not a wav".to_vec()),
    };
    assert!(matches!(
        play(&mut device, &options, pump),
        Err(SampleError::Wav(_))
    ));
    assert!(
        device.open(StreamRequest::native_stereo()).is_ok(),
        "the device was never opened"
    );
}

#[test]
fn a_session_that_never_ends_gives_up() {
    let mut device = OfflineDevice::new();
    let options = Options {
        tone_millis: 100,
        wav: None,
    };
    assert!(matches!(
        play(&mut device, &options, |_| {}),
        Err(SampleError::TimedOut)
    ));
    assert_eq!(
        device.stop().err(),
        Some(AudioError::WrongState),
        "the device was stopped"
    );
}

#[test]
fn arguments_are_read_and_bad_ones_refused() {
    assert_eq!(parse_args(&[]).map(|a| a.tone_millis), Ok(1000));
    let args = ["--millis", "2500", "--wav", "a.wav"].map(str::to_owned);
    assert_eq!(
        parse_args(&args).map(|a| (a.tone_millis, a.wav_path)),
        Ok((2500, Some("a.wav".to_owned())))
    );
    let bad_lists: [&[&str]; 5] = [
        &["--millis", "0"],
        &["--millis", "x"],
        &["--millis", "99999"],
        &["--wav"],
        &["--zzz", "1"],
    ];
    for bad in bad_lists {
        let list: Vec<String> = bad.iter().map(|s| (*s).to_owned()).collect();
        assert!(parse_args(&list).is_err(), "{bad:?}");
    }
}
