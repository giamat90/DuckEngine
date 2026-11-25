//! Card 8-50: the opt-in speed clamp. By default a step that would give a body a speed beyond
//! `MAX_SPEED` or `MAX_SPIN` is refused as `Diverged`; with `SpeedPolicy::Clamp` the step instead
//! brings every dynamic body back to the limits you set (direction kept) and goes on, as the
//! engines that clamp do. The scenes are the ones that card 8-47 and 8-48 found refused.

use base::math::{Vec2, Vec3};
use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use physics::core::PhysicsError;
use physics::{d2, d3};
use platform::threads::StdThreads;
use std::sync::Arc;

const DT: f32 = 1.0 / 60.0;
/// Limits far below the bounds, in the range of other engines (500 m/s, about 50 rad/s).
const LINEAR: f32 = 500.0;
const ANGULAR: f32 = 50.0;
const CLAMP: d2::SpeedPolicy = d2::SpeedPolicy::Clamp {
    linear: LINEAR,
    angular: ANGULAR,
};

fn world2() -> Result<d2::World, PhysicsError> {
    d2::World::new(d2::WorldConfig {
        max_bodies: 4,
        gravity: Vec2::ZERO,
        max_pairs: 16,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })
}

fn world3() -> Result<d3::World, PhysicsError> {
    d3::World::new(d3::WorldConfig {
        max_bodies: 4,
        gravity: Vec3::ZERO,
        max_pairs: 16,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })
}

/// A light and a 1000 times heavier elastic ball overlapping and going at each other at the
/// largest given speed: refused by default (card 8-47).
fn head_on2() -> Result<d2::World, PhysicsError> {
    let mut world = world2()?;
    for (x, velocity, density) in [(-0.45, 1.0, 1.0), (0.45, -1.0, 1.0e3)] {
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(velocity * d2::MAX_GIVEN_SPEED, 0.0),
            density,
            restitution: 1.0,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(x, 0.0),
                d2::Shape2::Circle { radius: 0.5 },
            )
        })?;
    }
    Ok(world)
}

fn head_on3() -> Result<d3::World, PhysicsError> {
    let mut world = world3()?;
    for (x, velocity, density) in [(-0.45, 1.0, 1.0), (0.45, -1.0, 1.0e3)] {
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(velocity * d3::MAX_GIVEN_SPEED, 0.0, 0.0),
            density,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(x, 0.0, 0.0),
                d3::Shape3::Sphere { radius: 0.5 },
            )
        })?;
    }
    Ok(world)
}

#[test]
fn a_world_refuses_by_default() -> Result<(), PhysicsError> {
    assert_eq!(world2()?.speed_policy(), d2::SpeedPolicy::Refuse);
    assert_eq!(world3()?.speed_policy(), d3::SpeedPolicy::Refuse);
    assert_eq!(head_on2()?.step(DT), Err(PhysicsError::Diverged));
    assert_eq!(head_on3()?.step(DT), Err(PhysicsError::Diverged));
    Ok(())
}

#[test]
fn limits_that_are_not_positive_finite_or_above_the_given_ceiling_are_refused_and_change_nothing()
-> Result<(), PhysicsError> {
    let mut world = world2()?;
    let bad = [
        (0.0, ANGULAR),
        (LINEAR, 0.0),
        (-1.0, ANGULAR),
        (LINEAR, -1.0),
        (f32::NAN, ANGULAR),
        (LINEAR, f32::INFINITY),
        (d2::MAX_GIVEN_SPEED * 1.01, ANGULAR),
        (LINEAR, d2::MAX_GIVEN_SPIN * 1.01),
    ];
    for (linear, angular) in bad {
        let policy = d2::SpeedPolicy::Clamp { linear, angular };
        assert_eq!(
            world.set_speed_policy(policy),
            Err(PhysicsError::InvalidSpeedLimit),
            "{linear} {angular}"
        );
        assert_eq!(world.speed_policy(), d2::SpeedPolicy::Refuse);
    }
    let ceiling = d2::SpeedPolicy::Clamp {
        linear: d2::MAX_GIVEN_SPEED,
        angular: d2::MAX_GIVEN_SPIN,
    };
    assert_eq!(world.set_speed_policy(ceiling), Ok(()));
    assert_eq!(world.speed_policy(), ceiling);
    assert_eq!(world.set_speed_policy(d2::SpeedPolicy::Refuse), Ok(()));
    Ok(())
}

#[test]
fn a_clamping_world_steps_the_collision_that_is_refused_by_default_in_3d()
-> Result<(), PhysicsError> {
    let mut world = head_on3()?;
    world.set_speed_policy(d3::SpeedPolicy::Clamp {
        linear: LINEAR,
        angular: ANGULAR,
    })?;
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.clamped_bodies() > 0);
    for (_, body) in world.iter() {
        assert!(body.linear_velocity().length() <= LINEAR * 1.0001);
    }
    Ok(())
}

#[test]
fn a_clamping_world_steps_the_collision_that_is_refused_by_default_in_2d()
-> Result<(), PhysicsError> {
    let mut world = head_on2()?;
    world.set_speed_policy(CLAMP)?;
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.clamped_bodies() > 0);
    for (_, body) in world.iter() {
        assert!(body.linear_velocity().length() <= LINEAR * 1.0001);
        assert!(body.angular_velocity().abs() <= ANGULAR * 1.0001);
    }
    Ok(())
}

#[test]
fn the_clamp_keeps_the_direction_of_the_velocity() -> Result<(), PhysicsError> {
    let mut world = world2()?;
    world.set_speed_policy(CLAMP)?;
    let ball = world.create_body(&d2::BodyDesc {
        linear_velocity: Vec2::new(-3000.0, 4000.0),
        angular_velocity: -400.0,
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::ZERO,
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    assert_eq!(world.step(DT), Ok(()));
    let velocity = world.body(ball)?.linear_velocity();
    assert!((velocity.x - -300.0).abs() < 0.01, "{velocity:?}");
    assert!((velocity.y - 400.0).abs() < 0.01, "{velocity:?}");
    assert!((world.body(ball)?.angular_velocity() - -ANGULAR).abs() < 0.001);
    assert_eq!(world.clamped_bodies(), 1);
    Ok(())
}

#[test]
fn a_body_within_the_limits_is_not_touched_and_is_not_counted() -> Result<(), PhysicsError> {
    let run = |policy: d2::SpeedPolicy| -> Result<(u64, usize), PhysicsError> {
        let mut world = world2()?;
        world.set_speed_policy(policy)?;
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(100.0, -20.0),
            angular_velocity: 3.0,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::ZERO,
                d2::Shape2::Circle { radius: 0.5 },
            )
        })?;
        for _ in 0..30 {
            world.step(DT)?;
        }
        let bits = world
            .iter()
            .map(|(_, body)| u64::from(body.linear_velocity().x.to_bits()))
            .sum();
        Ok((bits, world.clamped_bodies()))
    };
    let refusing = run(d2::SpeedPolicy::Refuse)?;
    let clamping = run(CLAMP)?;
    assert_eq!(refusing, clamping);
    assert_eq!(clamping.1, 0);
    Ok(())
}

#[test]
fn the_count_is_of_the_last_step_and_a_refused_step_keeps_it() -> Result<(), PhysicsError> {
    let mut world = world2()?;
    world.set_speed_policy(CLAMP)?;
    world.create_body(&d2::BodyDesc {
        linear_velocity: Vec2::new(2000.0, 0.0),
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::ZERO,
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.clamped_bodies(), 1);
    // Already inside the limit: the next step clamps nothing.
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.clamped_bodies(), 0);
    Ok(())
}

#[test]
fn a_value_that_is_not_finite_is_still_refused_when_clamping() -> Result<(), PhysicsError> {
    let mut world = world2()?;
    world.set_speed_policy(CLAMP)?;
    world.create_body(&d2::BodyDesc {
        linear_velocity: Vec2::new(d2::MAX_GIVEN_SPEED, 0.0),
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::new(f32::MAX / 2.0, 0.0),
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    let hash = world.state_hash();
    assert_eq!(world.step(1.0e35), Err(PhysicsError::NotFinite));
    assert_eq!(world.state_hash(), hash);
    Ok(())
}

#[test]
fn a_clamping_world_gives_the_same_state_every_time() -> Result<(), PhysicsError> {
    let run = || -> Result<u64, PhysicsError> {
        let mut world = head_on3()?;
        world.set_speed_policy(d3::SpeedPolicy::Clamp {
            linear: LINEAR,
            angular: ANGULAR,
        })?;
        for _ in 0..120 {
            world.step(DT)?;
        }
        Ok(world.state_hash())
    };
    assert_eq!(run()?, run()?);
    Ok(())
}

#[test]
fn the_policy_is_part_of_the_state_hash_only_when_it_clamps() -> Result<(), PhysicsError> {
    let plain = world2()?.state_hash();
    let mut world = world2()?;
    world.set_speed_policy(CLAMP)?;
    assert_ne!(world.state_hash(), plain);
    world.set_speed_policy(d2::SpeedPolicy::Refuse)?;
    assert_eq!(world.state_hash(), plain);
    Ok(())
}

/// Twenty-four circles thrown at a floor at 2000 m/s (far above the clamp), so the clamp fires on
/// every body in the first steps and the contacts are many: the state after every step.
fn thrown_hashes(execution: d2::SolverExecution) -> Result<(Vec<u64>, usize), PhysicsError> {
    let mut world = d2::World::new(d2::WorldConfig {
        max_bodies: 32,
        gravity: d2::STANDARD_GRAVITY,
        max_pairs: 512,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 512,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })?;
    world.set_execution(execution);
    world.set_speed_policy(CLAMP)?;
    world.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::new(0.0, -1.0),
        d2::Shape2::Box {
            half_extents: Vec2::new(20.0, 1.0),
        },
    ))?;
    for index in 0..24_u8 {
        let (column, row) = (f32::from(index % 6), f32::from(index / 6));
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(0.0, -2000.0),
            restitution: 0.5,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(column * 1.2 - 3.0, 1.0 + row * 1.2),
                d2::Shape2::Circle { radius: 0.5 },
            )
        })?;
    }
    let (mut hashes, mut clamped) = (Vec::new(), 0);
    for _ in 0..60 {
        world.step(DT)?;
        hashes.push(world.state_hash());
        clamped += world.clamped_bodies();
    }
    Ok((hashes, clamped))
}

#[test]
fn a_clamping_world_gives_the_same_state_at_every_worker_count() -> Result<(), PhysicsError> {
    let (reference, clamped) = thrown_hashes(d2::SolverExecution::SERIAL)?;
    assert!(clamped >= 24, "the clamp must have fired: {clamped}");
    for workers in [1, 2, 4, 6] {
        let pool = WorkStealingPool::new(workers, &StdThreads::new())
            .map(|pool| Arc::new(pool) as Arc<dyn IScheduler>)
            .map_err(|_| PhysicsError::NotFinite)?;
        let execution = d2::SolverExecution {
            scheduler: Some(pool),
            simd: false,
            chunk: 4,
        };
        assert_eq!(thrown_hashes(execution)?.0, reference, "{workers} workers");
    }
    Ok(())
}

#[test]
fn the_angular_clamp_works_on_the_vector_of_3d_and_keeps_its_axis() -> Result<(), PhysicsError> {
    let mut world = world3()?;
    world.set_speed_policy(d3::SpeedPolicy::Clamp {
        linear: LINEAR,
        angular: ANGULAR,
    })?;
    let ball = world.create_body(&d3::BodyDesc {
        angular_velocity: Vec3::new(0.0, 300.0, 400.0),
        ..d3::BodyDesc::new(
            d3::BodyKind::Dynamic,
            Vec3::ZERO,
            d3::Shape3::Sphere { radius: 0.5 },
        )
    })?;
    assert_eq!(world.step(DT), Ok(()));
    let spin = world.body(ball)?.angular_velocity();
    assert!(spin.x.abs() < 1.0e-3, "{spin:?}");
    assert!((spin.y - 30.0).abs() < 0.01, "{spin:?}");
    assert!((spin.z - 40.0).abs() < 0.01, "{spin:?}");
    assert_eq!(world.clamped_bodies(), 1);
    Ok(())
}

#[test]
fn a_refused_step_leaves_the_clamp_count_and_the_state_as_they_were() -> Result<(), PhysicsError> {
    let mut world = world2()?;
    world.set_speed_policy(CLAMP)?;
    world.create_body(&d2::BodyDesc {
        linear_velocity: Vec2::new(2000.0, 0.0),
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::ZERO,
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.clamped_bodies(), 1);
    // The next step carries the body past the largest float: refused, and the count stays.
    let hash = world.state_hash();
    assert_eq!(world.step(1.0e38), Err(PhysicsError::NotFinite));
    assert_eq!(world.clamped_bodies(), 1);
    assert_eq!(world.state_hash(), hash);
    Ok(())
}

#[test]
fn setting_the_policy_on_a_world_that_already_holds_a_fast_body_acts_from_the_next_step()
-> Result<(), PhysicsError> {
    let mut world = world2()?;
    let ball = world.create_body(&d2::BodyDesc {
        linear_velocity: Vec2::new(3000.0, 0.0),
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::ZERO,
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    world.set_speed_policy(CLAMP)?;
    assert!((world.body(ball)?.linear_velocity().x - 3000.0).abs() < 0.01);
    assert_eq!(world.step(DT), Ok(()));
    assert!((world.body(ball)?.linear_velocity().x - LINEAR).abs() < 0.01);
    Ok(())
}
