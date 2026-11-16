//! After warm-up, the call makes no allocator call on the calling thread.

use crate::common::{Failure, emitter, last_frame, listener, rig, tone};
use audio::engine::{AudioEngine, SoundHandle};
use base::alloc::{CountingAllocator, count_allocations};
use ecs::{Entity, World};
use std::alloc::System;
use sync_audio::{CueQueue, SoundCue, sync_audio};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn fill(queue: &mut CueQueue, sound: SoundHandle, source: Entity, n: usize) -> Result<(), Failure> {
    for _ in 0..n {
        queue.push(SoundCue::new(sound).at(source))?;
    }
    Ok(())
}

fn warm(engine: &mut AudioEngine) -> Result<SoundHandle, Failure> {
    tone(engine, 16, 0.25)
}

#[test]
fn a_warm_call_with_plays_failures_and_finished_voices_does_not_allocate() -> Result<(), Failure> {
    let mut rig = rig(4)?;
    let sound = warm(&mut rig.engine)?;
    let mut world = World::new();
    let source = emitter(&mut world, 3.0)?;
    let mut queue = CueQueue::new(8);
    fill(&mut queue, sound, source, 3)?;
    sync_audio(&world, &mut rig.engine, &mut queue, &listener());
    last_frame(&mut rig.processor);
    // Six cues for four voices: some play, some fail; the first three voices have ended.
    fill(&mut queue, sound, source, 6)?;
    let (report, counts) =
        count_allocations(|| sync_audio(&world, &mut rig.engine, &mut queue, &listener()));
    assert_eq!(report.finished, 3);
    assert!(report.played > 0 && report.failed > 0);
    assert_eq!(counts.calls(), 0);
    Ok(())
}
