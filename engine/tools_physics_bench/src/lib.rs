//! The physics benchmark of card 4-22: workload W1 (a pyramid of boxes), a scaling scene on the
//! work-stealing pool, scheduler overhead, work counters and the instruction-count investigation.
//! Dev-time only; the engine does not depend on it. Report only: no time is judged here.
//!
//! The scene is deterministic (no random numbers), so the work counters and the replay hash are
//! the same on every run of a build; times are not and are reported with their spread.

mod cli;
mod counters;
mod error;
mod icount;
mod measure;
mod overhead;
mod report;
mod scaling;
mod scene;
mod stats;

pub use cli::{Command, Options, USAGE, parse};
pub use counters::WorkCounters;
pub use error::BenchError;
pub use icount::{
    FEASIBLE_MAX, GateFeasibility, IcountError, NOISE_SHARE, Tool, WARN_FRACTION, gate_feasibility,
    parse_instructions, relative_spread,
};
pub use measure::{MIN_ITERATIONS, Measured, measure_w1};
pub use overhead::{
    OverheadKind, OverheadRow, TINY_COUNT, TINY_GRAIN, run_overhead, tiny_job_checksum,
};
pub use report::{
    W1_BUDGET_MS, counters_header, counters_row, overhead_table, scaling_table, stability_table,
    w1_header, w1_row,
};
pub use scaling::{ScalingRow, WORKER_COUNTS, make_pool, parallel_efficiency, run_scaling};
pub use scene::{
    DRIFT_BOUND, DT, HALF, NANOS_PER_SECOND, PyramidScene, REST_SPEED, SETTLED_FROM, SceneRun,
    W1_ROWS, W1_STEPS, boxes_in, icount_workload, run_scene,
};
pub use stats::{Summary, summarize};
