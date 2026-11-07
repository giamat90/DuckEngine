//! After `World::new`, a world with joints allocates nothing either (card 4-16): steps of a chain, a
//! bridge and contacts, creating and destroying joints within the capacity, the refusal at the
//! capacity, destroying a body that has joints, and the state hash make no allocator call on the
//! calling thread (counting allocator with per-thread counters, as the other physics tests do).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    DistanceJoint2, Joint2, JointDesc, PhysicsError, RevoluteJoint2, STANDARD_GRAVITY, Shape2,
    SleepConfig, SolverConfig, SolverExecution, World, WorldConfig,
};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const LINKS: u8 = 12;
const DT: f32 = 1.0 / 60.0;

fn build_world(sleep: SleepConfig) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 32,
        gravity: STANDARD_GRAVITY,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 16,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })
}

fn body(
    world: &mut World,
    kind: BodyKind,
    at: Vec2,
    half: Vec2,
) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(kind, at, Shape2::Box { half_extents: half }))
}

/// A floor, a pin and a chain of `LINKS` links from it that falls onto the floor, so the steps have
/// joints and contacts both.
fn chain_over_floor(world: &mut World) -> Result<Vec<BodyHandle>, PhysicsError> {
    body(
        world,
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Vec2::new(20.0, 1.0),
    )?;
    let pin = body(
        world,
        BodyKind::Static,
        Vec2::new(0.0, 3.0),
        Vec2::new(0.1, 0.1),
    )?;
    let mut previous = pin;
    let mut links = Vec::new();
    for i in 0..LINKS {
        let x = 0.5 * f32::from(i) + 0.25;
        let link = body(
            world,
            BodyKind::Dynamic,
            Vec2::new(x, 3.0),
            Vec2::new(0.25, 0.05),
        )?;
        let first_anchor = if i == 0 {
            Vec2::ZERO
        } else {
            Vec2::new(0.25, 0.0)
        };
        world.create_joint(&JointDesc::new(
            previous,
            link,
            Joint2::Revolute(RevoluteJoint2::new(first_anchor, Vec2::new(-0.25, 0.0))),
        ))?;
        links.push(link);
        previous = link;
    }
    Ok(links)
}

fn step_many(world: &mut World, steps: u32) -> Result<(), PhysicsError> {
    for _ in 0..steps {
        world.step(DT)?;
    }
    Ok(())
}

/// Makes five rods between `a` and `b`, steps once and destroys them. The chain uses 12 of 16
/// joints, so four rods fit and the fifth is refused as a value. Returns how many were made.
fn rod_round(world: &mut World, a: BodyHandle, b: BodyHandle) -> Result<u32, PhysicsError> {
    let rod = JointDesc::new(
        a,
        b,
        Joint2::Distance(DistanceJoint2::new(Vec2::ZERO, Vec2::ZERO, 3.0)),
    );
    let mut handles = [None; 5];
    for slot in &mut handles {
        *slot = world.create_joint(&rod).ok();
    }
    world.step(DT)?;
    let mut made = 0;
    for handle in handles.into_iter().flatten() {
        world.destroy_joint(handle)?;
        made += 1;
    }
    Ok(made)
}

#[test]
fn the_counter_sees_an_allocation() {
    let (world, counts) = count_allocations(|| build_world(SleepConfig::DEFAULT));
    assert!(world.is_ok());
    assert!(
        counts.calls() >= 1,
        "world creation allocates; the counter is not installed otherwise"
    );
}

#[test]
fn steps_with_joints_and_contacts_make_no_allocator_call() -> Result<(), PhysicsError> {
    for execution in [
        SolverExecution::SERIAL,
        SolverExecution {
            simd: true,
            ..SolverExecution::SERIAL
        },
    ] {
        let mut world = build_world(SleepConfig::DEFAULT)?;
        chain_over_floor(&mut world)?;
        world.set_execution(execution.clone());
        world.step(DT)?; // warm-up
        let (result, counts) = count_allocations(|| step_many(&mut world, 1500));
        assert_eq!(result, Ok(()));
        assert_eq!(counts.calls(), 0, "{counts:?}");
        assert!(world.solver_stats().joints_solved + world.solver_stats().joints_frozen > 0);
    }
    Ok(())
}

#[test]
fn creating_and_destroying_joints_and_bodies_make_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = build_world(SleepConfig::DEFAULT)?;
    let links = chain_over_floor(&mut world)?;
    world.step(DT)?;
    let a = *links.first().ok_or(PhysicsError::NotFinite)?;
    let b = *links.last().ok_or(PhysicsError::NotFinite)?;
    let (result, counts) = count_allocations(|| {
        let mut made = 0_u32;
        for _ in 0..50 {
            made += rod_round(&mut world, a, b)?;
        }
        Ok::<u32, PhysicsError>(made)
    });
    assert_eq!(result, Ok(200), "four of five fit each round");
    assert_eq!(counts.calls(), 0, "{counts:?}");
    let (hash, counts) = count_allocations(|| {
        let spare = world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(0.0, 8.0),
            Shape2::Circle { radius: 0.2 },
        ));
        if let Ok(spare) = spare {
            let _ = world.destroy_body(spare);
        }
        let _ = world.destroy_body(a);
        world.state_hash()
    });
    assert_ne!(hash, 0);
    assert_eq!(
        counts.calls(),
        0,
        "destroying a body with joints: {counts:?}"
    );
    assert_eq!(
        world.joint_count(),
        10,
        "the first link took its two joints with it"
    );
    Ok(())
}
