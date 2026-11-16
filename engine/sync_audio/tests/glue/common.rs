//! Shared set-up: an engine and its processor, sounds, and a world with a listener.

use audio::device::AudioRenderer;
use audio::engine::{AudioEngine, AudioProcessor, EngineConfig, SoundHandle, engine};
use audio::format::{ChannelLayout, StreamFormat};
use audio::sound::Sound;
use base::math::Vec2;
use ecs::{Entity, World};
use scene::{Transform2D, TransformUpdater};
use sync_audio::Listener;

pub(crate) type Failure = Box<dyn std::error::Error>;

pub(crate) const RATE: u32 = 48_000;
pub(crate) const BLOCK: u32 = 64;

pub(crate) struct Rig {
    pub(crate) engine: AudioEngine,
    pub(crate) processor: AudioProcessor,
}

pub(crate) fn rig(max_voices: u32) -> Result<Rig, Failure> {
    let format = StreamFormat {
        sample_rate: RATE,
        layout: ChannelLayout::Stereo,
        max_block_frames: 1024,
    };
    let config = EngineConfig {
        max_voices,
        max_sounds: 8,
        command_capacity: 64,
    };
    let (engine, processor) = engine(format, config)?;
    Ok(Rig { engine, processor })
}

/// A mono sound of `frames` frames of the constant `value`, registered in `engine`.
pub(crate) fn tone(
    engine: &mut AudioEngine,
    frames: usize,
    value: f32,
) -> Result<SoundHandle, Failure> {
    Ok(engine.register_sound(Sound::new(RATE, 1, vec![value; frames])?)?)
}

/// Renders one block and returns the last frame as (left, right).
pub(crate) fn last_frame(processor: &mut AudioProcessor) -> (f32, f32) {
    let mut out = vec![9.0; usize::try_from(BLOCK).unwrap_or(0) * 2];
    processor.render(&mut out, BLOCK);
    let n = out.len();
    (out[n - 2], out[n - 1])
}

pub(crate) fn listener() -> Listener {
    Listener {
        x: 0.0,
        half_width: 10.0,
    }
}

/// An entity at (`x`, 0) with its world transform up to date.
pub(crate) fn emitter(world: &mut World, x: f32) -> Result<Entity, Failure> {
    let entity = world.spawn()?;
    world.insert(entity, Transform2D::from_position(Vec2::new(x, 0.0)))?;
    TransformUpdater::new().update(world)?;
    Ok(entity)
}

pub(crate) fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}
