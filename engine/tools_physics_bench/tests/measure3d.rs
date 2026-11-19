//! The timed measurement of the 3D scenes and their scaling rows (card 8-14). Few steps and
//! iterations: the test build is unoptimised; the numbers of the budget note come from release.

use physics::d3::SolverExecution;
use tools_physics_bench::scene3d::{Scene3, run_scene3};
use tools_physics_bench::{BenchError, WORKER_COUNTS, measure_scene3, run_scaling3};

#[test]
fn nothing_to_measure_without_steps_or_iterations() {
    let serial = SolverExecution::SERIAL;
    assert!(matches!(
        measure_scene3(Scene3::Tower, 0, 3, &serial, false),
        Err(BenchError::NothingToMeasure)
    ));
    assert!(matches!(
        measure_scene3(Scene3::Tower, 5, 0, &serial, false),
        Err(BenchError::NothingToMeasure)
    ));
}

#[test]
fn a_measurement_pools_the_steps_of_every_iteration_and_keeps_the_hash() -> Result<(), BenchError> {
    let serial = SolverExecution::SERIAL;
    let measured = measure_scene3(Scene3::Tower, 20, 3, &serial, false)?;
    assert_eq!(measured.samples, 60);
    assert!(measured.time.min <= measured.time.median);
    assert!(measured.time.median <= measured.time.p95);
    assert!(measured.time.min > 0.0);
    let single = run_scene3(Scene3::Tower, 20, &serial, false)?;
    assert_eq!(measured.run.hash, single.hash);
    Ok(())
}

#[test]
fn the_scaling_rows_are_the_ten_configurations_and_all_agree_with_the_serial_scalar_row()
-> Result<(), BenchError> {
    let rows = run_scaling3(Scene3::Pyramid, 15, 2)?;
    assert_eq!(rows.len(), 2 + 2 * WORKER_COUNTS.len());
    let reference = rows.first().map(|row| row.hash);
    assert!(rows.iter().all(|row| Some(row.hash) == reference));
    for row in &rows {
        assert_eq!(
            row.efficiency.is_some(),
            row.workers.is_some(),
            "{}",
            row.label
        );
    }
    Ok(())
}
