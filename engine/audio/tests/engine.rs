//! The game-side handle API and the audio-side processor (card 5-5): commands cross a queue,
//! events come back, handles are made on the game side.

use audio::device::AudioRenderer;
use audio::engine::{AudioEngine, AudioProcessor, EngineConfig, EngineError, engine};
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::{Mixer, PlayParams};
use audio::sound::Sound;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RATE: u32 = 48_000;

fn format() -> StreamFormat {
    StreamFormat {
        sample_rate: RATE,
        layout: ChannelLayout::Stereo,
        max_block_frames: 1024,
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        max_voices: 4,
        max_sounds: 3,
        command_capacity: 16,
    }
}

fn pair() -> Result<(AudioEngine, AudioProcessor), EngineError> {
    engine(format(), config())
}

fn tone(frames: usize, value: f32) -> Result<Sound, Box<dyn std::error::Error>> {
    Ok(Sound::new(RATE, 1, vec![value; frames])?)
}

fn render(processor: &mut AudioProcessor, frames: u32) -> Vec<f32> {
    let mut out = vec![9.0; usize::try_from(frames).unwrap_or(0) * 2];
    processor.render(&mut out, frames);
    out
}

fn drain(engine: &mut AudioEngine) -> usize {
    let mut count = 0;
    while engine.poll_finished().is_some() {
        count += 1;
    }
    count
}

#[test]
fn a_bad_configuration_is_refused_by_name() {
    let zero_voices = EngineConfig {
        max_voices: 0,
        ..config()
    };
    let zero_sounds = EngineConfig {
        max_sounds: 0,
        ..config()
    };
    let zero_queue = EngineConfig {
        command_capacity: 0,
        ..config()
    };
    for bad in [zero_voices, zero_sounds, zero_queue] {
        assert_eq!(
            engine(format(), bad).err(),
            Some(EngineError::InvalidConfig)
        );
    }
    let mono = StreamFormat {
        layout: ChannelLayout::Mono,
        ..format()
    };
    assert_eq!(
        engine(mono, config()).err(),
        Some(EngineError::UnsupportedFormat)
    );
}

#[test]
fn a_played_sound_comes_out_like_the_mixer_plays_it() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let handle = engine.register_sound(tone(64, 0.5)?)?;
    let params = PlayParams {
        gain: 0.8,
        pan: -0.4,
        ..PlayParams::default()
    };
    engine.play(handle, params)?;
    let through_engine = render(&mut processor, 100);

    let mut mixer = Mixer::new(format(), 4)?;
    mixer.play(std::sync::Arc::new(tone(64, 0.5)?), params)?;
    let mut direct = vec![9.0; 200];
    mixer.render(&mut direct, 100);
    assert_eq!(through_engine, direct);
    assert!(through_engine.iter().any(|sample| *sample != 0.0));
    Ok(())
}

#[test]
fn nothing_plays_before_the_audio_side_runs() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let handle = engine.register_sound(tone(64, 0.5)?)?;
    engine.play(handle, PlayParams::default())?;
    assert!(render(&mut processor, 8).iter().any(|s| *s != 0.0));
    let (_other, mut idle) = pair()?;
    assert!(render(&mut idle, 8).iter().all(|s| *s == 0.0));
    Ok(())
}

#[test]
fn an_unknown_or_unregistered_sound_cannot_be_played() -> TestResult {
    let (mut engine, _processor) = pair()?;
    let handle = engine.register_sound(tone(8, 0.1)?)?;
    engine.unregister_sound(handle)?;
    assert_eq!(
        engine.play(handle, PlayParams::default()).err(),
        Some(EngineError::StaleHandle)
    );
    assert_eq!(
        engine.unregister_sound(handle).err(),
        Some(EngineError::StaleHandle)
    );
    Ok(())
}

#[test]
fn a_sound_the_mixer_cannot_play_is_refused_when_registered() -> TestResult {
    let (mut engine, _processor) = pair()?;
    let three_channels = Sound::new(RATE, 3, vec![0.0; 9])?;
    assert_eq!(
        engine.register_sound(three_channels).err(),
        Some(EngineError::UnsupportedSound)
    );
    let empty = Sound::new(RATE, 1, Vec::new())?;
    assert_eq!(
        engine.register_sound(empty).err(),
        Some(EngineError::UnsupportedSound)
    );
    Ok(())
}

#[test]
fn the_sound_table_has_a_fixed_size_and_frees_a_place_on_unregister() -> TestResult {
    let (mut engine, _processor) = pair()?;
    let mut handles = Vec::new();
    for _ in 0..3 {
        handles.push(engine.register_sound(tone(8, 0.1)?)?);
    }
    assert_eq!(
        engine.register_sound(tone(8, 0.1)?).err(),
        Some(EngineError::SoundsFull)
    );
    engine.unregister_sound(handles[1])?;
    engine.register_sound(tone(8, 0.1)?)?;
    Ok(())
}

#[test]
fn a_finished_voice_is_reported_once_and_its_handle_then_goes_stale() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(32, 0.5)?)?;
    let voice = engine.play(sound, PlayParams::default())?;
    render(&mut processor, 16);
    assert_eq!(engine.poll_finished(), None);
    render(&mut processor, 64);
    assert_eq!(engine.poll_finished(), Some(voice));
    assert_eq!(engine.poll_finished(), None);
    assert_eq!(engine.stop(voice).err(), Some(EngineError::StaleHandle));
    assert_eq!(
        engine.set_gain(voice, 0.5).err(),
        Some(EngineError::StaleHandle)
    );
    Ok(())
}

#[test]
fn a_stop_fades_the_voice_and_ends_it() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(100_000, 0.5)?)?;
    let voice = engine.play(sound, PlayParams::default())?;
    render(&mut processor, 512);
    engine.stop(voice)?;
    let faded = render(&mut processor, 512);
    assert!(faded.iter().skip(2 * 300).all(|s| *s == 0.0));
    assert_eq!(engine.poll_finished(), Some(voice));
    Ok(())
}

#[test]
fn parameters_are_checked_on_the_game_side() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(1000, 0.5)?)?;
    let voice = engine.play(sound, PlayParams::default())?;
    assert_eq!(
        engine.set_gain(voice, -1.0).err(),
        Some(EngineError::InvalidParameter)
    );
    assert_eq!(
        engine.set_gain(voice, f32::NAN).err(),
        Some(EngineError::InvalidParameter)
    );
    assert_eq!(
        engine.set_pan(voice, 1.5).err(),
        Some(EngineError::InvalidParameter)
    );
    assert_eq!(
        engine.set_pitch(voice, 0.0).err(),
        Some(EngineError::InvalidParameter)
    );
    let bad = PlayParams {
        pitch: 100.0,
        ..PlayParams::default()
    };
    assert_eq!(
        engine.play(sound, bad).err(),
        Some(EngineError::InvalidParameter)
    );
    render(&mut processor, 8);
    Ok(())
}

#[test]
fn set_commands_change_the_sound() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(100_000, 1.0)?)?;
    let voice = engine.play(sound, PlayParams::default())?;
    render(&mut processor, 512);
    engine.set_gain(voice, 0.0)?;
    let silent = render(&mut processor, 512);
    assert!(silent.iter().skip(2 * 300).all(|s| *s == 0.0));
    engine.set_gain(voice, 1.0)?;
    engine.set_pan(voice, 1.0)?;
    let right = render(&mut processor, 1024);
    let last = &right[right.len() - 2..];
    assert!(last[0].abs() < 1e-6 && last[1] > 0.99);
    engine.set_pitch(voice, 2.0)?;
    render(&mut processor, 8);
    Ok(())
}

#[test]
fn a_full_command_queue_is_an_error_and_loses_no_handle() -> TestResult {
    let small = EngineConfig {
        command_capacity: 2,
        ..config()
    };
    let (mut engine, mut processor) = engine(format(), small)?;
    let sound = engine.register_sound(tone(100_000, 0.5)?)?;
    let first = engine.play(sound, PlayParams::default())?;
    let second = engine.play(sound, PlayParams::default())?;
    assert_eq!(
        engine.play(sound, PlayParams::default()).err(),
        Some(EngineError::QueueFull)
    );
    assert_eq!(
        engine.set_gain(first, 0.5).err(),
        Some(EngineError::QueueFull)
    );
    render(&mut processor, 8);
    // The refused play did not use up a voice handle: all four voices can still be started.
    engine.play(sound, PlayParams::default())?;
    engine.play(sound, PlayParams::default())?;
    assert_eq!(
        engine.play(sound, PlayParams::default()).err(),
        Some(EngineError::QueueFull)
    );
    render(&mut processor, 8);
    assert_eq!(
        engine.play(sound, PlayParams::default()).err(),
        Some(EngineError::VoicesExhausted)
    );
    engine.stop(second)?;
    Ok(())
}

#[test]
fn voices_are_limited_and_come_back_only_after_the_event_is_read() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(16, 0.5)?)?;
    for _ in 0..4 {
        engine.play(sound, PlayParams::default())?;
    }
    assert_eq!(
        engine.play(sound, PlayParams::default()).err(),
        Some(EngineError::VoicesExhausted)
    );
    render(&mut processor, 64);
    assert_eq!(
        engine.play(sound, PlayParams::default()).err(),
        Some(EngineError::VoicesExhausted),
        "the voices ended but their events are unread"
    );
    assert_eq!(drain(&mut engine), 4);
    for _ in 0..4 {
        engine.play(sound, PlayParams::default())?;
    }
    Ok(())
}

#[test]
fn a_command_for_an_ended_voice_never_reaches_the_voice_that_reuses_its_place() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let short = engine.register_sound(tone(16, 0.5)?)?;
    let old = engine.play(short, PlayParams::default())?;
    render(&mut processor, 64);
    // Ended on the audio side, event not read yet: the commands are accepted, and wait in the
    // queue while the handle is freed and its place is used again.
    engine.stop(old)?;
    engine.set_gain(old, 0.0)?;
    assert_eq!(engine.poll_finished(), Some(old));
    let long = engine.register_sound(tone(100_000, 0.5)?)?;
    let new = engine.play(long, PlayParams::default())?;
    assert_eq!(new.index(), old.index());
    assert_ne!(new, old);
    assert_eq!(engine.stop(old).err(), Some(EngineError::StaleHandle));
    let out = render(&mut processor, 1024);
    let loud = out.iter().rev().take(2).all(|s| s.abs() > 0.3);
    assert!(
        loud,
        "the new voice was neither stopped nor muted by the old commands"
    );
    Ok(())
}

#[test]
fn the_game_side_accepts_exactly_what_the_mixer_accepts() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(1000, 0.5)?)?;
    let voice = engine.play(sound, PlayParams::default())?;
    let mut mixer = Mixer::new(format(), 4)?;
    let mixer_voice = mixer.play(std::sync::Arc::new(tone(1000, 0.5)?), PlayParams::default())?;
    let values = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        0.0,
        -1.0,
        -1.0001,
        1.0,
        1.0001,
        0.5,
        16.0,
        16.001,
        1.0e-30,
    ];
    for value in values {
        // Empty the queue so that a full queue cannot look like a refusal.
        render(&mut processor, 8);
        assert_eq!(
            engine.set_gain(voice, value).is_ok(),
            mixer.set_gain(mixer_voice, value).is_ok(),
            "gain {value}"
        );
        assert_eq!(
            engine.set_pan(voice, value).is_ok(),
            mixer.set_pan(mixer_voice, value).is_ok(),
            "pan {value}"
        );
        assert_eq!(
            engine.set_pitch(voice, value).is_ok(),
            mixer.set_pitch(mixer_voice, value).is_ok(),
            "pitch {value}"
        );
    }
    Ok(())
}

#[test]
fn an_unregistered_sound_keeps_playing_and_is_freed_on_the_game_side() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let sound = engine.register_sound(tone(200, 0.5)?)?;
    engine.play(sound, PlayParams::default())?;
    render(&mut processor, 8);
    engine.unregister_sound(sound)?;
    assert!(render(&mut processor, 8).iter().any(|s| *s != 0.0));
    render(&mut processor, 512);
    assert_eq!(drain(&mut engine), 1);
    Ok(())
}

#[test]
fn the_output_does_not_depend_on_the_block_size() -> TestResult {
    let run = |blocks: &[u32]| -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let (mut engine, mut processor) = pair()?;
        let sound = engine.register_sound(tone(5000, 0.7)?)?;
        let voice = engine.play(sound, PlayParams::default())?;
        engine.set_gain(voice, 0.3)?;
        let mut all = Vec::new();
        for frames in blocks {
            all.extend(render(&mut processor, *frames));
        }
        Ok(all)
    };
    assert_eq!(run(&[1024, 1024])?, {
        let mut mixed = run(&[1, 7, 255, 256, 257, 1024, 248])?;
        mixed.truncate(2048 * 2);
        mixed
    });
    Ok(())
}

#[test]
fn every_engine_error_has_its_own_message() {
    let all = [
        EngineError::InvalidConfig,
        EngineError::UnsupportedFormat,
        EngineError::UnsupportedSound,
        EngineError::SoundsFull,
        EngineError::VoicesExhausted,
        EngineError::StaleHandle,
        EngineError::InvalidParameter,
        EngineError::QueueFull,
    ];
    let messages: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (index, message) in messages.iter().enumerate() {
        assert!(!message.is_empty());
        assert_eq!(messages.iter().position(|m| m == message), Some(index));
    }
}

#[test]
fn a_stereo_sound_can_be_registered_and_played() -> TestResult {
    let (mut engine, mut processor) = pair()?;
    let stereo = Sound::new(RATE, 2, vec![0.25; 200])?;
    let handle = engine.register_sound(stereo)?;
    engine.play(handle, PlayParams::default())?;
    assert!(render(&mut processor, 8).iter().any(|s| *s != 0.0));
    Ok(())
}
