//! Once warm, a frame of the sandbox makes no allocator call: the zero-allocation rule for the
//! hot path, checked on the Null device with the physics step on this thread. The frames hold the
//! input, the stream, the physics steps, the transform update, the extraction, the batching, the
//! recording, the submit, the cues and the audio mix.

use crate::common::{Failure, null_session};
use app_physics_sample::simulation::STEP_TICKS;
use base::alloc::{CountingAllocator, count_allocations};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_frame_makes_no_allocator_call() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    // Past the stream (80 bodies, one every 12 steps), so the arena holds moving and resting
    // bodies and the impacts have played.
    for _ in 0..1100 {
        session.frame(&mut device, STEP_TICKS)?;
    }
    for _ in 0..60 {
        let (frame, counts) = count_allocations(|| session.frame(&mut device, STEP_TICKS));
        frame?;
        assert_eq!(counts.calls(), 0);
    }
    // The measured frames are part of the steady state, not an empty arena.
    assert!(session.sandbox().bodies() >= 80);
    Ok(())
}

#[test]
fn a_frame_with_impacts_and_cues_makes_no_allocator_call() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    for _ in 0..40 {
        session.frame(&mut device, STEP_TICKS)?;
    }
    // Early in the stream the bodies land one after the other: some frame has a cue.
    let mut played = 0;
    for _ in 0..400 {
        let (frame, counts) = count_allocations(|| session.frame(&mut device, STEP_TICKS));
        let report = frame?;
        played += report.played;
        // A frame that makes an entity may grow a column of the ECS (the ECS has no reserve call
        // yet); a frame that does not must not allocate, impacts and cues included.
        if report.spawned == 0 {
            assert_eq!(counts.calls(), 0);
        }
    }
    assert!(played > 0);
    Ok(())
}
