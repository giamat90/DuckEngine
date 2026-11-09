//! The device contract against the machine's real sound device. A machine without miniaudio
//! compiled in, or without a playback device, reports "not run: device not set up" and passes,
//! unless `DUCK_REQUIRE_AUDIO` is set (then the missing device is a failure).

use adapter_audio_miniaudio::{MiniaudioDevice, is_compiled_in};
use audio::contract::device_contract;
use audio::device::{AudioRenderer, IAudioDevice};
use audio::engine::{EngineConfig, engine};
use audio::error::AudioError;
use audio::format::StreamRequest;
use audio::mixer::PlayParams;
use audio::sound::Sound;
use std::time::{Duration, Instant};

/// True when the test can go on; otherwise says why it did not run. It tries a device of its own,
/// so the device a test then builds is still closed (the contract suite starts from there).
fn device_is_set_up() -> bool {
    let reason = if is_compiled_in() {
        match MiniaudioDevice::new().open(StreamRequest::native_stereo()) {
            Ok(_) => return true,
            Err(error) => format!("open failed: {error}"),
        }
    } else {
        "miniaudio is not compiled in".to_owned()
    };
    assert!(
        std::env::var_os("DUCK_REQUIRE_AUDIO").is_none(),
        "an audio device is required here but cannot start: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
    false
}

#[test]
fn the_real_device_passes_the_contract_suite() {
    if !device_is_set_up() {
        return;
    }
    let mut device = MiniaudioDevice::new();
    device_contract(&mut device, true, |_| {
        std::thread::sleep(Duration::from_millis(150));
    });
}

/// A quiet tone, so a person running the test with speakers on may hear that it is real.
struct Tone(f32);

impl AudioRenderer for Tone {
    fn render(&mut self, out: &mut [f32], _frames: u32) {
        for frame in out.chunks_mut(2) {
            self.0 = (self.0 + 0.0583) % std::f32::consts::TAU;
            frame.fill(self.0.sin() * 0.02);
        }
    }
}

#[test]
fn the_real_device_calls_the_renderer_and_counts() {
    if !device_is_set_up() {
        return;
    }
    let mut device = MiniaudioDevice::new();
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
    assert_eq!(device.start(Box::new(Tone(0.0))), Ok(()));
    std::thread::sleep(Duration::from_millis(300));
    let status = device.status();
    assert!(status.callbacks > 0, "no callback in 300 ms");
    assert!(status.frames_rendered > 0);
    assert!(!status.lost);
    assert!(device.stop().is_ok());
    assert_eq!(device.stop().err(), Some(AudioError::WrongState));
}

#[test]
fn the_real_device_restarts_and_a_started_device_can_be_dropped() {
    if !device_is_set_up() {
        return;
    }
    let mut device = MiniaudioDevice::new();
    for _ in 0..3 {
        assert!(device.open(StreamRequest::native_stereo()).is_ok());
        assert_eq!(device.start(Box::new(Tone(0.0))), Ok(()));
        std::thread::sleep(Duration::from_millis(60));
        assert!(device.stop().is_ok());
    }
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
    assert_eq!(device.start(Box::new(Tone(0.0))), Ok(()));
    std::thread::sleep(Duration::from_millis(60));
    drop(device);
}

/// The whole chain on the real device: the engine's processor is the device's renderer, the game
/// thread starts a 200 ms sound, and the device plays it to its end (the end comes back as an
/// event). This is the first sound out of a real device.
#[test]
fn the_engine_plays_a_sound_to_its_end_on_the_real_device() -> Result<(), Box<dyn std::error::Error>>
{
    if !device_is_set_up() {
        return Ok(());
    }
    let mut device = MiniaudioDevice::new();
    let format = device.open(StreamRequest::native_stereo())?;
    let config = EngineConfig {
        max_voices: 8,
        max_sounds: 2,
        command_capacity: 16,
    };
    let (mut game, processor) = engine(format, config)?;
    device.start(Box::new(processor))?;
    let frames = usize::try_from(format.sample_rate / 5)?;
    let wave: Vec<f32> = (0..frames)
        .scan(0.0_f32, |phase, _| {
            *phase = (*phase + 0.0583) % std::f32::consts::TAU;
            Some(phase.sin() * 0.02)
        })
        .collect();
    let sound = game.register_sound(Sound::new(format.sample_rate, 1, wave)?)?;
    let voice = game.play(sound, PlayParams::default())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ended = None;
    while ended.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        ended = game.poll_finished();
    }
    drop(device.stop()?);
    assert_eq!(ended, Some(voice), "the voice did not end within 5 s");
    Ok(())
}
