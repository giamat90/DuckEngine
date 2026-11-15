//! After warm-up, the two copies make no allocator call on the calling thread.

use crate::common::{DT, circle, entity, physics};
use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec2;
use ecs::World;
use std::alloc::System;
use sync_physics::{AppliedForce, SyncError, attach_body, pull_transforms, push_forces};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_frame_of_both_copies_does_not_allocate() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::new(0.0, -9.8), 64)?;
    for n in 0..40_u8 {
        let e = entity(&mut world)?;
        attach_body(
            &mut world,
            &mut phys,
            e,
            &circle(Vec2::new(f32::from(n) * 2.0, 5.0)),
        )?;
        world.insert(
            e,
            AppliedForce {
                force: Vec2::new(1.0, 0.0),
                torque: 0.0,
            },
        )?;
    }
    push_forces(&mut world, &mut phys)?;
    phys.step(DT)?;
    pull_transforms(&mut world, &phys)?;
    let (result, counts) = count_allocations(|| {
        let pushed = push_forces(&mut world, &mut phys)?;
        let pulled = pull_transforms(&mut world, &phys)?;
        Ok::<_, SyncError>((pushed.synced, pulled.synced))
    });
    assert_eq!(result?, (40, 40));
    assert_eq!(counts.allocations, 0);
    Ok(())
}
