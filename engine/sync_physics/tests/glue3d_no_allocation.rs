//! After warm-up, the two 3D copies make no allocator call on the calling thread.

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec3;
use ecs::World;
use physics::d3::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Shape3, SleepConfig,
    SolverConfig, World as PhysicsWorld, WorldConfig,
};
use scene::Transform3D;
use std::alloc::System;
use sync_physics::SyncError;
use sync_physics::d3::{AppliedForce, attach_body, pull_transforms, push_forces};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn a_warm_frame_of_both_3d_copies_does_not_allocate() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = PhysicsWorld::new(WorldConfig {
        max_bodies: 64,
        gravity: Vec3::new(0.0, -9.8, 0.0),
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })?;
    for n in 0..40_u8 {
        let e = world.spawn()?;
        world.insert(e, Transform3D::IDENTITY)?;
        let at = Vec3::new(f32::from(n) * 2.0, 5.0, 0.0);
        attach_body(
            &mut world,
            &mut phys,
            e,
            &BodyDesc::new(BodyKind::Dynamic, at, Shape3::Sphere { radius: 0.5 }),
        )?;
        world.insert(
            e,
            AppliedForce {
                force: Vec3::new(1.0, 0.0, 0.0),
                torque: Vec3::ZERO,
            },
        )?;
    }
    push_forces(&mut world, &mut phys)?;
    phys.step(1.0 / 60.0)?;
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
