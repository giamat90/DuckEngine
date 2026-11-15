//! The benchmark's workloads agree with themselves: the serial and the parallel path give the
//! same checksum, the checksum depends on the data, and the command line is parsed strictly.

use jobs::reversed::ReversedScheduler;
use tools_ecs_bench::{Command, Kind, Options, build, parse, run_parallel, run_serial};

#[test]
fn the_world_holds_the_asked_number_of_entities_in_three_tables_and_the_empty_one() {
    let Ok(world) = build(300) else {
        panic!("the world is built");
    };
    assert_eq!(world.len(), 300);
    // The empty table every spawn starts in, and the three the workloads read.
    assert_eq!(world.archetype_count(), 4);
}

#[test]
fn each_workload_is_repeatable() {
    for kind in Kind::ALL {
        let (Ok(mut a), Ok(mut b)) = (build(500), build(500)) else {
            panic!("the worlds are built");
        };
        let first = run_serial(&mut a, kind, 3);
        assert!(first.is_ok());
        assert_eq!(first, run_serial(&mut b, kind, 3));
    }
}

#[test]
fn the_parallel_path_gives_the_serial_checksum() {
    for kind in Kind::ALL {
        let (Ok(mut serial), Ok(mut parallel)) = (build(900), build(900)) else {
            panic!("the worlds are built");
        };
        let want = run_serial(&mut serial, kind, 2);
        let got = run_parallel(&mut parallel, kind, 2, &ReversedScheduler, 16);
        assert!(want.is_ok());
        assert_eq!(got, want, "{kind:?}");
    }
}

#[test]
fn the_checksum_sees_the_data() {
    let (Ok(mut few), Ok(mut many)) = (build(100), build(200)) else {
        panic!("the worlds are built");
    };
    assert_ne!(
        run_serial(&mut few, Kind::Integrate, 1),
        run_serial(&mut many, Kind::Integrate, 1)
    );
}

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(ToString::to_string).collect()
}

#[test]
fn a_report_command_has_defaults_and_options() {
    assert_eq!(
        parse(&args(&["report"])),
        Ok(Command::Report(Options::default()))
    );
    let Ok(Command::Report(options)) =
        parse(&args(&["report", "--entities", "1000", "--iters", "5"]))
    else {
        panic!("a report command");
    };
    assert_eq!(options.entities, 1000);
    assert_eq!(options.iters, 5);
}

#[test]
fn the_instruction_count_workload_is_parsed() {
    assert_eq!(
        parse(&args(&[
            "icount-workload",
            "--entities",
            "2048",
            "--frames",
            "4"
        ])),
        Ok(Command::IcountWorkload {
            entities: 2048,
            frames: 4
        })
    );
}

#[test]
fn bad_arguments_are_refused() {
    assert!(parse(&args(&[])).is_err());
    assert!(parse(&args(&["nope"])).is_err());
    assert!(parse(&args(&["report", "--entities"])).is_err());
    assert!(parse(&args(&["report", "--entities", "x"])).is_err());
    assert!(parse(&args(&["report", "--entities", "0"])).is_err());
    assert!(parse(&args(&["report", "--what", "1"])).is_err());
}
