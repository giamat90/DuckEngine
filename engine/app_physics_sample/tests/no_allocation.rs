//! Once warm, a frame of the sample (spawn, physics steps, sprites, debug drawing, sort, batch,
//! upload, record, submit) makes no allocator call: the zero-allocation rule for the hot path,
//! checked on the Null device with the physics step on this thread.

use app_physics_sample::session::Session;
use app_physics_sample::simulation::STEP_TICKS;
use app_sprite_sample::GOLDEN_SHADER;
use assets::dshd::Program;
use base::alloc::{CountingAllocator, count_allocations};
use platform::window::{Action, Event, WindowId};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const SIZE: (u32, u32) = (800, 450);

#[test]
fn a_warm_frame_makes_no_allocator_call() {
    let mut device = NullDevice::new();
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: SIZE.0,
            height: SIZE.1,
        })
        .unwrap_or_else(|_| unreachable!());
    let program = Program::parse(GOLDEN_SHADER).unwrap_or_else(|_| unreachable!());
    let mut session =
        Session::new(&mut device, &program, surface, SIZE, 24).unwrap_or_else(|_| unreachable!());
    // Warm up through the stream, so the arena holds sleeping and moving bodies and contacts.
    for _ in 0..300 {
        assert!(session.frame(&mut device, STEP_TICKS).is_ok());
    }
    let window = WindowId::from_raw(0, 0);
    session.apply(&Event::MouseButton {
        window,
        button: 0,
        action: Action::Press,
    });
    // This frame carries a click (a body created) and, being step 300, a stream spawn.
    let (done, counts) = count_allocations(|| session.frame(&mut device, STEP_TICKS).is_ok());
    assert!(done);
    assert_eq!(counts.calls(), 0);
    for _ in 0..10 {
        let (done, counts) = count_allocations(|| session.frame(&mut device, STEP_TICKS).is_ok());
        assert!(done);
        assert_eq!(counts.calls(), 0);
    }
}
