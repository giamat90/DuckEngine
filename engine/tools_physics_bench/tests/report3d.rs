//! The 3D section of the bench report (card 8-12): one row per scene of the catalogue.

use physics::d3::SolverExecution;
use tools_physics_bench::BenchError;
use tools_physics_bench::scene3d::{Scene3, run_scene3};
use tools_physics_bench::{
    measure_scene3, scene3_header, scene3_row, scene3_timed_header, scene3_timed_row,
};

type Failure = Box<dyn std::error::Error>;

#[test]
fn the_header_names_the_columns_the_rows_fill() {
    let header = scene3_header();
    let columns = header.lines().next().map_or(0, |l| l.matches('|').count());
    let rule = header.lines().nth(1).map_or(0, |l| l.matches('|').count());
    assert_eq!(columns, rule);
    for word in [
        "scene",
        "bodies",
        "median ms",
        "settled drift",
        "escaped",
        "asleep",
        "replay hash",
    ] {
        assert!(header.contains(word), "{word}");
    }
}

#[test]
fn a_row_prints_the_scene_its_numbers_and_the_hash() -> Result<(), Failure> {
    let run = run_scene3(Scene3::Tower, 30, &SolverExecution::SERIAL, true)?;
    let row = scene3_row(Scene3::Tower, 30, &run);
    assert!(row.starts_with("| tower | 10 | 30 |"), "{row}");
    assert!(row.contains(&format!("{:016x}", run.hash)));
    assert_eq!(
        row.matches('|').count(),
        scene3_header()
            .lines()
            .next()
            .map_or(0, |l| l.matches('|').count())
    );
    Ok(())
}

#[test]
fn a_timed_row_has_the_columns_of_its_header_and_the_median() -> Result<(), BenchError> {
    let measured = measure_scene3(Scene3::Tower, 20, 2, &SolverExecution::SERIAL, false)?;
    let row = scene3_timed_row(Scene3::Tower, 20, 2, &measured);
    let columns = |text: &str| text.lines().next().map_or(0, |l| l.matches('|').count());
    assert_eq!(columns(&row), columns(&scene3_timed_header()));
    assert!(row.starts_with("| tower | 10 | 20 x 2 |"), "{row}");
    assert!(row.contains(&format!("{:016x}", measured.run.hash)));
    assert!(scene3_timed_header().contains("p95 ms"));
    Ok(())
}
