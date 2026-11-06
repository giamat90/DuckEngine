//! The text of the report: markdown tables, built from the measured values so that tests can read
//! them. No threshold is applied to a time; the provisional W1 budget is printed next to the
//! median and nothing compares them.

use crate::counters::WorkCounters;
use crate::measure::Measured;
use crate::overhead::{OverheadKind, OverheadRow};
use crate::scaling::ScalingRow;
use crate::scene::{DRIFT_BOUND, SceneRun, boxes_in};
use crate::stats::Summary;
use std::fmt::Write;

/// The provisional budget of W1 in milliseconds per step, printed beside the median (never judged).
pub const W1_BUDGET_MS: f64 = 2.0;

const NANOS_PER_MILLI: f64 = 1e6;

/// Nanoseconds as milliseconds with three decimals.
fn ms(nanos: f64) -> String {
    format!("{:.3}", nanos / NANOS_PER_MILLI)
}

/// Nanoseconds as a whole number.
fn ns(nanos: f64) -> String {
    format!("{nanos:.0}")
}

/// The header of the W1 table.
#[must_use]
pub fn w1_header() -> String {
    format!(
        "| scene | boxes | steps x iterations | min ms | median ms | p95 ms | budget ms (not judged) | max drift m | settled drift m | final speed m/s | standing (< {DRIFT_BOUND} m, at rest) | replay hash |\n|---|---|---|---|---|---|---|---|---|---|---|---|"
    )
}

/// One row of the W1 table.
#[must_use]
pub fn w1_row(label: &str, rows: u8, steps: u32, iterations: u32, measured: &Measured) -> String {
    let run = &measured.run;
    let time = measured.time;
    format!(
        "| {label} | {} | {steps} x {iterations} | {} | {} | {} | {W1_BUDGET_MS:.1} | {:.4} | {:.4} | {:.4} | {} | {:016x} |",
        boxes_in(rows),
        ms(time.min),
        ms(time.median),
        ms(time.p95),
        run.max_drift,
        run.settled_drift,
        run.final_speed,
        if run.is_standing() { "yes" } else { "NO" },
        run.hash,
    )
}

/// The stability table: how far a pyramid of each size moved in the run (one run each, no timing).
#[must_use]
pub fn stability_table(runs: &[(u8, SceneRun)]) -> String {
    let mut text = format!(
        "| rows | boxes | max drift m | settled drift m | final speed m/s | standing (< {DRIFT_BOUND} m, at rest) |
|---|---|---|---|---|---|
"
    );
    for (rows, run) in runs {
        let _ = writeln!(
            text,
            "| {rows} | {} | {:.4} | {:.4} | {:.4} | {} |",
            boxes_in(*rows),
            run.max_drift,
            run.settled_drift,
            run.final_speed,
            if run.is_standing() { "yes" } else { "NO" },
        );
    }
    text
}

/// The header of the counters table.
#[must_use]
pub fn counters_header() -> String {
    "| scene | steps | sub-steps | pairs tested | manifolds built | constraints solved | constraints frozen | colours max | colours mean | overflow | bodies awake (mean) |\n|---|---|---|---|---|---|---|---|---|---|---|".to_owned()
}

/// One row of the counters table: totals over the run, and means where a mean reads better.
#[must_use]
pub fn counters_row(label: &str, c: &WorkCounters) -> String {
    format!(
        "| {label} | {} | {} | {} | {} | {} | {} | {} | {:.2} | {} | {:.1} |",
        c.steps,
        c.sub_steps,
        c.pairs,
        c.manifolds,
        c.constraints_solved,
        c.constraints_frozen,
        c.colours_max,
        c.per_step(c.colours_sum),
        c.overflow,
        c.per_step(c.bodies_awake),
    )
}

/// The scaling table.
#[must_use]
pub fn scaling_table(rows: &[ScalingRow], reference_hash: Option<u64>) -> String {
    let mut text = String::from(
        "| configuration | min ms | median ms | p95 ms | parallel efficiency | same hash as serial scalar |\n|---|---|---|---|---|---|\n",
    );
    for row in rows {
        let efficiency = row
            .efficiency
            .map_or_else(|| "n/a".to_owned(), |e| format!("{e:.2}"));
        let same = if Some(row.hash) == reference_hash {
            "yes"
        } else {
            "NO"
        };
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {efficiency} | {same} |",
            row.label,
            ms(row.time.min),
            ms(row.time.median),
            ms(row.time.p95),
        );
    }
    text
}

/// The name of an overhead scheduler.
fn scheduler_name(workers: Option<u32>) -> String {
    workers.map_or_else(|| "inline".to_owned(), |n| format!("pool {n}"))
}

/// The overhead table: nanoseconds per fork-join call.
#[must_use]
pub fn overhead_table(rows: &[OverheadRow]) -> String {
    let mut text = String::from(
        "| job | scheduler | min ns/call | median ns/call | p95 ns/call |\n|---|---|---|---|---|\n",
    );
    for row in rows {
        let kind = match row.kind {
            OverheadKind::Empty => "empty (1 index)",
            OverheadKind::Tiny => "tiny (64 indices, 8 chunks)",
        };
        let Summary { min, median, p95 } = row.time;
        let _ = writeln!(
            text,
            "| {kind} | {} | {} | {} | {} |",
            scheduler_name(row.workers),
            ns(min),
            ns(median),
            ns(p95),
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(standing: bool) -> Measured {
        let counters = WorkCounters::default();
        Measured {
            time: Summary {
                min: 1_000_000.0,
                median: 2_500_000.0,
                p95: 4_000_000.0,
            },
            samples: 9000,
            run: SceneRun {
                counters,
                max_drift: if standing { 0.01 } else { 0.2 },
                settled_drift: 0.005,
                final_speed: 0.0,
                hash: 0xABC,
                complete: true,
                step_ns: Vec::new(),
            },
        }
    }

    #[test]
    fn a_w1_row_shows_the_times_in_milliseconds_and_the_verdict() {
        let row = w1_row("W1", 44, 300, 30, &measured(true));
        assert!(
            row.contains("| 990 | 300 x 30 | 1.000 | 2.500 | 4.000 | 2.0 |"),
            "{row}"
        );
        assert!(
            row.contains("| yes |") && row.ends_with("0000000000000abc |"),
            "{row}"
        );
    }

    #[test]
    fn a_pyramid_that_moved_is_marked_not_standing() {
        assert!(w1_row("W1", 44, 300, 30, &measured(false)).contains("| NO |"));
    }

    #[test]
    fn the_stability_table_has_a_row_per_size() {
        let table = stability_table(&[(20, measured(true).run), (44, measured(false).run)]);
        assert!(
            table.contains("| 20 | 210 | 0.0100 | 0.0050 | 0.0000 | yes |"),
            "{table}"
        );
        assert!(
            table.contains("| 44 | 990 | 0.2000 | 0.0050 | 0.0000 | NO |"),
            "{table}"
        );
    }

    #[test]
    fn the_headers_have_as_many_columns_as_their_rows() {
        let columns = |text: &str| text.lines().next().map_or(0, |l| l.matches('|').count());
        let w1 = w1_row("W1", 44, 300, 30, &measured(true));
        assert_eq!(columns(&w1_header()), w1.matches('|').count());
        let counters = counters_row("W1", &WorkCounters::default());
        assert_eq!(columns(&counters_header()), counters.matches('|').count());
    }

    #[test]
    fn a_counters_row_shows_totals_and_means() {
        let c = WorkCounters {
            steps: 4,
            sub_steps: 16,
            pairs: 40,
            manifolds: 30,
            constraints_solved: 20,
            constraints_frozen: 10,
            colours_max: 6,
            colours_sum: 18,
            overflow: 1,
            bodies_awake: 10,
        };
        let row = counters_row("S", &c);
        assert_eq!(
            row,
            "| S | 4 | 16 | 40 | 30 | 20 | 10 | 6 | 4.50 | 1 | 2.5 |"
        );
    }

    #[test]
    fn the_scaling_table_marks_a_row_whose_hash_differs() {
        let summary = Summary {
            min: 1e6,
            median: 2e6,
            p95: 3e6,
        };
        let row = |hash, efficiency| ScalingRow {
            label: "pool 2, scalar".to_owned(),
            workers: Some(2),
            simd: false,
            time: summary,
            hash,
            efficiency,
        };
        let table = scaling_table(&[row(7, Some(0.5)), row(8, None)], Some(7));
        assert!(
            table.contains("| pool 2, scalar | 1.000 | 2.000 | 3.000 | 0.50 | yes |"),
            "{table}"
        );
        assert!(table.contains("| n/a | NO |"), "{table}");
    }

    #[test]
    fn the_overhead_table_names_job_and_scheduler_in_nanoseconds() {
        let summary = Summary {
            min: 1_500.0,
            median: 2_000.0,
            p95: 9_000.0,
        };
        let rows = [
            OverheadRow {
                kind: OverheadKind::Empty,
                workers: None,
                time: summary,
            },
            OverheadRow {
                kind: OverheadKind::Tiny,
                workers: Some(4),
                time: summary,
            },
        ];
        let table = overhead_table(&rows);
        assert!(
            table.contains("| empty (1 index) | inline | 1500 | 2000 | 9000 |"),
            "{table}"
        );
        assert!(
            table.contains("| tiny (64 indices, 8 chunks) | pool 4 | 1500 | 2000 | 9000 |"),
            "{table}"
        );
    }
}
