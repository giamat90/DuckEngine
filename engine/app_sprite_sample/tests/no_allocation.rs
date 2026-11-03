//! Once built, a frame of the sample (sort, batch, upload, record, submit) makes no allocator
//! call: the zero-allocation rule for the hot path, checked on the Null device.

use app_sprite_sample::{GOLDEN_SHADER, Sample, Scene};
use assets::dshd::Program;
use base::alloc::{CountingAllocator, count_allocations};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_frame_makes_no_allocator_call() {
    let mut device = NullDevice::new();
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: 640,
            height: 360,
        })
        .unwrap_or_else(|_| unreachable!());
    let program = Program::parse(GOLDEN_SHADER).unwrap_or_else(|_| unreachable!());
    let mut sample =
        Sample::new(&mut device, &program, surface, (640, 360)).unwrap_or_else(|_| unreachable!());
    for scene in [Scene::Triangle, Scene::Sprites] {
        assert!(sample.draw(&mut device, 0, scene).is_ok());
        let (done, counts) = count_allocations(|| sample.draw(&mut device, 1, scene).is_ok());
        assert!(done, "{scene:?}");
        assert_eq!(counts.calls(), 0, "{scene:?}");
    }
}
