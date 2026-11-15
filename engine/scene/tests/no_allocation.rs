//! After a warm-up the update makes no allocator call on the calling thread (digest section 4:
//! zero heap allocations per frame on hot paths; counting allocator with per-thread counters).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec2;
use ecs::World;
use scene::{SceneError, Transform2D, TransformUpdater, set_parent};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn the_counter_sees_an_allocation() {
    let ((), counts) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(counts.allocations >= 1);
}

#[test]
fn updating_a_warm_scene_does_not_allocate() -> Result<(), SceneError> {
    let mut world = World::new();
    let mut parents = Vec::new();
    for i in 0..8_u8 {
        let root = world.spawn()?;
        world.insert(
            root,
            Transform2D::from_position(Vec2::new(f32::from(i), 0.0)),
        )?;
        parents.push(root);
        for _ in 0..8 {
            let child = world.spawn()?;
            world.insert(child, Transform2D::from_position(Vec2::new(0.0, 1.0)))?;
            set_parent(&mut world, child, root)?;
        }
    }
    let mut updater = TransformUpdater::new();
    updater.update(&mut world)?;
    let (report, counts) = count_allocations(|| updater.update(&mut world));
    assert_eq!(report?.updated, 72);
    assert_eq!(counts.allocations, 0);
    Ok(())
}
