//! Property tests over seeded random scenes (`generator`): 8 properties, each run over the same 25
//! scenes (25 seeds from a fixed list; 8 x 25 = 200 checks, but 25 different scenes, not 200). A
//! property that fails names its seed, which is all that is needed to reproduce the scene.

use super::execution::{executions, trace};
use super::generator::{generate, generate_with_bin};
use super::limits::{DT, ENERGY_SLACK_PER_BODY, ENERGY_SLACK_RATIO, PENETRATION};
use super::measure::{all_finite, deepest, dynamic_count, energy};
use super::scene::{Scene, config, hinge_at};
use base::math::Vec2;
use base::rng::Pcg32;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, Joint2, JointDesc, PhysicsError, RevoluteJoint2, Shape2,
    SleepConfig, SolverExecution, World,
};

/// Seeds per property.
const SEED_COUNT: u64 = 25;

/// The first seed and the distance between seeds of the fixed list.
const FIRST_SEED: u64 = 1_000;
const SEED_STRIDE: u64 = 7;

/// The seeds every property runs over.
fn seeds() -> impl Iterator<Item = u64> {
    (0..SEED_COUNT).map(|i| FIRST_SEED + i * SEED_STRIDE)
}

/// No body may leave this box, in metres on either axis, nor go below the floor by more than 1 m:
/// a bound far outside any scene (the bin is 8 m wide and 8 m high).
const WORLD_BOUND: f32 = 100.0;

/// Steps of the quick properties (2.5 s) and of the ones that need the scene to settle (4 s).
const SHORT_STEPS: u32 = 150;
const SETTLE_STEPS: u32 = 240;

/// The bin is calm when its fastest body moves below this speed in metres per second, and the
/// penetration bound is checked once it has been calm for `CALM_STEPS` steps (0.5 s) in a row. A
/// body that lands at several metres per second goes deeper than the bound for a few steps, also
/// after its speed has dropped (seed 1000: 0.027 m at step 21; seed 1014: 0.0455 m at 4.6 m/s);
/// the bound is for bodies at rest. Defined after the first runs, with the bound unchanged.
const CALM_SPEED: f32 = 1.0;
const CALM_STEPS: u32 = 30;

/// Steps of the penetration run (8 s): some piles take several seconds to become calm.
const CALM_RUN_STEPS: u32 = 480;

/// The number of calm steps the property must have checked in every scene, so that it cannot pass
/// because a scene was never calm.
const MIN_CALM_CHECKS: u32 = 150;

/// The least number of scenes that must fall completely asleep within `SETTLE_STEPS`, so that the
/// check "a bit changed while everything sleeps" runs.
const MIN_ALL_ASLEEP: u64 = 8;

/// Steps of the execution comparison: the scenes are small, the pools are many.
const COMPARE_STEPS: u32 = 60;

/// The least number of seeds whose huge-velocity step must be refused (the others must agree with
/// their twin), so that the refusal branch cannot silently never run.
const MIN_REFUSALS: u64 = 20;

/// A scene changes its hash more often than this in `SHORT_STEPS` steps (it moves).
const MIN_CHANGES: usize = 30;

/// The stream of the generator of refused inputs.
const REFUSAL_STREAM: u64 = 31;

/// A value that is not a finite number, chosen by `pick`.
fn not_a_number(pick: u32) -> f32 {
    let kinds = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
    kinds[usize::try_from(pick % 3).unwrap_or(0)]
}

/// Asserts that every tracked body of `scene` is inside the generous world bound.
fn assert_inside(scene: &Scene, seed: u64, step: u32) -> Result<(), PhysicsError> {
    for &handle in &scene.tracked {
        let at = scene.world.body(handle)?.position();
        assert!(
            at.x.abs() < WORLD_BOUND && at.y > -1.0 && at.y < WORLD_BOUND,
            "seed {seed} step {step}: a body is at {at:?}"
        );
    }
    Ok(())
}

#[test]
fn no_state_is_ever_not_a_number_and_no_body_leaves_the_world() -> Result<(), PhysicsError> {
    for seed in seeds() {
        let mut scene = generate(seed, SleepConfig::DEFAULT)?;
        for step in 0..SHORT_STEPS {
            scene.world.step(DT)?;
            assert!(
                all_finite(&scene.world),
                "seed {seed} step {step}: not finite"
            );
            assert_inside(&scene, seed, step)?;
        }
    }
    Ok(())
}

#[test]
fn energy_never_rises_above_its_start_by_more_than_the_slack_when_nothing_bounces()
-> Result<(), PhysicsError> {
    for seed in seeds() {
        let mut scene = generate(seed, SleepConfig::DEFAULT)?;
        let first = energy(&scene.world);
        let slack = ENERGY_SLACK_RATIO * first.abs()
            + ENERGY_SLACK_PER_BODY
                * f64::from(u32::try_from(dynamic_count(&scene.world)).unwrap_or(0));
        for step in 0..SETTLE_STEPS {
            scene.world.step(DT)?;
            let now = energy(&scene.world);
            assert!(
                now <= first + slack,
                "seed {seed} step {step}: energy {now} J above start {first} J (slack {slack})"
            );
        }
    }
    Ok(())
}

/// The deepest contact of the scene of `seed` over the steps at which its bin has been calm, and
/// the number of such steps.
fn calm_depth(seed: u64) -> Result<(f32, u32), PhysicsError> {
    let (mut scene, in_bin) = generate_with_bin(seed, SleepConfig::DISABLED)?;
    let (mut calm, mut checked, mut deepest_calm) = (0_u32, 0_u32, 0.0_f32);
    for _ in 0..CALM_RUN_STEPS {
        scene.world.step(DT)?;
        let mut fastest = 0.0_f32;
        for &handle in scene.tracked.iter().take(in_bin) {
            fastest = fastest.max(scene.world.body(handle)?.linear_velocity().length());
        }
        calm = if fastest < CALM_SPEED { calm + 1 } else { 0 };
        if calm >= CALM_STEPS {
            checked += 1;
            deepest_calm = deepest_calm.max(deepest(&scene.world));
        }
    }
    Ok((deepest_calm, checked))
}

#[test]
fn no_contact_is_deep_once_the_bin_has_been_calm_for_half_a_second() -> Result<(), PhysicsError> {
    let mut too_short = Vec::new();
    for seed in seeds() {
        let (depth, steps) = calm_depth(seed)?;
        assert!(
            depth < PENETRATION,
            "seed {seed}: a contact is {depth} m deep"
        );
        if steps < MIN_CALM_CHECKS {
            too_short.push((seed, steps));
        }
    }
    assert!(
        too_short.is_empty(),
        "seeds with fewer than {MIN_CALM_CHECKS} calm steps checked: {too_short:?}"
    );
    Ok(())
}

#[test]
fn the_same_seed_gives_the_same_hash_after_every_step() -> Result<(), PhysicsError> {
    for seed in seeds() {
        let build = || generate(seed, SleepConfig::DEFAULT);
        let first = trace(&build, SolverExecution::SERIAL, SHORT_STEPS)?;
        let second = trace(&build, SolverExecution::SERIAL, SHORT_STEPS)?;
        assert_eq!(first, second, "seed {seed}");
        let changes = first.windows(2).filter(|pair| pair[0] != pair[1]).count();
        assert!(changes > MIN_CHANGES, "seed {seed}: the scene moves");
    }
    Ok(())
}

#[test]
fn serial_parallel_and_lane_execution_give_the_same_hash_for_every_seed() -> Result<(), PhysicsError>
{
    let configurations = executions()?;
    for seed in seeds() {
        let build = || generate(seed, SleepConfig::DEFAULT);
        let reference = trace(&build, SolverExecution::SERIAL, COMPARE_STEPS)?;
        for (label, execution) in &configurations {
            let got = trace(&build, execution.clone(), COMPARE_STEPS)?;
            assert_eq!(got, reference, "seed {seed}: {label}");
        }
    }
    Ok(())
}

/// Steps the scene of `seed` with sleeping off and on: the hashes must agree until the first body
/// sleeps, and be constant from the step everything sleeps. Whether something fell asleep, and
/// whether everything did at some step.
fn sleeping_agrees(seed: u64) -> Result<(bool, bool), PhysicsError> {
    let mut awake = generate(seed, SleepConfig::DISABLED)?;
    let mut sleepy = generate(seed, SleepConfig::DEFAULT)?;
    let mut frozen: Option<u64> = None;
    let (mut ever_slept, mut all_slept) = (false, false);
    for step in 0..SETTLE_STEPS {
        awake.world.step(DT)?;
        sleepy.world.step(DT)?;
        ever_slept |= sleepy.world.sleeping_count() > 0;
        let hash = sleepy.world.state_hash();
        assert!(
            ever_slept || awake.world.state_hash() == hash,
            "seed {seed} step {step}: sleeping on differs before anything slept"
        );
        if sleepy.world.sleeping_count() == dynamic_count(&sleepy.world) {
            assert!(
                frozen.is_none_or(|before| before == hash),
                "seed {seed} step {step}: a bit changed while everything sleeps"
            );
            frozen = Some(hash);
            all_slept = true;
        }
    }
    Ok((ever_slept, all_slept))
}

#[test]
fn sleeping_changes_nothing_until_the_first_body_sleeps_and_an_asleep_world_is_frozen()
-> Result<(), PhysicsError> {
    let (mut slept, mut all_slept) = (0_u64, 0_u64);
    for seed in seeds() {
        let (some, all) = sleeping_agrees(seed)?;
        slept += u64::from(some);
        all_slept += u64::from(all);
    }
    assert!(
        slept >= SEED_COUNT / 2,
        "only {slept} of {SEED_COUNT} scenes slept"
    );
    assert!(
        all_slept >= MIN_ALL_ASLEEP,
        "only {all_slept} of {SEED_COUNT} scenes fell completely asleep, so the frozen check ran too rarely"
    );
    Ok(())
}

/// The least number of scenes in which a body of the bin and a link of the chain are in one island
/// at some step, so that the properties cover joints and contacts together and not side by side.
const MIN_COUPLED: u64 = 2;

/// Steps over which a scene is watched for a chain and the bin sharing an island.
const COUPLED_STEPS: u32 = 120;

/// True when a link of the chain is in the same island as a body of the bin.
fn chain_meets_bin(scene: &Scene, in_bin: usize) -> Result<bool, PhysicsError> {
    let Some(&link) = scene.tracked.get(in_bin) else {
        return Ok(false);
    };
    let island = scene.world.island_of(link)?;
    for &body in scene.tracked.iter().take(in_bin) {
        if scene.world.island_of(body)? == island {
            return Ok(true);
        }
    }
    Ok(false)
}

#[test]
fn some_random_scenes_put_the_chain_and_the_bin_in_one_island() -> Result<(), PhysicsError> {
    let mut coupled = 0_u64;
    for seed in seeds() {
        let (mut scene, in_bin) = generate_with_bin(seed, SleepConfig::DISABLED)?;
        let mut together = false;
        for _ in 0..COUPLED_STEPS {
            scene.world.step(DT)?;
            together |= chain_meets_bin(&scene, in_bin)?;
        }
        coupled += u64::from(together);
    }
    assert!(
        coupled >= MIN_COUPLED,
        "only {coupled} of {SEED_COUNT} scenes put the chain and the bin in one island"
    );
    Ok(())
}

/// Calls that must be refused, each applied to the world of `scene`.
fn refuse_bad_calls(scene: &mut Scene, body: BodyHandle, rng: &mut Pcg32) {
    let bad = not_a_number(rng.next_u32());
    for dt in [f32::NAN, f32::INFINITY, 0.0, -DT] {
        assert_eq!(scene.world.step(dt), Err(PhysicsError::InvalidTimeStep));
    }
    let world = &mut scene.world;
    assert_eq!(
        world.set_linear_velocity(body, Vec2::new(bad, 0.0)),
        Err(PhysicsError::NotFinite)
    );
    assert_eq!(
        world.set_angular_velocity(body, bad),
        Err(PhysicsError::NotFinite)
    );
    assert_eq!(
        world.apply_force(body, Vec2::new(0.0, bad)),
        Err(PhysicsError::NotFinite)
    );
    assert_eq!(world.apply_torque(body, bad), Err(PhysicsError::NotFinite));
    let at_nan = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(bad, 1.0),
        Shape2::Circle { radius: 0.5 },
    );
    assert_eq!(
        world.create_body(&at_nan).err(),
        Some(PhysicsError::NotFinite)
    );
    let flat = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, 1.0),
        Shape2::Circle { radius: -1.0 },
    );
    assert_eq!(
        world.create_body(&flat).err(),
        Some(PhysicsError::InvalidShape)
    );
    let twice = JointDesc::new(
        body,
        body,
        Joint2::Revolute(RevoluteJoint2::new(Vec2::ZERO, Vec2::ZERO)),
    );
    assert_eq!(
        world.create_joint(&twice).err(),
        Some(PhysicsError::InvalidJoint)
    );
}

#[test]
fn refused_calls_and_a_refused_step_leave_the_state_unchanged() -> Result<(), PhysicsError> {
    let mut refusals = 0_u64;
    for seed in seeds() {
        let mut rng = Pcg32::new(seed, REFUSAL_STREAM);
        let mut scene = generate(seed, SleepConfig::DEFAULT)?;
        let mut twin = generate(seed, SleepConfig::DEFAULT)?;
        for _ in 0..SETTLE_STEPS {
            scene.world.step(DT)?;
            twin.world.step(DT)?;
        }
        let body = scene
            .tracked
            .first()
            .copied()
            .ok_or(PhysicsError::StaleHandle)?;
        refuse_bad_calls(&mut scene, body, &mut rng);
        assert_eq!(
            scene.world.state_hash(),
            twin.world.state_hash(),
            "seed {seed}"
        );
        // A finite velocity whose step overflows is accepted by the setter and refused by the step,
        // provided the body is awake (a sleeping body is not integrated), so it is woken first.
        scene.world.wake(body)?;
        twin.world.wake(body)?;
        scene
            .world
            .set_linear_velocity(body, Vec2::new(0.0, -f32::MAX))?;
        twin.world
            .set_linear_velocity(body, Vec2::new(0.0, -f32::MAX))?;
        let before = scene.world.state_hash();
        let result = scene.world.step(DT);
        assert_eq!(result, twin.world.step(DT), "seed {seed}: twins disagree");
        if result.is_err() {
            assert_eq!(
                scene.world.state_hash(),
                before,
                "seed {seed}: the refused step changed the state"
            );
            refusals += 1;
        } else {
            assert_eq!(
                scene.world.state_hash(),
                twin.world.state_hash(),
                "seed {seed}: the accepted step differs from its twin"
            );
        }
    }
    assert!(
        refusals >= MIN_REFUSALS,
        "only {refusals} of {SEED_COUNT} steps were refused"
    );
    Ok(())
}

/// A world of `bodies` and `joints` slots and nothing in it.
fn small_world(bodies: u32, joints: u32) -> Result<World, PhysicsError> {
    World::new(config(bodies, joints, SleepConfig::DEFAULT))
}

/// A dynamic circle of radius 0.4 at `(2 i, 5)`: far enough from the others to touch none.
fn spaced_ball(world: &mut World, i: u32) -> Result<BodyHandle, PhysicsError> {
    let at = Vec2::new(2.0 * f32::from(u8::try_from(i).unwrap_or(0)), 5.0);
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        at,
        Shape2::Circle { radius: 0.4 },
    ))
}

/// Makes `bodies` balls and `joints` hinges between neighbours, which fills a world of exactly
/// that capacity.
fn fill(world: &mut World, bodies: u32, joints: u32) -> Result<(), PhysicsError> {
    let mut handles = Vec::new();
    for i in 0..bodies {
        handles.push(spaced_ball(world, i)?);
    }
    for pair in handles
        .windows(2)
        .take(usize::try_from(joints).unwrap_or(0))
    {
        let middle = (world.body(pair[0])?.position() + world.body(pair[1])?.position()) * 0.5;
        hinge_at(world, pair[0], pair[1], middle)?;
    }
    Ok(())
}

#[test]
fn a_full_world_reports_the_overflow_as_a_value_and_is_not_corrupted() -> Result<(), PhysicsError> {
    for seed in seeds() {
        let mut rng = Pcg32::new(seed, REFUSAL_STREAM);
        let bodies = 3 + rng.next_u32() % 6;
        let joints = 1 + rng.next_u32() % (bodies - 1);
        let mut full = small_world(bodies, joints)?;
        let mut twin = small_world(bodies, joints)?;
        fill(&mut full, bodies, joints)?;
        fill(&mut twin, bodies, joints)?;
        let hash = full.state_hash();
        let extra = full.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(-5.0, 5.0),
            Shape2::Circle { radius: 0.4 },
        ));
        assert_eq!(
            extra.err(),
            Some(PhysicsError::CapacityExhausted),
            "seed {seed}"
        );
        let handles: Vec<_> = full.iter().map(|(handle, _)| handle).collect();
        let (first, last) = (handles.first(), handles.last());
        let (Some(&first), Some(&last)) = (first, last) else {
            return Err(PhysicsError::StaleHandle);
        };
        let desc = JointDesc::new(
            first,
            last,
            Joint2::Revolute(RevoluteJoint2::new(Vec2::ZERO, Vec2::ZERO)),
        );
        assert_eq!(
            full.create_joint(&desc).err(),
            Some(PhysicsError::JointCapacityExhausted),
            "seed {seed}"
        );
        assert_eq!(
            full.len(),
            usize::try_from(bodies).unwrap_or(0),
            "seed {seed}"
        );
        assert_eq!(full.joint_count(), usize::try_from(joints).unwrap_or(0));
        assert_eq!(
            full.state_hash(),
            hash,
            "seed {seed}: refusals changed the state"
        );
        for _ in 0..SHORT_STEPS / 2 {
            full.step(DT)?;
            twin.step(DT)?;
        }
        assert_eq!(
            full.state_hash(),
            twin.state_hash(),
            "seed {seed}: continued stepping"
        );
        assert!(all_finite(&full), "seed {seed}");
    }
    Ok(())
}
