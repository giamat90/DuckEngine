//! The engine across two real threads, and the real-time rule of the processor (card 5-5):
//! once built, `AudioProcessor::render` makes no allocator call and frees nothing, even when
//! sounds are unregistered while their voices play. The allocator counters are per thread, so the
//! real-time test runs the processor on the test thread; the two-thread test checks the accounting
//! of voices, not allocation.

use audio::device::AudioRenderer;
use audio::engine::{AudioEngine, AudioProcessor, EngineConfig, EngineError, SoundHandle, engine};
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::PlayParams;
use audio::sound::Sound;
use base::alloc::{CountingAllocator, count_allocations};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RATE: u32 = 48_000;
const VOICES: u32 = 16;

fn format() -> StreamFormat {
    StreamFormat {
        sample_rate: RATE,
        layout: ChannelLayout::Stereo,
        max_block_frames: 256,
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        max_voices: VOICES,
        max_sounds: 4,
        command_capacity: 64,
    }
}

fn tone(frames: usize) -> Result<Sound, Box<dyn std::error::Error>> {
    Ok(Sound::new(RATE, 1, vec![0.25; frames])?)
}

fn drain(engine: &mut AudioEngine) -> usize {
    let mut count = 0;
    while engine.poll_finished().is_some() {
        count += 1;
    }
    count
}

/// Voices started and voices reported finished.
#[derive(Default)]
struct Tally {
    played: usize,
    finished: usize,
}

/// One game-side step: start voices, change and stop some, swap a sound now and then.
fn game_step(
    engine: &mut AudioEngine,
    sounds: &mut [Option<SoundHandle>],
    step: usize,
    looping: bool,
    tally: &mut Tally,
) -> Result<(), Box<dyn std::error::Error>> {
    let slot = step % sounds.len();
    if step.is_multiple_of(97) {
        if let Some(old) = sounds.get_mut(slot).and_then(Option::take) {
            engine.unregister_sound(old)?;
        }
        if let Some(entry) = sounds.get_mut(slot) {
            *entry = Some(engine.register_sound(tone(300 + step % 700)?)?);
        }
    }
    let Some(Some(sound)) = sounds.get(slot).copied() else {
        return Ok(());
    };
    let params = PlayParams {
        looping: looping && step.is_multiple_of(5),
        ..PlayParams::default()
    };
    match engine.play(sound, params) {
        Ok(voice) => {
            tally.played += 1;
            if step.is_multiple_of(3) {
                // The voice may have ended already; that is a stale handle, not a fault.
                let _ = engine.set_gain(voice, 0.5);
                let _ = engine.stop(voice);
            }
        }
        Err(EngineError::VoicesExhausted | EngineError::QueueFull) => {
            // The audio thread has not caught up yet.
            std::thread::yield_now();
        }
        Err(other) => return Err(other.into()),
    }
    tally.finished += drain(engine);
    Ok(())
}

fn render_counted(processor: &mut AudioProcessor, out: &mut [f32], frames: u32) -> u64 {
    let ((), counts) = count_allocations(|| processor.render(out, frames));
    counts.calls()
}

#[test]
fn rendering_makes_no_allocator_call_and_frees_nothing() -> TestResult {
    let (mut engine, mut processor) = engine(format(), config())?;
    let mut sounds: Vec<Option<SoundHandle>> = vec![None; 3];
    let mut out = vec![0.0_f32; 256 * 2];
    let mut calls = 0;
    let mut tally = Tally::default();
    for step in 0..4000 {
        game_step(&mut engine, &mut sounds, step, true, &mut tally)?;
        calls += render_counted(&mut processor, &mut out, 64 + u32::try_from(step % 150)?);
    }
    assert_eq!(calls, 0, "allocator calls inside render");
    assert!(
        tally.finished > 100,
        "the run really ended voices: {}",
        tally.finished
    );
    Ok(())
}

#[test]
fn two_threads_play_stop_and_swap_sounds_without_losing_a_voice() -> TestResult {
    let (mut engine, mut processor) = engine(format(), config())?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let audio = std::thread::spawn(move || {
        let mut out = vec![0.0_f32; 256 * 2];
        let mut blocks = 0_u64;
        while !flag.load(Ordering::Acquire) {
            processor.render(&mut out, 100 + u32::try_from(blocks % 150).unwrap_or(0));
            blocks += 1;
            std::thread::yield_now();
        }
        processor
    });
    let mut sounds: Vec<Option<SoundHandle>> = vec![None; 3];
    let mut tally = Tally::default();
    // Run until enough voices were started; the step limit only stops a stuck audio thread.
    let target = if cfg!(miri) { 40 } else { 20_000 };
    let mut steps = 0;
    while tally.played < target && steps < 50_000_000 {
        game_step(&mut engine, &mut sounds, steps, false, &mut tally)?;
        steps += 1;
    }
    stop.store(true, Ordering::Release);
    let mut processor = audio.join().map_err(|_| "audio thread panicked")?;

    // No voice loops here: let the rest end, read the events, and every voice is free again.
    let mut out = vec![0.0_f32; 256 * 2];
    for _ in 0..8 {
        processor.render(&mut out, 256);
    }
    tally.finished += drain(&mut engine);
    assert!(
        tally.played >= target,
        "voices were started: {}",
        tally.played
    );
    assert_eq!(
        tally.finished, tally.played,
        "every started voice was reported once"
    );
    let Some(Some(sound)) = sounds.first().copied() else {
        return Err("no sound registered".into());
    };
    for _ in 0..VOICES {
        engine.play(sound, PlayParams::default())?;
    }
    Ok(())
}

#[test]
fn after_a_run_every_voice_is_free_again() -> TestResult {
    let (mut engine, mut processor) = engine(format(), config())?;
    let sound = engine.register_sound(tone(100)?)?;
    let mut out = vec![0.0_f32; 256 * 2];
    for round in 0..50 {
        for _ in 0..VOICES {
            engine.play(sound, PlayParams::default())?;
        }
        assert_eq!(
            engine.play(sound, PlayParams::default()).err(),
            Some(EngineError::VoicesExhausted),
            "round {round}"
        );
        processor.render(&mut out, 256);
        drain(&mut engine);
    }
    Ok(())
}
