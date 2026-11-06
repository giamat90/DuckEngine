//! The interactive example's frame on the Null device (card 3-16): the sprites of the controls are
//! a valid command stream, and a warm frame makes no allocator call. No GPU needed.

use app_sprite_sample::input::{Controls, keys};
use app_sprite_sample::{GOLDEN_SHADER, Sample};
use assets::dshd::Program;
use base::alloc::{CountingAllocator, count_allocations};
use platform::window::{Action, Event, WindowId};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const SIZE: (u32, u32) = (640, 360);

fn sample(device: &mut NullDevice) -> Sample {
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: SIZE.0,
            height: SIZE.1,
        })
        .unwrap_or_else(|_| unreachable!());
    let program = Program::parse(GOLDEN_SHADER).unwrap_or_else(|_| unreachable!());
    Sample::new(device, &program, surface, SIZE).unwrap_or_else(|_| unreachable!())
}

#[test]
fn the_player_and_the_marker_are_two_draws() {
    let mut device = NullDevice::new();
    let mut sample = sample(&mut device);
    let stats = sample.draw_controls(&mut device, &Controls::new(SIZE));
    assert_eq!(stats.as_ref().err(), None);
    // The checker for the player and the white texture for the marker.
    assert_eq!(stats.map_or(0, |s| s.draws), 2);
}

#[test]
fn a_warm_frame_with_input_makes_no_allocator_call() {
    let mut device = NullDevice::new();
    let mut sample = sample(&mut device);
    let mut controls = Controls::new(SIZE);
    assert!(sample.draw_controls(&mut device, &controls).is_ok());
    controls.apply(&Event::Key {
        window: WindowId::from_raw(0, 0),
        scancode: keys::RIGHT[0],
        action: Action::Press,
        mods: 0,
    });
    controls.step();
    let (done, counts) = count_allocations(|| sample.draw_controls(&mut device, &controls).is_ok());
    assert!(done);
    assert_eq!(counts.calls(), 0);
}
