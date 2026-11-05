//! Islands and sleeping through the public 2D API (card 4-14): islands are the sets of dynamic bodies
//! joined by manifolds (static bodies join nothing), their ids are canonical, a settled island
//! sleeps and does not move while it sleeps, anything that touches or changes it wakes it, and
//! sleeping is deterministic and does not change a world until the first island sleeps.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SLEEP_ANGULAR_SPEED,
    DEFAULT_SLEEP_LINEAR_SPEED, DEFAULT_SPECULATIVE_MARGIN, DEFAULT_TIME_TO_SLEEP, PhysicsError,
    STANDARD_GRAVITY, Shape2, SleepConfig, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
const HALF: Vec2 = Vec2::new(0.5, 0.5);
/// Steps in the time to sleep.
const SLEEP_STEPS: u32 = 30;
/// More steps than any scene here needs to settle and sleep.
const PLENTY: u32 = 600;

fn config(max_bodies: u32, gravity: Vec2, sleep: SleepConfig) -> WorldConfig {
    WorldConfig {
        max_bodies,
        gravity,
        max_pairs: 2048,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 2048,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    }
}

fn world(sleep: SleepConfig) -> Result<World, PhysicsError> {
    World::new(config(128, STANDARD_GRAVITY, sleep))
}

fn zero_gravity() -> Result<World, PhysicsError> {
    World::new(config(16, Vec2::ZERO, SleepConfig::DEFAULT))
}

fn ground(world: &mut World, x: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(x, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(8.0, 1.0),
        },
    ))
}

fn crate_at(world: &mut World, x: f32, y: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Box { half_extents: HALF },
    ))
}

fn ball_at(world: &mut World, x: f32, y: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(x, y),
        Shape2::Circle { radius: 0.25 },
    ))
}

/// A pyramid of `rows` rows centred on `x`, bottom row first, boxes touching; returns the boxes in
/// creation order.
fn pyramid(world: &mut World, rows: u8, x: f32) -> Result<Vec<BodyHandle>, PhysicsError> {
    stacked(world, rows, x, 0.0)
}

/// As [`pyramid`], with `gap` metres of air under every box, so the pyramid has to land and settle.
fn dropped_pyramid(world: &mut World, rows: u8, x: f32) -> Result<Vec<BodyHandle>, PhysicsError> {
    stacked(world, rows, x, 0.05)
}

fn stacked(world: &mut World, rows: u8, x: f32, gap: f32) -> Result<Vec<BodyHandle>, PhysicsError> {
    let mut boxes = Vec::new();
    for row in 0..rows {
        let count = rows - row;
        for i in 0..count {
            let offset = f32::from(i) - f32::from(count - 1) / 2.0;
            let y = 0.5 + (1.0 + gap) * f32::from(row) + gap;
            boxes.push(crate_at(world, x + offset, y)?);
        }
    }
    Ok(boxes)
}

fn step_n(world: &mut World, steps: u32) -> Result<(), PhysicsError> {
    for _ in 0..steps {
        world.step(DT)?;
    }
    Ok(())
}

/// Steps until `done` holds; returns the number of steps taken, or `PLENTY` when it never held.
fn step_until(world: &mut World, done: impl Fn(&World) -> bool) -> Result<u32, PhysicsError> {
    for taken in 1..=PLENTY {
        world.step(DT)?;
        if done(world) {
            return Ok(taken);
        }
    }
    Ok(PLENTY)
}

fn all_asleep(world: &World, bodies: &[BodyHandle]) -> bool {
    bodies.iter().all(|&b| world.is_asleep(b).unwrap_or(false))
}

fn none_asleep(world: &World, bodies: &[BodyHandle]) -> bool {
    bodies.iter().all(|&b| !world.is_asleep(b).unwrap_or(true))
}

/// The bits of everything a body keeps.
fn bits_of(world: &World, body: BodyHandle) -> Result<[u32; 7], PhysicsError> {
    let state = world.body(body)?;
    let (position, velocity) = (state.position(), state.linear_velocity());
    let (cosine, sine) = state.rotation().direction();
    Ok([
        position.x.to_bits(),
        position.y.to_bits(),
        velocity.x.to_bits(),
        velocity.y.to_bits(),
        cosine.to_bits(),
        sine.to_bits(),
        state.angular_velocity().to_bits(),
    ])
}

fn snapshot(world: &World, bodies: &[BodyHandle]) -> Result<Vec<[u32; 7]>, PhysicsError> {
    bodies.iter().map(|&b| bits_of(world, b)).collect()
}

// ---------------------------------------------------------------------------------------------
// Islands
// ---------------------------------------------------------------------------------------------

#[test]
fn piles_on_the_same_ground_are_separate_islands_and_the_ground_has_none()
-> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    let floor = ground(&mut world, 0.0)?;
    let left = pyramid(&mut world, 3, -5.0)?;
    let right = pyramid(&mut world, 3, 5.0)?;
    step_n(&mut world, 2)?;
    assert_eq!(world.island_count(), 2);
    assert_eq!(world.island_of(floor)?, None);
    let island_left = world.island_of(left[0])?;
    let island_right = world.island_of(right[0])?;
    assert_ne!(island_left, island_right);
    for &b in &left {
        assert_eq!(world.island_of(b)?, island_left);
    }
    for &b in &right {
        assert_eq!(world.island_of(b)?, island_right);
    }
    Ok(())
}

#[test]
fn the_island_id_is_the_lowest_body_whatever_the_creation_order() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    // Two stacks created interleaved: the slots of one stack are not consecutive.
    let a0 = crate_at(&mut world, -5.0, 0.5)?;
    let b0 = crate_at(&mut world, 5.0, 0.5)?;
    let a1 = crate_at(&mut world, -5.0, 1.5)?;
    let b1 = crate_at(&mut world, 5.0, 1.5)?;
    let a2 = crate_at(&mut world, -5.0, 2.5)?;
    step_n(&mut world, 2)?;
    for b in [a0, a1, a2] {
        assert_eq!(world.island_of(b)?, Some(a0));
    }
    for b in [b0, b1] {
        assert_eq!(world.island_of(b)?, Some(b0));
    }
    Ok(())
}

#[test]
fn a_lone_body_is_its_own_island() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let a = ball_at(&mut world, 0.0, 0.0)?;
    let b = ball_at(&mut world, 10.0, 0.0)?;
    step_n(&mut world, 1)?;
    assert_eq!(world.island_count(), 2);
    assert_eq!(world.island_of(a)?, Some(a));
    assert_eq!(world.island_of(b)?, Some(b));
    Ok(())
}

#[test]
fn touching_bodies_merge_into_one_island() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let a = ball_at(&mut world, 0.0, 0.0)?;
    let b = ball_at(&mut world, 3.0, 0.0)?;
    world.set_linear_velocity(b, Vec2::new(-1.0, 0.0))?;
    step_n(&mut world, 1)?;
    assert_eq!(world.island_count(), 2);
    assert_ne!(world.island_of(a)?, world.island_of(b)?);
    // They meet after about 2.5 m at 1 m/s.
    let meet = step_until(&mut world, |w| w.island_count() == 1)?;
    assert!(meet > 100 && meet < 160, "merged after {meet} steps");
    assert_eq!(world.island_of(b)?, Some(a));
    assert_eq!(world.island_of(a)?, Some(a));
    Ok(())
}

#[test]
fn a_destroyed_body_leaves_the_island_it_joined() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let a = ball_at(&mut world, 0.0, 0.0)?;
    let b = ball_at(&mut world, 0.5, 0.0)?;
    let c = ball_at(&mut world, 1.0, 0.0)?;
    step_n(&mut world, 1)?;
    assert_eq!(world.island_count(), 1);
    world.destroy_body(b)?;
    step_n(&mut world, 1)?;
    assert_eq!(world.island_count(), 2);
    assert_eq!(world.island_of(a)?, Some(a));
    assert_eq!(world.island_of(c)?, Some(c));
    assert_eq!(world.island_of(b), Err(PhysicsError::StaleHandle));
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Sleeping
// ---------------------------------------------------------------------------------------------

#[test]
fn the_defaults_are_the_documented_values() {
    let config = SleepConfig::default();
    assert!(config.enabled);
    assert_eq!(
        config.time_to_sleep.to_bits(),
        DEFAULT_TIME_TO_SLEEP.to_bits()
    );
    assert_eq!(
        config.linear_speed.to_bits(),
        DEFAULT_SLEEP_LINEAR_SPEED.to_bits()
    );
    assert_eq!(
        config.angular_speed.to_bits(),
        DEFAULT_SLEEP_ANGULAR_SPEED.to_bits()
    );
}

#[test]
fn an_invalid_sleep_configuration_is_refused() {
    for bad in [
        SleepConfig {
            time_to_sleep: 0.0,
            ..SleepConfig::DEFAULT
        },
        SleepConfig {
            linear_speed: -1.0,
            ..SleepConfig::DEFAULT
        },
        SleepConfig {
            angular_speed: f32::NAN,
            ..SleepConfig::DEFAULT
        },
    ] {
        assert_eq!(
            World::new(config(4, STANDARD_GRAVITY, bad)).err(),
            Some(PhysicsError::InvalidSleepConfig)
        );
    }
}

#[test]
fn a_settled_pyramid_sleeps_as_one_island_after_the_time_to_sleep() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = dropped_pyramid(&mut world, 5, 0.0)?;
    let taken = step_until(&mut world, |w| w.sleeping_count() > 0)?;
    println!("pyramid of 15 boxes: first sleeper after {taken} steps");
    assert!(taken < PLENTY, "never slept");
    assert!(
        taken > SLEEP_STEPS,
        "slept before the time to sleep: {taken}"
    );
    // One island: everybody sleeps in that very step, and the sleepers have zero velocity.
    assert!(all_asleep(&world, &boxes));
    assert_eq!(world.sleeping_count(), boxes.len());
    for &b in &boxes {
        let body = world.body(b)?;
        assert_eq!(body.linear_velocity(), Vec2::ZERO);
        assert_eq!(body.angular_velocity().to_bits(), 0.0_f32.to_bits());
        assert_eq!(world.rest_time(b)?.to_bits(), 0.0_f32.to_bits());
    }
    Ok(())
}

#[test]
fn nothing_sleeps_before_it_rests_for_the_time_to_sleep() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let rest = crate_at(&mut world, 0.0, 0.5)?;
    // At rest from the first step on: the rest timer is dt after one step and sleeps at 30 steps.
    step_n(&mut world, SLEEP_STEPS - 1)?;
    assert!(!world.is_asleep(rest)?);
    assert!(world.rest_time(rest)? > 0.45);
    world.step(DT)?;
    assert!(world.is_asleep(rest)?);
    Ok(())
}

#[test]
fn a_sleeping_island_does_not_drift_and_costs_no_solver_work() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = pyramid(&mut world, 5, 0.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 15)?;
    let before = snapshot(&world, &boxes)?;
    let hash = world.state_hash();
    step_n(&mut world, 600)?;
    assert_eq!(
        snapshot(&world, &boxes)?,
        before,
        "bits changed while asleep"
    );
    assert_eq!(world.state_hash(), hash);
    let stats = world.solver_stats();
    assert_eq!(stats.solved, 0);
    assert_eq!(stats.frozen, world.manifolds().len());
    assert!(stats.frozen > 0);
    assert!(all_asleep(&world, &boxes));
    Ok(())
}

#[test]
fn a_body_that_falls_or_moves_does_not_sleep() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    let ball = ball_at(&mut world, 0.0, 1000.0)?;
    step_n(&mut world, 120)?;
    assert!(!world.is_asleep(ball)?);
    assert_eq!(world.rest_time(ball)?.to_bits(), 0.0_f32.to_bits());
    Ok(())
}

#[test]
fn the_linear_speed_limit_decides_in_free_space() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let slow = ball_at(&mut world, 0.0, 0.0)?;
    let fast = ball_at(&mut world, 0.0, 10.0)?;
    world.set_linear_velocity(slow, Vec2::new(0.04, 0.0))?;
    world.set_linear_velocity(fast, Vec2::new(0.06, 0.0))?;
    step_n(&mut world, 45)?;
    assert!(world.is_asleep(slow)?);
    assert!(!world.is_asleep(fast)?);
    // Falling asleep stops the body: its velocity is zero now and it stays where it stopped.
    assert_eq!(world.body(slow)?.linear_velocity(), Vec2::ZERO);
    Ok(())
}

#[test]
fn the_angular_speed_limit_decides_in_free_space() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let slow = crate_at(&mut world, 0.0, 0.0)?;
    let fast = crate_at(&mut world, 0.0, 10.0)?;
    world.set_angular_velocity(slow, 0.03)?;
    world.set_angular_velocity(fast, 0.04)?;
    step_n(&mut world, 45)?;
    assert!(world.is_asleep(slow)?);
    assert!(!world.is_asleep(fast)?);
    assert_eq!(
        world.body(slow)?.angular_velocity().to_bits(),
        0.0_f32.to_bits()
    );
    Ok(())
}

#[test]
fn an_island_sleeps_only_when_every_body_in_it_rests() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    // Two frictionless boxes, one on the other; the top one slides along the lower one.
    let mut low = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, 0.0),
        Shape2::Box { half_extents: HALF },
    );
    low.friction = 0.0;
    let mut high = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, 1.0),
        Shape2::Box { half_extents: HALF },
    );
    high.friction = 0.0;
    high.linear_velocity = Vec2::new(0.4, 0.0);
    let (a, b) = (world.create_body(&low)?, world.create_body(&high)?);
    step_n(&mut world, 40)?;
    assert_eq!(world.island_of(a)?, world.island_of(b)?, "they touch");
    assert!(
        !world.is_asleep(a)?,
        "the lower box must wait for the upper one"
    );
    world.set_linear_velocity(b, Vec2::ZERO)?;
    step_until(&mut world, |w| w.sleeping_count() == 2)?;
    assert!(world.is_asleep(a)? && world.is_asleep(b)?);
    Ok(())
}

#[test]
fn a_body_dropped_on_a_sleeping_pyramid_wakes_all_of_it() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = pyramid(&mut world, 5, 0.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 15)?;
    let ball = ball_at(&mut world, 0.0, 9.0)?;
    let mut woke_at = None;
    for taken in 1..=PLENTY {
        world.step(DT)?;
        if woke_at.is_none() && !world.is_asleep(boxes[0])? {
            woke_at = Some(taken);
            // The whole island wakes, not only the box that was hit.
            assert!(none_asleep(&world, &boxes), "partly awake");
        }
        if woke_at.is_none() {
            assert!(all_asleep(&world, &boxes));
        }
    }
    println!("ball woke the pyramid after {woke_at:?} steps");
    let woke = woke_at.ok_or(PhysicsError::NotFinite)?;
    // It falls about 3.75 m before it touches the top: roughly 1 s.
    assert!(woke > 40 && woke < 100, "woke at {woke}");
    // Afterwards everything settles and sleeps again, ball included.
    assert!(world.is_asleep(ball)? && all_asleep(&world, &boxes));
    Ok(())
}

#[test]
fn a_wake_by_contact_leaves_other_piles_asleep_and_untouched() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let near = pyramid(&mut world, 3, -5.0)?;
    let far = pyramid(&mut world, 3, 5.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 12)?;
    let far_before = snapshot(&world, &far)?;
    ball_at(&mut world, -5.0, 6.0)?;
    step_until(&mut world, |w| !w.is_asleep(near[0]).unwrap_or(true))?;
    assert!(none_asleep(&world, &near));
    assert!(all_asleep(&world, &far));
    step_n(&mut world, 30)?;
    assert_eq!(snapshot(&world, &far)?, far_before);
    Ok(())
}

#[test]
fn the_explicit_wake_wakes_the_island_and_restarts_the_timer() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = pyramid(&mut world, 3, 0.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 6)?;
    world.wake(boxes[4])?;
    assert!(none_asleep(&world, &boxes));
    assert_eq!(world.rest_time(boxes[4])?.to_bits(), 0.0_f32.to_bits());
    // Awake and at rest: it needs the whole time to sleep again.
    step_n(&mut world, SLEEP_STEPS - 1)?;
    assert!(none_asleep(&world, &boxes));
    step_n(&mut world, 1)?;
    assert!(all_asleep(&world, &boxes));
    Ok(())
}

#[test]
fn waking_an_awake_body_restarts_its_rest_timer() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let ball = ball_at(&mut world, 0.0, 0.0)?;
    step_n(&mut world, 20)?;
    assert!(world.rest_time(ball)? > 0.3);
    world.wake(ball)?;
    assert_eq!(world.rest_time(ball)?.to_bits(), 0.0_f32.to_bits());
    Ok(())
}

#[test]
fn waking_checks_the_handle_and_the_kind() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    let floor = ground(&mut world, 0.0)?;
    let rest = crate_at(&mut world, 0.0, 0.5)?;
    assert_eq!(world.wake(floor), Err(PhysicsError::StaticBody));
    world.destroy_body(rest)?;
    assert_eq!(world.wake(rest), Err(PhysicsError::StaleHandle));
    assert_eq!(world.is_asleep(rest), Err(PhysicsError::StaleHandle));
    assert_eq!(world.rest_time(rest), Err(PhysicsError::StaleHandle));
    assert_eq!(world.is_asleep(floor), Ok(false));
    Ok(())
}

#[test]
fn every_setter_wakes_the_island_of_a_sleeping_body() -> Result<(), PhysicsError> {
    type Change = fn(&mut World, BodyHandle) -> Result<(), PhysicsError>;
    let changes: [(&str, Change); 4] = [
        ("linear velocity", |w, b| {
            w.set_linear_velocity(b, Vec2::ZERO)
        }),
        ("angular velocity", |w, b| w.set_angular_velocity(b, 0.0)),
        ("force", |w, b| w.apply_force(b, Vec2::ZERO)),
        ("torque", |w, b| w.apply_torque(b, 0.0)),
    ];
    for (name, change) in changes {
        let mut world = world(SleepConfig::DEFAULT)?;
        ground(&mut world, 0.0)?;
        let boxes = pyramid(&mut world, 3, 0.0)?;
        step_until(&mut world, |w| w.sleeping_count() == 6)?;
        change(&mut world, boxes[5])?;
        assert!(
            none_asleep(&world, &boxes),
            "{name} did not wake the island"
        );
        assert_eq!(
            world.rest_time(boxes[5])?.to_bits(),
            0.0_f32.to_bits(),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn a_force_on_a_sleeping_body_moves_it_in_the_next_step() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let ball = ball_at(&mut world, 0.0, 0.0)?;
    step_until(&mut world, |w| w.is_asleep(ball).unwrap_or(false))?;
    world.apply_force(ball, Vec2::new(10.0, 0.0))?;
    world.step(DT)?;
    assert!(world.body(ball)?.position().x > 0.0);
    assert!(!world.is_asleep(ball)?);
    Ok(())
}

#[test]
fn a_refused_setter_does_not_wake() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let ball = ball_at(&mut world, 0.0, 0.0)?;
    step_until(&mut world, |w| w.is_asleep(ball).unwrap_or(false))?;
    assert_eq!(
        world.set_linear_velocity(ball, Vec2::new(f32::NAN, 0.0)),
        Err(PhysicsError::NotFinite)
    );
    assert!(world.is_asleep(ball)?);
    Ok(())
}

/// Between destroying the lowest body of an island and the next step the others name an empty slot:
/// they answer with themselves.
#[test]
fn an_island_whose_lowest_body_was_destroyed_answers_with_its_own_handle()
-> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let first = ball_at(&mut world, 0.0, 0.0)?;
    let second = ball_at(&mut world, 0.4, 0.0)?;
    step_n(&mut world, 1)?;
    assert_eq!(world.island_of(second)?, Some(first));
    world.destroy_body(first)?;
    assert_eq!(world.island_of(second)?, Some(second));
    Ok(())
}

/// A slot that is reused starts clean: alone in its island, with a rest timer of zero.
#[test]
fn a_reused_slot_starts_alone_and_with_no_rest() -> Result<(), PhysicsError> {
    let mut world = zero_gravity()?;
    let first = ball_at(&mut world, 0.0, 0.0)?;
    let second = ball_at(&mut world, 0.4, 0.0)?;
    step_n(&mut world, 20)?;
    assert_eq!(world.island_of(second)?, Some(first), "they touch");
    assert!(world.rest_time(second)? > 0.2, "it has rested");
    world.destroy_body(second)?;
    let third = ball_at(&mut world, 50.0, 0.0)?;
    assert_eq!(third.index(), second.index(), "the slot is reused");
    assert_eq!(world.island_of(third)?, Some(third));
    assert_eq!(world.rest_time(third)?.to_bits(), 0.0_f32.to_bits());
    Ok(())
}

#[test]
fn destroying_the_body_a_sleeper_rests_on_wakes_it() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let low = crate_at(&mut world, 0.0, 0.5)?;
    let high = crate_at(&mut world, 0.0, 1.5)?;
    step_until(&mut world, |w| w.sleeping_count() == 2)?;
    world.destroy_body(low)?;
    assert!(!world.is_asleep(high)?);
    step_n(&mut world, 30)?;
    assert!(world.body(high)?.position().y < 1.4, "it did not fall");
    Ok(())
}

#[test]
fn destroying_the_static_body_a_sleeper_rests_on_wakes_it() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    let floor = ground(&mut world, 0.0)?;
    let rest = crate_at(&mut world, 0.0, 0.5)?;
    let bystander = crate_at(&mut world, 100.0, 50.0)?;
    step_until(&mut world, |w| w.is_asleep(rest).unwrap_or(false))?;
    world.destroy_body(floor)?;
    assert!(!world.is_asleep(rest)?);
    assert!(!world.is_asleep(bystander)?);
    step_n(&mut world, 30)?;
    assert!(world.body(rest)?.position().y < 0.4, "it did not fall");
    Ok(())
}

/// The static body may come second in the pair as well as first (it was created after the box).
#[test]
fn destroying_a_later_static_body_a_sleeper_rests_on_wakes_it() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    let rest = crate_at(&mut world, 0.0, 0.5)?;
    let floor = ground(&mut world, 0.0)?;
    step_until(&mut world, |w| w.is_asleep(rest).unwrap_or(false))?;
    world.destroy_body(floor)?;
    assert!(!world.is_asleep(rest)?);
    Ok(())
}

#[test]
fn creating_a_static_body_wakes_every_sleeper() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = pyramid(&mut world, 3, 0.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 6)?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(50.0, 50.0),
        Shape2::Circle { radius: 1.0 },
    ))?;
    assert!(none_asleep(&world, &boxes));
    Ok(())
}

#[test]
fn a_body_created_on_a_sleeping_pyramid_wakes_it_in_the_next_step() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = pyramid(&mut world, 3, 0.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 6)?;
    // Created overlapping the top box.
    ball_at(&mut world, 0.0, 2.8)?;
    world.step(DT)?;
    assert!(none_asleep(&world, &boxes));
    Ok(())
}

#[test]
fn a_refused_step_wakes_nothing_and_changes_nothing() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let sleeper = crate_at(&mut world, 0.0, 0.5)?;
    step_until(&mut world, |w| w.is_asleep(sleeper).unwrap_or(false))?;
    let attacker = crate_at(&mut world, 0.0, 1.5)?;
    world.set_linear_velocity(attacker, Vec2::new(0.0, -3.0e38))?;
    let before = world.state_hash();
    let steps = world.step_count();
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert!(world.is_asleep(sleeper)?);
    assert_eq!(world.state_hash(), before);
    assert_eq!(world.step_count(), steps);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Determinism and the hash
// ---------------------------------------------------------------------------------------------

/// The state hash after every step of a pyramid that settles, sleeps, is hit and settles again.
fn history(sleep: SleepConfig) -> Result<(Vec<u64>, Vec<usize>), PhysicsError> {
    let mut world = world(sleep)?;
    ground(&mut world, 0.0)?;
    dropped_pyramid(&mut world, 4, 0.0)?;
    let mut hashes = Vec::new();
    let mut sleepers = Vec::new();
    for step in 0..PLENTY {
        if step == 200 {
            ball_at(&mut world, 0.2, 7.0)?;
        }
        world.step(DT)?;
        hashes.push(world.state_hash());
        sleepers.push(world.sleeping_count());
    }
    Ok((hashes, sleepers))
}

#[test]
fn sleeping_and_waking_are_the_same_on_every_run() -> Result<(), PhysicsError> {
    let first = history(SleepConfig::DEFAULT)?;
    let second = history(SleepConfig::DEFAULT)?;
    assert_eq!(first, second);
    // The scene does sleep, wake and sleep again.
    let sleepers = &first.1;
    assert!(sleepers.iter().any(|&n| n > 0));
    let woken = sleepers.windows(2).any(|w| w[0] > 0 && w[1] == 0);
    assert!(woken, "the ball must wake the pyramid");
    assert!(sleepers.last().is_some_and(|&n| n > 0));
    Ok(())
}

#[test]
fn a_world_with_sleeping_equals_one_without_until_the_first_island_sleeps()
-> Result<(), PhysicsError> {
    let (with, sleepers) = history(SleepConfig::DEFAULT)?;
    let (without, none) = history(SleepConfig::DISABLED)?;
    assert!(none.iter().all(|&n| n == 0));
    let first_sleep = sleepers
        .iter()
        .position(|&n| n > 0)
        .ok_or(PhysicsError::NotFinite)?;
    assert!(first_sleep > 30);
    assert_eq!(with.get(..first_sleep), without.get(..first_sleep));
    assert_ne!(with.get(first_sleep), without.get(first_sleep));
    Ok(())
}

#[test]
fn with_sleeping_off_nothing_ever_sleeps() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DISABLED)?;
    ground(&mut world, 0.0)?;
    let boxes = pyramid(&mut world, 4, 0.0)?;
    step_n(&mut world, PLENTY)?;
    assert_eq!(world.sleeping_count(), 0);
    assert!(none_asleep(&world, &boxes));
    assert!(world.solver_stats().solved > 0);
    Ok(())
}

#[test]
fn a_sleeping_body_is_part_of_the_state_hash() -> Result<(), PhysicsError> {
    // Two worlds in the same state of motion (nothing moves) differ only in that one sleeps.
    let mut asleep = zero_gravity()?;
    let mut awake = World::new(config(16, Vec2::ZERO, SleepConfig::DISABLED))?;
    ball_at(&mut asleep, 0.0, 0.0)?;
    ball_at(&mut awake, 0.0, 0.0)?;
    step_n(&mut asleep, 29)?;
    step_n(&mut awake, 29)?;
    assert_eq!(asleep.state_hash(), awake.state_hash());
    step_n(&mut asleep, 1)?;
    step_n(&mut awake, 1)?;
    assert_eq!(asleep.sleeping_count(), 1);
    assert_ne!(asleep.state_hash(), awake.state_hash());
    Ok(())
}

#[test]
fn a_woken_pyramid_resumes_from_its_warm_start_without_a_jolt() -> Result<(), PhysicsError> {
    let mut world = world(SleepConfig::DEFAULT)?;
    ground(&mut world, 0.0)?;
    let boxes = dropped_pyramid(&mut world, 6, 0.0)?;
    step_until(&mut world, |w| w.sleeping_count() == 21)?;
    let heights = |world: &World| -> Result<Vec<f32>, PhysicsError> {
        boxes
            .iter()
            .map(|&b| Ok(world.body(b)?.position().y))
            .collect()
    };
    let asleep = heights(&world)?;
    world.wake(boxes[0])?;
    // The impulses that held the pyramid up were kept while it slept, so the first step after the
    // wake starts from them. Without them every box would start the step with no support.
    let mut fastest = 0.0_f32;
    for _ in 0..10 {
        world.step(DT)?;
        for &b in &boxes {
            fastest = fastest.max(world.body(b)?.linear_velocity().length());
        }
    }
    println!("after the wake: fastest {fastest:e} m/s");
    assert!(fastest < 0.01, "the pyramid jolted: {fastest} m/s");
    let now = heights(&world)?;
    let drift = now
        .iter()
        .zip(&asleep)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(drift < 0.002, "moved {drift} m in 10 steps");
    Ok(())
}
