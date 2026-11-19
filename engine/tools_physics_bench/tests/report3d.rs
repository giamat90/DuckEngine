//! The 3D section of the bench report (card 8-12): one row per scene of the catalogue.

use physics::d3::SolverExecution;
use tools_physics_bench::scene3d::{Scene3, run_scene3};
use tools_physics_bench::{scene3_header, scene3_row};

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
