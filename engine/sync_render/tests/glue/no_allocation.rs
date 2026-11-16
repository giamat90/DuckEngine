//! After warm-up, the extraction makes no allocator call on the calling thread.

use crate::common::{at, drawable, refresh, render_scene, textures, unit};
use base::alloc::{CountingAllocator, count_allocations};
use ecs::World;
use std::alloc::System;
use sync_render::{SyncError, extract_sprites};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_extraction_does_not_allocate() -> Result<(), SyncError> {
    let tex = textures(3);
    let mut world = World::new();
    for n in 0..200_u8 {
        drawable(
            &mut world,
            at(f32::from(n), 0.0),
            unit(tex[usize::from(n % 3)]),
        )?;
    }
    refresh(&mut world);
    let mut scene = render_scene(256);
    extract_sprites(&mut world, &mut scene)?;
    let (result, counts) = count_allocations(|| extract_sprites(&mut world, &mut scene));
    assert_eq!(result?.extracted, 200);
    assert_eq!(counts.allocations, 0);
    Ok(())
}
