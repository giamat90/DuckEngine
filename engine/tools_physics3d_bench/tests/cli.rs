//! Card 8-29: the command line of the 3D benchmark tool. The instruction-count gate calls
//! `icount-workload --layers N --steps N` and reads one line back.

use tools_physics3d_bench::{Workload, parse, run};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|text| (*text).to_owned()).collect()
}

#[test]
fn the_workload_command_reads_layers_and_steps() {
    assert_eq!(
        parse(&args(&[
            "icount-workload",
            "--layers",
            "6",
            "--steps",
            "40"
        ])),
        Ok(Workload {
            layers: 6,
            steps: 40
        })
    );
}

#[test]
fn the_flags_may_come_in_either_order() {
    assert_eq!(
        parse(&args(&[
            "icount-workload",
            "--steps",
            "40",
            "--layers",
            "6"
        ])),
        parse(&args(&[
            "icount-workload",
            "--layers",
            "6",
            "--steps",
            "40"
        ]))
    );
}

#[test]
fn a_missing_flag_an_unknown_flag_or_a_bad_number_is_refused() {
    for bad in [
        &["icount-workload", "--layers", "6"][..],
        &["icount-workload", "--steps", "40"],
        &["icount-workload", "--layers", "x", "--steps", "40"],
        &["icount-workload", "--layers", "300", "--steps", "40"],
        &["icount-workload", "--layers", "41", "--steps", "40"],
        &["icount-workload", "--layers", "6", "--steps", "1000001"],
        &[
            "icount-workload",
            "--layers",
            "6",
            "--steps",
            "40",
            "--rows",
            "3",
        ],
        &["report"],
        &[],
    ] {
        assert!(parse(&args(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn a_pyramid_of_no_layers_runs_and_has_no_boxes() {
    let line = run(Workload {
        layers: 0,
        steps: 3,
    });
    assert!(
        line.as_deref()
            .is_ok_and(|text| text.starts_with("icount-workload layers=0 steps=3 bodies=0 hash="))
    );
}

#[test]
fn running_a_workload_prints_one_line_with_the_hash() {
    let line = run(Workload {
        layers: 3,
        steps: 5,
    });
    assert!(
        line.as_deref()
            .is_ok_and(|text| text.starts_with("icount-workload layers=3 steps=5 bodies=14 hash="))
    );
}
