//! The large pyramid of the 3D benchmark (card 8-14): about a thousand boxes, the 3D counterpart of
//! the W1 pyramid. It is a bench scene only: it is not in the catalogue the stability tests judge,
//! because it is too large for the unoptimised test build to run for hundreds of steps.

use physics::d3::SolverExecution;
use tools_physics_bench::make_pool;
use tools_physics_bench::scene3d::{Scene3, Scene3World, run_scene3};

type Failure = Box<dyn std::error::Error>;

#[test]
fn the_large_pyramid_has_a_thousand_boxes_and_is_not_in_the_catalogue() {
    assert_eq!(Scene3::LargePyramid.bodies(), 1015);
    assert!(!Scene3::ALL.contains(&Scene3::LargePyramid));
    assert_eq!(Scene3::LargePyramid.name(), "large-pyramid");
}

#[test]
fn the_large_pyramid_is_built_with_every_box_and_no_overflow() -> Result<(), Failure> {
    let world = Scene3World::new(Scene3::LargePyramid, &SolverExecution::SERIAL, false)?;
    assert_eq!(world.positions()?.len(), 1015);
    let found = run_scene3(Scene3::LargePyramid, 10, &SolverExecution::SERIAL, false)?;
    assert!(found.complete);
    assert!(found.peak_speed < 1.0, "{}", found.peak_speed);
    Ok(())
}

#[test]
fn the_large_pyramid_replays_the_same_on_the_pool() -> Result<(), Failure> {
    let serial = run_scene3(Scene3::LargePyramid, 10, &SolverExecution::SERIAL, false)?;
    let pool = SolverExecution {
        scheduler: Some(make_pool(4).map_err(|e| format!("{e:?}"))?),
        ..SolverExecution::SERIAL
    };
    assert_eq!(
        run_scene3(Scene3::LargePyramid, 10, &pool, false)?.hash,
        serial.hash
    );
    Ok(())
}
