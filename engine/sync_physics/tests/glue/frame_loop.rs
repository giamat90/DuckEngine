//! A whole frame in the order of the design note: gameplay writes a force, the fixed steps run
//! the three physics phases, the transforms are updated, and two runs agree bit for bit.

use crate::common::{DT, circle, entity, ground, physics};
use base::math::Vec2;
use ecs::World;
use frame::{FramePlan, Phase};
use scene::{Transform2D, TransformUpdater, WorldTransform2D};
use sync_physics::{AppliedForce, SyncError, attach_body, pull_transforms, push_forces};

struct Sim {
    world: World,
    phys: physics::d2::World,
    updater: TransformUpdater,
}

impl Sim {
    fn run_phase(&mut self, phase: Phase) -> Result<(), SyncError> {
        match phase {
            Phase::SyncPhysicsIn => push_forces(&mut self.world, &mut self.phys).map(drop),
            Phase::PhysicsStep => Ok(self.phys.step(DT)?),
            Phase::SyncPhysicsOut => pull_transforms(&mut self.world, &self.phys).map(drop),
            Phase::SceneUpdate => self
                .updater
                .update(&mut self.world)
                .map(drop)
                .map_err(|_| SyncError::NotAttached),
            _ => Ok(()),
        }
    }
}

fn run(frames: u32) -> Result<(u64, Vec2, usize), SyncError> {
    let mut sim = Sim {
        world: World::new(),
        phys: physics(Vec2::new(0.0, -9.8), 8)?,
        updater: TransformUpdater::new(),
    };
    let floor = entity(&mut sim.world)?;
    attach_body(
        &mut sim.world,
        &mut sim.phys,
        floor,
        &ground(Vec2::new(0.0, -1.0)),
    )?;
    let ball = entity(&mut sim.world)?;
    attach_body(
        &mut sim.world,
        &mut sim.phys,
        ball,
        &circle(Vec2::new(0.0, 2.0)),
    )?;
    let push = AppliedForce {
        force: Vec2::new(2.0, 0.0),
        torque: 0.0,
    };
    sim.world.insert(ball, push)?;
    let mut order = 0_usize;
    for _ in 0..frames {
        for phase in FramePlan::new(1) {
            order += 1;
            sim.run_phase(phase)?;
        }
    }
    let pose = sim
        .world
        .get::<Transform2D>(ball)
        .copied()
        .ok_or(SyncError::NotAttached)?;
    let world_pose = sim
        .world
        .get::<WorldTransform2D>(ball)
        .copied()
        .ok_or(SyncError::NotAttached)?;
    assert_eq!(world_pose.transform_point(Vec2::ZERO), pose.position);
    Ok((sim.phys.state_hash(), pose.position, order))
}

#[test]
fn the_ball_falls_to_the_floor_and_is_pushed_along_it() -> Result<(), SyncError> {
    let (_, at, order) = run(120)?;
    let y = at.y;
    assert!(at.x > 0.1, "the force pushed it, x = {}", at.x);
    assert_eq!(order, 120 * 9);
    assert!(y > -0.6 && y < 0.6, "the ball rests on the floor, y = {y}");
    Ok(())
}

#[test]
fn two_runs_give_the_same_physics_hash() -> Result<(), SyncError> {
    assert_eq!(run(90)?.0, run(90)?.0);
    Ok(())
}
