//! The 3D stability and determinism scenes (card 8-12): every scene of the catalogue stands, stays
//! in its box or hangs together as it should, never blows up, and gives the same state hash twice
//! and at one and at several threads (D0 and D1).

use physics::d3::{PhysicsError, SolverExecution};
use tools_physics_bench::make_pool;
use tools_physics_bench::scene3d::{
    CHAIN_LINK, Judgement, REST_SPEED, RNG_SEED, Run3, SPEED_LIMIT, STANDING_DRIFT, Scene3,
    Scene3World, run_scene3, run_seeded3,
};

type Failure = Box<dyn std::error::Error>;

const STEPS: u32 = 400;

fn serial() -> SolverExecution {
    SolverExecution::SERIAL
}

fn pooled(workers: u32) -> Result<SolverExecution, Failure> {
    Ok(SolverExecution {
        scheduler: Some(make_pool(workers).map_err(|e| format!("{e:?}"))?),
        ..SolverExecution::SERIAL
    })
}

fn run(scene: Scene3) -> Result<Run3, PhysicsError> {
    run_scene3(scene, STEPS, &serial(), true)
}

#[test]
fn the_catalogue_has_distinct_names_and_bodies() {
    let names: Vec<&str> = Scene3::ALL.iter().map(|s| s.name()).collect();
    for (i, name) in names.iter().enumerate() {
        assert!(!name.is_empty());
        assert!(!names[..i].contains(name), "{name} twice");
    }
    for scene in Scene3::ALL {
        assert!(scene.bodies() >= 5, "{}", scene.name());
    }
}

#[test]
fn a_scene_has_the_bodies_it_says_and_loses_no_contact() -> Result<(), Failure> {
    for scene in Scene3::ALL {
        let world = Scene3World::new(scene, &serial(), true)?;
        assert_eq!(world.positions()?.len(), scene.bodies(), "{}", scene.name());
        let finished = run(scene)?;
        assert!(
            finished.complete,
            "{}: a pair or manifold list overflowed",
            scene.name()
        );
    }
    Ok(())
}

#[test]
fn no_scene_blows_up() -> Result<(), Failure> {
    for scene in Scene3::ALL {
        let found = run(scene)?;
        assert!(found.peak_speed.is_finite(), "{}", scene.name());
        assert!(
            found.peak_speed < SPEED_LIMIT,
            "{}: {}",
            scene.name(),
            found.peak_speed
        );
        assert!(found.final_speed.is_finite(), "{}", scene.name());
    }
    Ok(())
}

/// The fastest spin any scene of the catalogue reaches, in radians per second, is far below the
/// bound proposed in card 8-37 (measured: a little over 40 for the chain, the fastest scene).
const SPIN_LIMIT: f32 = 100.0;

/// The fastest spin of any dynamic body now, in radians per second.
fn fastest_spin(world: &Scene3World) -> f32 {
    world
        .world()
        .iter()
        .map(|(_, body)| body.angular_velocity().length())
        .map(|spin| {
            if spin.is_finite() {
                spin
            } else {
                f32::INFINITY
            }
        })
        .fold(0.0, f32::max)
}

#[test]
fn no_scene_spins_wildly() -> Result<(), Failure> {
    for scene in Scene3::ALL {
        let mut world = Scene3World::new(scene, &serial(), true)?;
        let mut peak = 0.0_f32;
        for _ in 0..STEPS {
            world.step()?;
            peak = peak.max(fastest_spin(&world));
        }
        assert!(peak < SPIN_LIMIT, "{}: {peak} rad/s", scene.name());
    }
    Ok(())
}

#[test]
fn the_standing_scenes_stand_still_awake() -> Result<(), Failure> {
    for scene in Scene3::ALL
        .iter()
        .filter(|s| s.judgement() == Judgement::Standing)
    {
        let awake = run_scene3(*scene, STEPS, &serial(), false)?;
        assert!(
            awake.settled_drift < STANDING_DRIFT,
            "{}: {}",
            scene.name(),
            awake.settled_drift
        );
        assert!(
            awake.final_speed < REST_SPEED,
            "{}: {}",
            scene.name(),
            awake.final_speed
        );
    }
    Ok(())
}

#[test]
fn the_standing_scenes_stand_still() -> Result<(), Failure> {
    for scene in Scene3::ALL
        .iter()
        .filter(|s| s.judgement() == Judgement::Standing)
    {
        let found = run(*scene)?;
        assert!(
            found.settled_drift < STANDING_DRIFT,
            "{}: moved {} m after settling",
            scene.name(),
            found.settled_drift
        );
        assert!(
            found.final_speed < REST_SPEED,
            "{}: {}",
            scene.name(),
            found.final_speed
        );
    }
    Ok(())
}

/// Steps a pile needs to settle, and the speed under which it counts as settling: loose on purpose,
/// for every pile (see [`PILE_SLEEP_STEPS`] for why). Card 8-12 saw a capsule rock at 0.1 m/s for
/// thousands of steps; card 8-21 fixed it, and the stricter check is that the mixed pile sleeps
/// ([`the_mixed_pile_comes_to_rest_and_sleeps`]).
const PILE_STEPS: u32 = 800;
const PILE_SETTLING_SPEED: f32 = 0.25;
/// Steps within which the whole sphere pile must be asleep (card 8-33). With the cross-platform
/// determinism feature off the platform's floating-point library (probably its trigonometric
/// functions, not separated from fused multiply-add inside them) decides the last digits of the
/// rotations: the last sphere is asleep at step 800 on Windows and between 800 and 900 on Linux, so
/// 800 had no margin and failed on Linux and ARM64. At 800 steps all but one body must already
/// sleep (the bound before the card, kept for the pile as a whole); by this many all must.
const PILE_SLEEP_STEPS: u32 = 1200;

#[test]
fn the_piles_stay_in_their_box_and_settle() -> Result<(), Failure> {
    for scene in Scene3::ALL
        .iter()
        .filter(|s| s.judgement() == Judgement::Contained)
    {
        let found = run_scene3(*scene, PILE_STEPS, &serial(), true)?;
        assert_eq!(found.escaped, 0, "{}", scene.name());
        assert!(
            found.final_speed < PILE_SETTLING_SPEED,
            "{}: {}",
            scene.name(),
            found.final_speed
        );
    }
    Ok(())
}

#[test]
fn the_sphere_pile_comes_to_rest_and_sleeps() -> Result<(), Failure> {
    let bodies = Scene3::SpherePile.bodies();
    let early = run_scene3(Scene3::SpherePile, PILE_STEPS, &serial(), true)?;
    assert!(
        early.asleep + 1 >= bodies,
        "{} of {bodies} asleep",
        early.asleep
    );
    let found = run_scene3(Scene3::SpherePile, PILE_SLEEP_STEPS, &serial(), true)?;
    assert_eq!(found.asleep, bodies);
    assert!(found.final_speed < REST_SPEED);
    Ok(())
}

/// Card 8-21. The mixed pile used to be awake for 3465 steps: a capsule that touched a box with one
/// point (its other end was 8 mm higher than the 5 mm an end was allowed) rocked at 0.1 m/s, and
/// the whole pile is one island. With both ends kept (an end within the margin of the nearest point
/// is a contact) the pile sleeps at step 683, and its fastest body is at rest at step 800 (0.06 m/s
/// with the old code, which was still awake at step 1500). The bound is that of the sphere pile
/// (1200 steps): the platform's floating-point library moves the last digits of the rotations
/// (card 8-33).
#[test]
fn the_mixed_pile_comes_to_rest_and_sleeps() -> Result<(), Failure> {
    let bodies = Scene3::MixedPile.bodies();
    let found = run_scene3(Scene3::MixedPile, PILE_SLEEP_STEPS, &serial(), true)?;
    assert_eq!(found.asleep, bodies, "{} of {bodies} asleep", found.asleep);
    assert!(found.final_speed < REST_SPEED, "{}", found.final_speed);
    Ok(())
}

/// Cards 9-6 and 9-8. The tests above run the pile of one seed (12), which is on the fast side of
/// the times to sleep: over seeds 1 to 20 the mixed pile was fully asleep at 2000 steps for 12 and,
/// since card 9-9, is for 18 (seeds 3 and 13 stay awake, card 9-10). So these runs take ten seeds
/// each. What must hold for every seed is checked for every seed (nothing leaves the box, nothing
/// blows up, the pile settles under the speed of [`PILE_SETTLING_SPEED`]); that a pile sleeps is
/// statistical (the pile is chaotic and the last digits depend on the platform, card 8-33), so a
/// share of the seeds must sleep, with a margin under the measured one.
const SEEDS: std::ops::RangeInclusive<u64> = 1..=10;

/// The piles that must be asleep of the ten (measured on Windows: 9 of 10 for the sphere pile at
/// 1200 steps, 9 of 10 for the mixed pile at 2000 steps; 4 of 10 for the mixed pile before 9-9).
const SEEDS_ASLEEP: usize = 8;

/// Steps of the mixed pile by which most seeds sleep (measured: the 18 of seeds 1 to 20 that sleep, by step 1492 at the latest).
const MIXED_SLEEP_STEPS: u32 = 2000;

/// Runs the pile of every seed for `steps`; checks what holds for all, returns how many are fully
/// asleep.
fn piles_asleep(scene: Scene3, steps: u32) -> Result<usize, Failure> {
    let mut asleep = 0;
    for seed in SEEDS {
        let found = run_seeded3(scene, seed, steps, &serial(), true)?;
        let name = scene.name();
        assert_eq!(found.escaped, 0, "{name}, seed {seed}");
        assert!(found.peak_speed < SPEED_LIMIT, "{name}, seed {seed}");
        assert!(
            found.final_speed < PILE_SETTLING_SPEED,
            "{name}, seed {seed}: {}",
            found.final_speed
        );
        asleep += usize::from(found.asleep == scene.bodies());
    }
    Ok(asleep)
}

#[test]
fn a_seed_makes_another_pile_and_the_seed_of_the_catalogue_makes_the_catalogue_pile()
-> Result<(), Failure> {
    for scene in [Scene3::SpherePile, Scene3::MixedPile, Scene3::Tower] {
        let catalogue = Scene3World::new(scene, &serial(), true)?.hash();
        let same = Scene3World::seeded(scene, RNG_SEED, &serial(), true)?.hash();
        let other = Scene3World::seeded(scene, RNG_SEED + 1, &serial(), true)?.hash();
        assert_eq!(catalogue, same, "{}", scene.name());
        // Only the piles are random: the tower is the same for every seed.
        assert_eq!(
            catalogue != other,
            scene != Scene3::Tower,
            "{}",
            scene.name()
        );
    }
    Ok(())
}

#[test]
fn the_sphere_piles_of_ten_seeds_stay_in_the_box_and_most_sleep() -> Result<(), Failure> {
    let asleep = piles_asleep(Scene3::SpherePile, PILE_SLEEP_STEPS)?;
    assert!(asleep >= SEEDS_ASLEEP, "{asleep} of 10 asleep");
    Ok(())
}

#[test]
fn the_mixed_piles_of_ten_seeds_stay_in_the_box_and_most_sleep() -> Result<(), Failure> {
    let asleep = piles_asleep(Scene3::MixedPile, MIXED_SLEEP_STEPS)?;
    assert!(asleep >= SEEDS_ASLEEP, "{asleep} of 10 asleep");
    Ok(())
}

#[test]
fn the_chain_keeps_its_links_together_and_swings() -> Result<(), Failure> {
    let mut world = Scene3World::new(Scene3::Chain, &serial(), true)?;
    let start = world.positions()?;
    let mut worst = 0.0_f32;
    for _ in 0..STEPS {
        world.step()?;
        worst = worst.max(world.link_error()?);
    }
    assert!(worst < 0.05 * CHAIN_LINK, "a link stretched by {worst} m");
    let end = world.positions()?;
    let moved = start
        .iter()
        .zip(&end)
        .map(|(a, b)| (*b - *a).length())
        .fold(0.0, f32::max);
    assert!(moved > 1.0, "the chain did not swing: {moved}");
    Ok(())
}

#[test]
fn a_run_takes_the_steps_and_times_each() -> Result<(), Failure> {
    let found = run_scene3(Scene3::Tower, 25, &serial(), false)?;
    assert_eq!(found.step_ns.len(), 25);
    assert!(found.step_ns.iter().all(|ns| *ns > 0.0));
    Ok(())
}

#[test]
fn the_same_scene_gives_the_same_hash_twice_and_scenes_differ() -> Result<(), Failure> {
    let mut seen = Vec::new();
    for scene in Scene3::ALL {
        let first = run(scene)?.hash;
        assert_eq!(first, run(scene)?.hash, "{}", scene.name());
        assert!(!seen.contains(&first), "{} shares a hash", scene.name());
        seen.push(first);
    }
    Ok(())
}

#[test]
fn the_hash_follows_the_state_step_by_step() -> Result<(), Failure> {
    for scene in Scene3::ALL {
        let mut world = Scene3World::new(scene, &serial(), false)?;
        let before = world.hash();
        world.step()?;
        assert_ne!(world.hash(), before, "{}", scene.name());
    }
    Ok(())
}

/// The hash after every tenth step.
fn trace(scene: Scene3, execution: &SolverExecution) -> Result<Vec<u64>, PhysicsError> {
    let mut world = Scene3World::new(scene, execution, true)?;
    let mut hashes = Vec::new();
    for step in 1..=STEPS {
        world.step()?;
        if step % 10 == 0 {
            hashes.push(world.hash());
        }
    }
    Ok(hashes)
}

#[test]
fn every_scene_replays_the_same_at_one_and_at_several_threads() -> Result<(), Failure> {
    for scene in Scene3::ALL {
        let reference = trace(scene, &serial())?;
        for workers in [1, 2, 4] {
            let pool = pooled(workers)?;
            assert_eq!(
                trace(scene, &pool)?,
                reference,
                "{} at {workers} workers",
                scene.name()
            );
            let wide = SolverExecution { simd: true, ..pool };
            assert_eq!(
                trace(scene, &wide)?,
                reference,
                "{} simd at {workers} workers",
                scene.name()
            );
        }
    }
    Ok(())
}

#[test]
fn sleeping_changes_the_run_but_not_its_determinism() -> Result<(), Failure> {
    let awake = run_scene3(Scene3::Pyramid, 200, &serial(), false)?;
    let sleeping = run_scene3(Scene3::Pyramid, 200, &serial(), true)?;
    assert_ne!(awake.hash, sleeping.hash);
    assert_eq!(
        sleeping.hash,
        run_scene3(Scene3::Pyramid, 200, &pooled(4)?, true)?.hash
    );
    Ok(())
}
