//! Once warm, a frame of the sandbox makes no allocator call: the zero-allocation rule for the
//! hot path, checked with the physics step on this thread. The frames hold the input, the stream,
//! the physics steps, the transform update and the extraction.

use crate::common::{Failure, Recorder, STEP_TICKS, sandbox};
use app_sandbox3d::controls::Requests;
use base::alloc::{CountingAllocator, count_allocations};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_frame_makes_no_allocator_call() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    // Past the stream, so the box holds moving and resting pieces.
    for _ in 0..1500 {
        sandbox.frame(Requests::default(), STEP_TICKS, &mut draw)?;
    }
    for _ in 0..60 {
        let (frame, counts) =
            count_allocations(|| sandbox.frame(Requests::default(), STEP_TICKS, &mut NoDraw));
        frame?;
        assert_eq!(counts.calls(), 0);
    }
    assert!(
        sandbox.pieces() >= 20,
        "the measured frames are not an empty box"
    );
    Ok(())
}

/// A draw target that does nothing, so the recorder's own copy of the scene is not measured.
struct NoDraw;

impl app_sandbox3d::sandbox::Draw for NoDraw {
    fn draw(&mut self, _: &render::mesh_scene::MeshScene) -> Result<(), app_sandbox3d::Error> {
        Ok(())
    }
}

#[test]
fn a_warm_frame_with_the_renderer_makes_no_allocator_call() -> Result<(), Failure> {
    let (mut device, mut session) = crate::common::null_session(24)?;
    for _ in 0..1500 {
        session.frame(&mut device, STEP_TICKS)?;
    }
    for _ in 0..60 {
        let (frame, counts) = count_allocations(|| session.frame(&mut device, STEP_TICKS));
        frame?;
        assert_eq!(counts.calls(), 0);
    }
    Ok(())
}
