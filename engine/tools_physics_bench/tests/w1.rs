//! Workload W1 and its work counters (card 4-22).
//!
//! The assertions use a small pyramid (10 rows, 55 boxes) and the 300 steps of W1, because the
//! test build is unoptimised and the full pyramid (44 rows, 990 boxes) takes minutes there. The
//! full size is run by the binary in release; its result is in `docs/budgets/physics-benchmark-4-22.md`.

use physics::d2::{PhysicsError, SolverExecution};
use tools_physics_bench::{
    DRIFT_BOUND, REST_SPEED, W1_STEPS, WorkCounters, boxes_in, icount_workload, run_scene,
};

const SMALL_ROWS: u8 = 10;

#[test]
fn a_pyramid_of_n_rows_has_n_times_n_plus_one_over_two_boxes() {
    assert_eq!(boxes_in(10), 55);
    assert_eq!(boxes_in(44), 990);
    assert_eq!(boxes_in(0), 0);
}

#[test]
fn the_small_pyramid_is_still_standing_after_300_steps() -> Result<(), PhysicsError> {
    let run = run_scene(SMALL_ROWS, W1_STEPS, &SolverExecution::SERIAL, false)?;
    assert!(
        run.max_drift < DRIFT_BOUND,
        "drift {} m is not below {DRIFT_BOUND} m",
        run.max_drift
    );
    assert!(
        run.final_speed < REST_SPEED,
        "still moving: {}",
        run.final_speed
    );
    assert_eq!(run.counters.steps, u64::from(W1_STEPS));
    Ok(())
}

#[test]
fn the_work_counters_of_sixty_steps_are_pinned() -> Result<(), PhysicsError> {
    let run = run_scene(SMALL_ROWS, 60, &SolverExecution::SERIAL, false)?;
    let expected = WorkCounters {
        steps: 60,
        sub_steps: 240,
        pairs: 8700,
        manifolds: 8700,
        constraints_solved: 8700,
        constraints_frozen: 0,
        colours_max: 6,
        colours_sum: 360,
        overflow: 0,
        bodies_awake: 3300,
    };
    assert!(run.complete, "a list overflowed, work was dropped");
    assert_eq!(run.counters, expected);
    Ok(())
}

#[test]
fn with_sleeping_on_the_pyramid_falls_asleep_and_its_constraints_freeze() -> Result<(), PhysicsError>
{
    let run = run_scene(SMALL_ROWS, W1_STEPS, &SolverExecution::SERIAL, true)?;
    let awake_without_sleep = 55 * u64::from(W1_STEPS);
    assert!(run.counters.bodies_awake < awake_without_sleep);
    assert!(run.counters.constraints_frozen > 0);
    let expected = WorkCounters {
        steps: 300,
        sub_steps: 1200,
        pairs: 43500,
        manifolds: 43500,
        constraints_solved: 5220,
        constraints_frozen: 38280,
        colours_max: 6,
        colours_sum: 216,
        overflow: 0,
        bodies_awake: 1925,
    };
    assert!(run.complete, "a list overflowed, work was dropped");
    assert_eq!(run.counters, expected);
    Ok(())
}

#[test]
fn the_untimed_workload_for_an_instruction_counter_does_the_same_work_as_the_timed_run()
-> Result<(), PhysicsError> {
    let timed = run_scene(SMALL_ROWS, 60, &SolverExecution::SERIAL, false)?;
    let (counters, hash) = icount_workload(SMALL_ROWS, 60)?;
    assert_eq!(counters, timed.counters);
    assert_eq!(hash, timed.hash);
    Ok(())
}
