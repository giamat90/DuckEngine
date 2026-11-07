//! Scheduler overhead: the cost of one empty or tiny fork-join call, inline and on the pool
//! (card 4-22). Times are not asserted (report only); the structure of the table is.

use tools_physics_bench::{BenchError, OverheadKind, WORKER_COUNTS, run_overhead};

#[test]
fn the_table_has_an_inline_row_and_a_pool_row_per_worker_count_for_each_kind()
-> Result<(), BenchError> {
    let rows = run_overhead(200, 3)?;
    let per_kind = 1 + WORKER_COUNTS.len();
    assert_eq!(rows.len(), 2 * per_kind);
    for kind in [OverheadKind::Empty, OverheadKind::Tiny] {
        let of_kind: Vec<_> = rows.iter().filter(|row| row.kind == kind).collect();
        assert_eq!(of_kind.len(), per_kind);
        assert_eq!(of_kind.first().map(|row| row.workers), Some(None));
        for row in of_kind {
            assert!(row.time.min >= 0.0 && row.time.min <= row.time.median);
            assert!(row.time.median <= row.time.p95);
        }
    }
    Ok(())
}

#[test]
fn a_run_with_no_calls_or_no_repetitions_is_refused() {
    assert_eq!(
        run_overhead(0, 3).map(|_| ()),
        Err(BenchError::NothingToMeasure)
    );
    assert_eq!(
        run_overhead(10, 0).map(|_| ()),
        Err(BenchError::NothingToMeasure)
    );
}

#[test]
fn the_tiny_job_runs_every_index_exactly_once_per_call() {
    assert_eq!(
        tools_physics_bench::tiny_job_checksum(),
        u64::from(tools_physics_bench::TINY_COUNT)
            * (u64::from(tools_physics_bench::TINY_COUNT) - 1)
            / 2
    );
}
