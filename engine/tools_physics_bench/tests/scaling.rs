//! The scaling scene and the measurement around it (card 4-22).
//!
//! A pyramid of 10 rows and a few repetitions are used because the test build is unoptimised;
//! the numbers of the report come from the binary in release.

use physics::d2::SolverExecution;
use tools_physics_bench::{
    BenchError, WORKER_COUNTS, measure_w1, parallel_efficiency, run_scaling,
};

const ROWS: u8 = 10;

#[test]
fn the_efficiency_of_a_perfect_split_is_one() {
    assert!((parallel_efficiency(100.0, 25.0, 4) - 1.0).abs() < 1e-12);
}

#[test]
fn the_efficiency_of_no_speed_up_on_four_workers_is_a_quarter() {
    assert!((parallel_efficiency(100.0, 100.0, 4) - 0.25).abs() < 1e-12);
}

#[test]
fn the_efficiency_of_a_zero_time_or_zero_workers_is_zero() {
    assert!(parallel_efficiency(100.0, 0.0, 4).abs() < f64::EPSILON);
    assert!(parallel_efficiency(100.0, 25.0, 0).abs() < f64::EPSILON);
}

#[test]
fn the_worker_counts_are_the_ones_of_owner_choice_o10() {
    assert_eq!(WORKER_COUNTS, [1, 2, 4, 6]);
}

#[test]
fn every_row_of_the_scaling_scene_ends_in_the_hash_of_the_serial_reference()
-> Result<(), BenchError> {
    let rows = run_scaling(ROWS, 40, 2)?;
    // serial scalar, serial lanes, then scalar and lanes at 1, 2, 4 and 6 workers
    assert_eq!(rows.len(), 2 + 2 * WORKER_COUNTS.len());
    let reference = rows.first().map(|row| row.hash);
    assert!(reference.is_some());
    for row in &rows {
        assert_eq!(Some(row.hash), reference, "{} differs", row.label);
        assert!(row.time.median > 0.0, "{} has no time", row.label);
    }
    Ok(())
}

#[test]
fn the_pool_rows_report_an_efficiency_against_one_worker_and_the_serial_rows_none()
-> Result<(), BenchError> {
    let rows = run_scaling(ROWS, 40, 2)?;
    for row in &rows {
        if row.workers.is_none() {
            assert!(row.efficiency.is_none(), "{}", row.label);
            continue;
        }
        let efficiency = row.efficiency.unwrap_or(0.0);
        assert!(efficiency > 0.0, "{} has no efficiency", row.label);
        if row.workers == Some(1) {
            assert!((efficiency - 1.0).abs() < 1e-12, "{}", row.label);
        }
    }
    Ok(())
}

#[test]
fn the_w1_measurement_pools_the_steps_of_every_timed_iteration() -> Result<(), BenchError> {
    let measured = measure_w1(ROWS, 30, 3, &SolverExecution::SERIAL, false)?;
    assert_eq!(measured.samples, 3 * 30);
    assert!(measured.time.min > 0.0);
    assert!(measured.time.min <= measured.time.median);
    assert!(measured.time.median <= measured.time.p95);
    Ok(())
}
