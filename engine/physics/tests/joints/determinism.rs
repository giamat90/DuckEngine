//! Determinism of scenes with joints (card 4-16): the same state hash every run, the pinned hashes,
//! and the equality of the scalar serial solver with the lane path and the scheduler paths at any
//! worker count (D0 and D1). The joints are solved serially in every path, so the equality has to
//! hold by construction; these tests are what says it does.

use super::common::{DT, ball, config, ground, pin, plank, rod};
use super::scenes::{bridge, chain, chain_on_ground, hanging_chain};
use base::math::Vec2;
use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::pool::WorkStealingPool;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use jobs::shuffled::ShuffledScheduler;
use physics::d2::{
    AngleLimits, DistanceJoint2, Joint2, JointDesc, LengthLimits, Motor, PhysicsError,
    RevoluteJoint2, STANDARD_GRAVITY, SleepConfig, SolverExecution, Spring, World,
};
use platform::threads::StdThreads;
use std::sync::Arc;

/// A chunk size small enough to cut the colours of the contact scenes into several chunks.
const SMALL_CHUNK: u32 = 4;

/// A whip of 20 links from a pin, sleeping off.
fn whip() -> Result<World, PhysicsError> {
    let mut world = World::new(config(32, 32, STANDARD_GRAVITY, SleepConfig::DISABLED))?;
    let anchor = pin(&mut world, 0.0, 10.0)?;
    chain(&mut world, anchor, Vec2::new(0.0, 10.0), 20, 1.0)?;
    Ok(world)
}

/// A bridge that a heavy box and a ball are dropped on.
fn loaded_bridge() -> Result<World, PhysicsError> {
    let mut world = World::new(config(48, 32, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    bridge(&mut world, 8.0, 5.0, 16, 1.0)?;
    plank(&mut world, Vec2::new(3.5, 6.5), Vec2::new(0.3, 0.3), 2.0)?;
    ball(&mut world, 0.2, Vec2::new(4.6, 7.5))?;
    Ok(world)
}

/// Two chains sliding and lying on the ground among loose crates, sleeping on.
fn ground_chains() -> Result<World, PhysicsError> {
    let mut world = World::new(config(64, 32, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    ground(&mut world)?;
    let sliding = chain_on_ground(&mut world, -6.0, 10, 0.0)?;
    let first = *sliding.links.first().ok_or(PhysicsError::NotFinite)?;
    world.set_linear_velocity(first, Vec2::new(4.0, 0.0))?;
    chain_on_ground(&mut world, 2.0, 8, 0.2)?;
    for i in 0..6_u8 {
        plank(
            &mut world,
            Vec2::new(-1.0 + 0.4 * f32::from(i), 1.0 + 0.3 * f32::from(i)),
            Vec2::new(0.2, 0.2),
            1.0,
        )?;
    }
    Ok(world)
}

/// Every kind of joint at once: a limited, motorised hinge, a rigid rod, a spring, a rope with a
/// minimum, and a hanging chain, over a floor with a ball in the way.
fn every_joint() -> Result<World, PhysicsError> {
    let mut world = World::new(config(48, 32, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    ground(&mut world)?;
    let pivot = pin(&mut world, -5.0, 6.0)?;
    let bar = plank(&mut world, Vec2::new(-4.0, 6.0), Vec2::new(1.0, 0.1), 1.0)?;
    world.create_joint(&JointDesc::new(
        pivot,
        bar,
        Joint2::Revolute(RevoluteJoint2 {
            limits: Some(AngleLimits {
                lower: -0.8,
                upper: 0.4,
            }),
            motor: Some(Motor {
                max_torque: 2.0,
                speed: -1.0,
            }),
            ..RevoluteJoint2::new(Vec2::ZERO, Vec2::new(-1.0, 0.0))
        }),
    ))?;
    let anchor = pin(&mut world, 0.0, 6.0)?;
    let bob = ball(&mut world, 0.3, Vec2::new(1.5, 5.0))?;
    rod(&mut world, anchor, bob, (Vec2::ZERO, Vec2::ZERO), 1.8)?;
    let heavy = plank(&mut world, Vec2::new(3.0, 6.0), Vec2::new(0.3, 0.3), 3.0)?;
    let spring = Spring {
        hertz: 1.5,
        damping_ratio: 0.2,
    };
    world.create_joint(&JointDesc::new(
        anchor,
        heavy,
        Joint2::Distance(DistanceJoint2 {
            spring: Some(spring),
            limits: Some(LengthLimits { min: 2.0, max: 4.0 }),
            ..DistanceJoint2::new(Vec2::ZERO, Vec2::ZERO, 3.0)
        }),
    ))?;
    let rope_end = ball(&mut world, 0.2, Vec2::new(6.0, 5.0))?;
    let rope_pin = pin(&mut world, 5.0, 7.0)?;
    world.create_joint(&JointDesc::new(
        rope_pin,
        rope_end,
        Joint2::Distance(DistanceJoint2 {
            spring: Some(Spring {
                hertz: 0.0,
                damping_ratio: 0.0,
            }),
            limits: Some(LengthLimits { min: 0.5, max: 2.0 }),
            ..DistanceJoint2::new(Vec2::ZERO, Vec2::ZERO, 1.0)
        }),
    ))?;
    let hook = pin(&mut world, 8.0, 7.0)?;
    hanging_chain(&mut world, hook, Vec2::new(8.0, 7.0), 4)?;
    Ok(world)
}

type Scene = (&'static str, fn() -> Result<World, PhysicsError>, u32);

const SCENES: [Scene; 4] = [
    ("whip", whip, 120),
    ("bridge", loaded_bridge, 150),
    ("ground chains", ground_chains, 400),
    ("every joint", every_joint, 200),
];

/// The hash after every step of `steps` steps of `scene` run with `execution`.
fn run(scene: &Scene, execution: SolverExecution) -> Result<Vec<u64>, PhysicsError> {
    let mut world = (scene.1)()?;
    world.set_execution(execution);
    let mut seen = Vec::new();
    for _ in 0..scene.2 {
        world.step(DT)?;
        seen.push(world.state_hash());
    }
    Ok(seen)
}

fn reference(scene: &Scene) -> Result<Vec<u64>, PhysicsError> {
    run(scene, SolverExecution::SERIAL)
}

fn scheduled(scheduler: Arc<dyn IScheduler>, simd: bool, chunk: u32) -> SolverExecution {
    SolverExecution {
        scheduler: Some(scheduler),
        simd,
        chunk,
    }
}

fn pool(workers: u32) -> Result<Arc<dyn IScheduler>, PhysicsError> {
    WorkStealingPool::new(workers, &StdThreads::new())
        .map(|pool| Arc::new(pool) as Arc<dyn IScheduler>)
        .map_err(|_| PhysicsError::NotFinite)
}

/// The scenes would prove little if the joints did nothing or the contacts never reached the
/// scheduler.
#[test]
fn the_scenes_have_the_shape_their_names_promise() -> Result<(), PhysicsError> {
    for scene in &SCENES {
        let mut world = (scene.1)()?;
        for _ in 0..30 {
            world.step(DT)?;
        }
        let stats = world.solver_stats();
        assert!(stats.joints_solved > 0, "{}: {stats:?}", scene.0);
    }
    let mut world = ground_chains()?;
    for _ in 0..60 {
        world.step(DT)?;
    }
    assert!(
        world.solver_stats().colours > 1,
        "the crates make contact colours"
    );
    let mut asleep = ground_chains()?;
    for _ in 0..400 {
        asleep.step(DT)?;
    }
    assert!(asleep.sleeping_count() > 0, "part of the scene sleeps");
    Ok(())
}

#[test]
fn a_scene_with_joints_hashes_the_same_in_every_run() -> Result<(), PhysicsError> {
    for scene in &SCENES {
        assert_eq!(reference(scene)?, reference(scene)?, "{}", scene.0);
    }
    Ok(())
}

/// The hash after the last step of each scene, measured on Windows and on Linux (WSL) with the
/// feature `cross-platform-determinism` off and on; the four values were equal in all four runs.
/// The pin of the scene "every joint" (limits, so it reads angles with `atan2`) is checked only with
/// the feature on, because only then is the angle function promised equal on every platform; the
/// other three scenes are checked in every build.
/// The scenes are in `SCENES` order.
const PINS: [u64; 4] = [
    0xf287_23c7_08dd_05fd,
    0x81be_1713_8228_246b,
    0xcefa_7f28_27cf_42d3,
    0x9113_0d63_eeba_7c18,
];

#[test]
fn the_joint_scenes_have_pinned_hashes() -> Result<(), PhysicsError> {
    for (scene, expected) in SCENES.iter().zip(PINS) {
        // The limited hinge reads its angle with `atan2`, which is the platform's function until
        // the feature `cross-platform-determinism` routes it through the facade: only then is the
        // hash promised to be the same on every platform.
        if scene.0 == "every joint" && !cfg!(feature = "cross-platform-determinism") {
            continue;
        }
        let last = reference(scene)?
            .last()
            .copied()
            .ok_or(PhysicsError::NotFinite)?;
        assert_eq!(last, expected, "{}: {last:#018x}", scene.0);
    }
    Ok(())
}

#[test]
fn the_lane_path_equals_the_scalar_path_bit_for_bit() -> Result<(), PhysicsError> {
    for scene in &SCENES {
        let lanes = SolverExecution {
            simd: true,
            ..SolverExecution::SERIAL
        };
        assert_eq!(run(scene, lanes)?, reference(scene)?, "{}", scene.0);
    }
    Ok(())
}

#[test]
fn the_staged_path_without_a_scheduler_equals_the_reference() -> Result<(), PhysicsError> {
    for scene in &SCENES {
        for simd in [false, true] {
            let staged = scheduled(Arc::new(InlineScheduler), simd, SMALL_CHUNK);
            assert_eq!(
                run(scene, staged)?,
                reference(scene)?,
                "{} simd {simd}",
                scene.0
            );
        }
    }
    Ok(())
}

fn check_pool(workers: u32) -> Result<(), PhysicsError> {
    let scheduler = pool(workers)?;
    for scene in &SCENES {
        let expected = reference(scene)?;
        for simd in [false, true] {
            let execution = scheduled(Arc::clone(&scheduler), simd, SMALL_CHUNK);
            assert_eq!(
                run(scene, execution)?,
                expected,
                "{} workers {workers} simd {simd}",
                scene.0
            );
        }
    }
    Ok(())
}

#[test]
fn a_pool_of_one_worker_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(1)
}

#[test]
fn a_pool_of_two_workers_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(2)
}

#[test]
fn a_pool_of_four_workers_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(4)
}

#[test]
fn a_pool_of_six_workers_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(6)
}

/// Asserts that `scene` through `scheduler` equals `expected` at every chunk size.
fn check_chunks(
    scene: &Scene,
    scheduler: &Arc<dyn IScheduler>,
    expected: &[u64],
) -> Result<(), PhysicsError> {
    for chunk in [1, SMALL_CHUNK, 1000] {
        let execution = scheduled(Arc::clone(scheduler), true, chunk);
        assert_eq!(
            run(scene, execution)?,
            expected,
            "{} chunk {chunk}",
            scene.0
        );
    }
    Ok(())
}

#[test]
fn the_chunk_size_the_order_and_the_grouping_do_not_change_a_bit() -> Result<(), PhysicsError> {
    let schedulers: [Arc<dyn IScheduler>; 4] = [
        Arc::new(ReversedScheduler),
        Arc::new(ShuffledScheduler::new(1, 4096)),
        Arc::new(ShuffledScheduler::new(99, 4096)),
        Arc::new(OneRange),
    ];
    for scene in &SCENES {
        let expected = reference(scene)?;
        for scheduler in &schedulers {
            check_chunks(scene, scheduler, &expected)?;
        }
    }
    Ok(())
}

#[test]
fn a_scheduler_that_refuses_leaves_the_result_unchanged() -> Result<(), PhysicsError> {
    for scene in &SCENES {
        let execution = scheduled(Arc::new(Refusing), true, SMALL_CHUNK);
        assert_eq!(run(scene, execution)?, reference(scene)?, "{}", scene.0);
    }
    Ok(())
}

/// Calls the body once with the whole range: the coarsest grouping a scheduler may make.
struct OneRange;

impl IScheduler for OneRange {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        if grain == 0 {
            return Err(JobError::InvalidGrain);
        }
        if count > 0 {
            body(0..count);
        }
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        for task in tasks {
            task();
        }
        Ok(())
    }
}

/// A scheduler that refuses every loop, so the solver must fall back to the calling thread.
struct Refusing;

impl IScheduler for Refusing {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, _: u32, _: u32, _: &RangeBody<'_>) -> Result<(), JobError> {
        Err(JobError::Busy)
    }

    fn run_scope(&self, _: &[&Task<'_>]) -> Result<(), JobError> {
        Err(JobError::Busy)
    }
}

/// A zero-gravity world with two resting balls and nothing else to hash.
fn two_balls(jointed: bool) -> Result<World, PhysicsError> {
    let mut world = World::new(config(8, 8, Vec2::ZERO, SleepConfig::DISABLED))?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let at_rest = ball(&mut world, 0.25, Vec2::new(1.0, 0.0))?;
    if jointed {
        rod(&mut world, anchor, at_rest, (Vec2::ZERO, Vec2::ZERO), 1.0)?;
    }
    Ok(world)
}

#[test]
fn the_state_hash_covers_the_joints_and_a_world_without_them_does_not_change()
-> Result<(), PhysicsError> {
    let mut plain = two_balls(false)?;
    let mut jointed = two_balls(true)?;
    // Before the first step the bodies hash alike and no joint impulse exists yet.
    assert_eq!(plain.state_hash(), jointed.state_hash());
    plain.step(DT)?;
    jointed.step(DT)?;
    // The same bodies at rest, one joint apart: only the joint record can tell them.
    assert_ne!(plain.state_hash(), jointed.state_hash());
    // A world that once had a joint and lost it hashes as one that never had it.
    let mut lost = two_balls(true)?;
    lost.step(DT)?;
    let handle = lost
        .joints()
        .first()
        .ok_or(PhysicsError::StaleJoint)?
        .handle;
    lost.destroy_joint(handle)?;
    lost.step(DT)?;
    plain.step(DT)?;
    assert_eq!(plain.state_hash(), lost.state_hash());
    Ok(())
}

#[test]
fn a_refused_step_with_a_limited_motorised_hinge_changes_nothing() -> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 8, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    let anchor = pin(&mut world, 0.0, 5.0)?;
    let bar = plank(&mut world, Vec2::new(1.0, 5.0), Vec2::new(1.0, 0.1), 1.0)?;
    world.create_joint(&JointDesc::new(
        anchor,
        bar,
        Joint2::Revolute(RevoluteJoint2 {
            limits: Some(AngleLimits {
                lower: -1.0,
                upper: 1.0,
            }),
            motor: Some(Motor {
                max_torque: 1.0,
                speed: 1.0,
            }),
            ..RevoluteJoint2::new(Vec2::ZERO, Vec2::new(-1.0, 0.0))
        }),
    ))?;
    for _ in 0..10 {
        world.step(DT)?;
    }
    world.set_linear_velocity(bar, Vec2::new(0.0, -3.0e38))?;
    let hash = world.state_hash();
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.state_hash(), hash);
    Ok(())
}

#[test]
fn a_refused_step_with_joints_changes_nothing() -> Result<(), PhysicsError> {
    let mut world = World::new(config(8, 8, STANDARD_GRAVITY, SleepConfig::DEFAULT))?;
    let anchor = pin(&mut world, 0.0, 5.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(1.0, 5.0))?;
    rod(&mut world, anchor, bob, (Vec2::ZERO, Vec2::ZERO), 1.0)?;
    for _ in 0..10 {
        world.step(DT)?;
    }
    let before = world.state_hash();
    let steps = world.step_count();
    world.set_linear_velocity(bob, Vec2::new(0.0, -3.0e38))?;
    let hash_with_velocity = world.state_hash();
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.state_hash(), hash_with_velocity);
    assert_eq!(world.step_count(), steps);
    assert_ne!(before, hash_with_velocity);
    Ok(())
}
