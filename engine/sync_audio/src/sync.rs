//! The copy from the cue queue into the audio engine.

use crate::cue::CueQueue;
use crate::cue::SoundCue;
use crate::listener::Listener;
use audio::engine::AudioEngine;
use audio::mixer::PlayParams;
use ecs::World;
use scene::WorldTransform2D;

/// What one call did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SyncReport {
    /// Cues that became voices.
    pub played: u32,
    /// Cues the engine refused (an unknown sound, a bad value, no free voice, a full command
    /// queue). They are gone, not retried.
    pub failed: u32,
    /// Voices that had ended and were freed.
    pub finished: u32,
}

/// Once a frame: frees the voices that ended, then plays every waiting cue in order and empties
/// the queue.
pub fn sync_audio(
    world: &World,
    engine: &mut AudioEngine,
    queue: &mut CueQueue,
    listener: &Listener,
) -> SyncReport {
    let mut report = SyncReport::default();
    while engine.poll_finished().is_some() {
        bump(&mut report.finished);
    }
    for cue in queue.drain() {
        let params = params_of(world, *listener, &cue);
        match engine.play(cue.sound, params) {
            Ok(_) => bump(&mut report.played),
            Err(_) => bump(&mut report.failed),
        }
    }
    report
}

/// Adds one to a counter without overflow.
fn bump(counter: &mut u32) {
    *counter = counter.saturating_add(1);
}

/// How the engine plays a cue: the pan comes from the world x of the source entity, the centre
/// when the cue has no source, the source is gone or has no world transform.
fn params_of(world: &World, listener: Listener, cue: &SoundCue) -> PlayParams {
    let pan = cue
        .source
        .and_then(|source| world.get::<WorldTransform2D>(source))
        .map_or(0.0, |transform| listener.pan(transform.matrix.cols[2].x));
    PlayParams {
        gain: cue.gain,
        pan,
        pitch: cue.pitch,
        looping: false,
        bus: cue.bus,
    }
}
