//! After `World::new`, nothing the world does allocates: steps, body creation and removal within
//! the capacity, lookups, setters, iteration and the state hash make no allocator call on the
//! calling thread (counting allocator with per-thread counters, as the jobs and render tests do).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    PhysicsError, Rot2, STANDARD_GRAVITY, Shape2, SolverConfig, World, WorldConfig,
};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CAPACITY: u32 = 64;
const SLOTS: usize = 64;
const DT: f32 = 1.0 / 60.0;

fn build_world() -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: CAPACITY,
        gravity: STANDARD_GRAVITY,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DEFAULT,
    })
}

fn box_desc(x: f32, y: f32) -> BodyDesc {
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Box {
            half_extents: Vec2::new(0.5, 0.25),
        },
    );
    desc.rotation = Rot2::from_angle(0.3);
    desc.angular_velocity = 1.0;
    desc.linear_damping = 0.1;
    desc
}

fn fill(world: &mut World) -> Result<Vec<BodyHandle>, PhysicsError> {
    (0..CAPACITY)
        .map(|i| world.create_body(&box_desc(f32::from(u8::try_from(i).unwrap_or(0)), 10.0)))
        .collect()
}

#[test]
fn the_counter_sees_an_allocation() {
    let (world, counts) = count_allocations(build_world);
    assert!(world.is_ok());
    assert!(
        counts.calls() >= 1,
        "world creation allocates; if the counter reads zero it is not installed"
    );
}

#[test]
fn a_step_in_steady_state_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = build_world()?;
    fill(&mut world)?;
    world.step(DT)?; // warm-up
    let (result, counts) = count_allocations(|| {
        for _ in 0..1000 {
            world.step(DT)?;
        }
        Ok::<(), PhysicsError>(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}

/// One round of everything the world offers, on a full world: create to the capacity, set, push,
/// read, step, hash, iterate, hit the capacity, destroy all. Handles are kept in an array, so the
/// test itself allocates nothing either.
fn churn(world: &mut World, handles: &mut [Option<BodyHandle>]) -> Result<u64, PhysicsError> {
    let desc = box_desc(0.0, 5.0);
    let mut sum = 0_u64;
    for (i, slot) in handles.iter_mut().enumerate() {
        let handle = world.create_body(&desc)?;
        world.set_linear_velocity(
            handle,
            Vec2::new(1.0, f32::from(u8::try_from(i).unwrap_or(0))),
        )?;
        world.set_angular_velocity(handle, 2.0)?;
        world.apply_force(handle, Vec2::new(0.0, 3.0))?;
        world.apply_torque(handle, 4.0)?;
        sum = sum.wrapping_add(u64::from(world.body(handle)?.position().x.to_bits()));
        *slot = Some(handle);
    }
    world.step(DT)?;
    sum = sum.wrapping_add(world.state_hash());
    sum = sum.wrapping_add(world.iter().count() as u64);
    assert_eq!(
        world.create_body(&desc).err(),
        Some(PhysicsError::CapacityExhausted)
    );
    for slot in handles.iter_mut() {
        if let Some(handle) = slot.take() {
            world.destroy_body(handle)?;
        }
    }
    Ok(sum)
}

#[test]
fn creating_destroying_stepping_hashing_and_querying_make_no_allocator_call()
-> Result<(), PhysicsError> {
    let mut world = build_world()?;
    let mut handles = [None; SLOTS];
    churn(&mut world, &mut handles)?; // warm-up
    let (result, counts) = count_allocations(|| {
        let mut sum = 0_u64;
        for _ in 0..20 {
            sum = sum.wrapping_add(churn(&mut world, &mut handles)?);
        }
        Ok::<u64, PhysicsError>(sum)
    });
    assert!(result.is_ok());
    assert_eq!(counts.calls(), 0, "{counts:?}");
    assert!(world.is_empty());
    Ok(())
}

/// Reads the stored box of the first body of the first pair, when there is a pair.
fn first_pair_bounds(world: &World) -> Result<(), PhysicsError> {
    match world.pairs().as_slice().first() {
        Some(pair) => world.broadphase_bounds(pair.first).map(drop),
        None => Ok(()),
    }
}

#[test]
fn moving_bodies_in_the_broadphase_finding_pairs_and_overflowing_the_list_allocate_nothing()
-> Result<(), PhysicsError> {
    // A list of 8 pairs is smaller than the pairs of a row of 64 touching boxes, so the overflow
    // path runs on every step.
    let mut world = World::new(WorldConfig {
        max_bodies: CAPACITY,
        gravity: STANDARD_GRAVITY,
        max_pairs: 8,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DEFAULT,
    })?;
    let handles = fill(&mut world)?;
    // Every body moves sideways so each one leaves its fat box about every other step and is
    // inserted again.
    for (i, handle) in handles.iter().enumerate() {
        let direction = if i % 2 == 0 { 3.0 } else { -3.0 };
        world.set_linear_velocity(*handle, Vec2::new(direction, 0.0))?;
    }
    world.step(DT)?; // warm-up
    let (result, counts) = count_allocations(|| {
        let mut changes = 0_u32;
        let mut last = world.broadphase_hash();
        for _ in 0..500 {
            world.step(DT)?;
            world.find_pairs();
            let now = world.broadphase_hash();
            changes += u32::from(now != last);
            last = now;
            first_pair_bounds(&world)?;
        }
        Ok::<u32, PhysicsError>(changes)
    });
    assert!(matches!(result, Ok(changes) if changes > 100), "{result:?}");
    assert!(!world.pairs().is_empty() && !world.pairs().is_complete());
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}

#[test]
fn finding_contacts_and_overflowing_the_manifold_list_allocate_nothing() -> Result<(), PhysicsError>
{
    // Eight manifolds fit, a row of 64 overlapping boxes makes far more, so the overflow path runs
    // on every step; the boxes also fall onto a floor, so contacts of every kind come and go.
    let mut world = World::new(WorldConfig {
        max_bodies: CAPACITY + 1,
        gravity: STANDARD_GRAVITY,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 8,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DEFAULT,
    })?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, 8.0),
        Shape2::Box {
            half_extents: Vec2::new(100.0, 1.0),
        },
    ))?;
    fill(&mut world)?;
    world.step(DT)?; // warm-up
    let (result, counts) = count_allocations(|| {
        let (mut contacts, mut overflowed) = (0, false);
        for _ in 0..300 {
            world.step(DT)?;
            world.collide();
            contacts += world.manifolds().len();
            overflowed |= !world.manifolds().is_complete();
        }
        Ok::<(usize, bool), PhysicsError>((contacts, overflowed))
    });
    assert!(
        matches!(result, Ok((contacts, true)) if contacts > 100),
        "{result:?}"
    );
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}

/// A pile of boxes on a floor: contacts exist, the solver runs every step, warm starting is used.
#[test]
fn a_step_with_contacts_and_the_solver_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = build_world()?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -0.5),
        Shape2::Box {
            half_extents: Vec2::new(20.0, 0.5),
        },
    ))?;
    for level in 0..6_u8 {
        for column in 0..3_u8 {
            world.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(f32::from(column) * 1.05, 0.5 + f32::from(level) * 1.01),
                Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            ))?;
        }
    }
    for _ in 0..30 {
        world.step(DT)?; // warm-up: contacts form, boxes settle
    }
    assert!(
        !world.manifolds().as_slice().is_empty(),
        "the scene has contacts"
    );
    let (result, counts) = count_allocations(|| {
        for _ in 0..300 {
            world.step(DT)?;
        }
        Ok::<(), PhysicsError>(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}

/// A pyramid that falls asleep, is woken by a call and by a ball, falls asleep again; the islands,
/// the sleep passes, the colouring and the frozen constraints all run inside the counted section.
#[test]
fn sleeping_waking_islands_and_colouring_make_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = build_world()?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -0.5),
        Shape2::Box {
            half_extents: Vec2::new(20.0, 0.5),
        },
    ))?;
    let mut bottom = None;
    for level in 0..5_u8 {
        for column in 0..3_u8 {
            let handle = world.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(f32::from(column) * 1.0, 0.5 + f32::from(level) * 1.0),
                Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            ))?;
            bottom = bottom.or(Some(handle));
        }
    }
    let bottom = bottom.ok_or(PhysicsError::StaleHandle)?;
    let ball = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(1.0, 12.0),
        Shape2::Circle { radius: 0.25 },
    );
    world.step(DT)?; // warm-up
    let (result, counts) = count_allocations(|| {
        let (mut asleep_steps, mut woken) = (0_u32, 0_u32);
        for step in 0..900_u32 {
            act_on(&mut world, bottom, &ball, step)?;
            world.step(DT)?;
            let sleeping = world.sleeping_count();
            asleep_steps += u32::from(sleeping > 0);
            woken += u32::from(step == 201 && sleeping == 0);
            let _ = (world.island_count(), world.solver_stats());
        }
        Ok::<(u32, u32), PhysicsError>((asleep_steps, woken))
    });
    assert!(
        matches!(result, Ok((asleep, 1)) if asleep > 300),
        "{result:?}"
    );
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}

/// The two calls of the sleeping test: an explicit wake at step 200, a new body at step 400.
fn act_on(
    world: &mut World,
    bottom: BodyHandle,
    ball: &BodyDesc,
    step: u32,
) -> Result<(), PhysicsError> {
    match step {
        200 => world.wake(bottom),
        400 => world.create_body(ball).map(|_| ()),
        _ => Ok(()),
    }
}

/// A body with more contacts than colours: the overflow set is solved on every step.
#[test]
fn the_colour_overflow_set_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 64,
        gravity: Vec2::ZERO,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DISABLED,
    })?;
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::ZERO,
        Shape2::Circle { radius: 1.0 },
    ))?;
    for k in 0..30_u8 {
        let spoke = Rot2::from_angle(f32::from(k) * std::f32::consts::TAU / 30.0)
            .rotate(Vec2::new(1.1, 0.0));
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            spoke,
            Shape2::Circle { radius: 0.1 },
        ))?;
    }
    world.step(DT)?; // warm-up
    let (result, counts) = count_allocations(|| {
        let mut overflow = 0;
        for _ in 0..200 {
            world.step(DT)?;
            overflow = world.solver_stats().overflow;
        }
        Ok::<usize, PhysicsError>(overflow)
    });
    assert_eq!(result, Ok(6));
    assert_eq!(counts.calls(), 0, "{counts:?}");
    Ok(())
}
