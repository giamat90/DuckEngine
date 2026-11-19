//! After a warm-up the 3D update makes no allocator call on the calling thread (digest section 4).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec3;
use ecs::World;
use scene::{SceneError, Transform3D, TransformUpdater3D, set_parent};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn updating_a_warm_3d_scene_does_not_allocate() -> Result<(), SceneError> {
    let mut world = World::new();
    for i in 0..8_u8 {
        let root = world.spawn()?;
        world.insert(
            root,
            Transform3D::from_position(Vec3::new(f32::from(i), 0.0, 0.0)),
        )?;
        for _ in 0..8 {
            let child = world.spawn()?;
            world.insert(child, Transform3D::from_position(Vec3::new(0.0, 1.0, 0.0)))?;
            set_parent(&mut world, child, root)?;
        }
    }
    let mut updater = TransformUpdater3D::new();
    updater.update(&mut world)?;
    let (report, counts) = count_allocations(|| updater.update(&mut world));
    assert_eq!(report?.updated, 72);
    assert_eq!(counts.allocations, 0);
    Ok(())
}
