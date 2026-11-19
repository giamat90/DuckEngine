//! The 3D stability and determinism scenes (card 8-12): every scene of the catalogue stands, stays
//! in its box or hangs together as it should, never blows up, and gives the same state hash twice
//! and at one and at several threads (D0 and D1).

use physics::d3::{PhysicsError, SolverExecution};
use tools_physics_bench::make_pool;
use tools_physics_bench::scene3d::{
    CHAIN_LINK, Judgement, REST_SPEED, Run3, SPEED_LIMIT, STANDING_DRIFT, Scene3, Scene3World,
    run_scene3,
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

/// Steps a pile needs to settle, and the speed under which it counts as settling (a capsule on a
/// sphere rocks at about 0.1 m/s for a long time: finding of card 8-12, see the review).
const PILE_STEPS: u32 = 800;
const PILE_SETTLING_SPEED: f32 = 0.25;

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
    let found = run_scene3(Scene3::SpherePile, PILE_STEPS, &serial(), true)?;
    assert_eq!(found.asleep, Scene3::SpherePile.bodies());
    assert!(found.final_speed < REST_SPEED);
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
