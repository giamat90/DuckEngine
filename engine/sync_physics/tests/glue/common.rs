//! Shared set-up: a physics world, an ECS world and entities with transforms.

use base::math::Vec2;
use ecs::{Entity, World};
use physics::d2::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Shape2, SleepConfig,
    SolverConfig, World as PhysicsWorld, WorldConfig,
};
use scene::Transform2D;
use sync_physics::SyncError;

pub(crate) const DT: f32 = 1.0 / 60.0;

pub(crate) fn physics(gravity: Vec2, max_bodies: u32) -> Result<PhysicsWorld, SyncError> {
    Ok(PhysicsWorld::new(WorldConfig {
        max_bodies,
        gravity,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })?)
}

pub(crate) fn entity(world: &mut World) -> Result<Entity, SyncError> {
    let entity = world.spawn()?;
    world.insert(entity, Transform2D::IDENTITY)?;
    Ok(entity)
}

pub(crate) fn circle(at: Vec2) -> BodyDesc {
    BodyDesc::new(BodyKind::Dynamic, at, Shape2::Circle { radius: 0.5 })
}

pub(crate) fn ground(at: Vec2) -> BodyDesc {
    BodyDesc::new(
        BodyKind::Static,
        at,
        Shape2::Box {
            half_extents: Vec2::new(10.0, 0.5),
        },
    )
}
