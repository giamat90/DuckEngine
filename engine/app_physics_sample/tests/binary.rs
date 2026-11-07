//! The built program: a bad command line is refused on any machine; the headless run (`--null`)
//! needs no window and no GPU and exits 0 with the same state hash every time; a run in a window
//! needs GLFW, a display and an OpenGL 4.5 driver. Without them it exits with code 3 and the test
//! reports `not run`; `DUCK_REQUIRE_GL=1` or `DUCK_REQUIRE_GLFW=1` makes that a failure.

use std::process::{Command, Output};

const EXIT_USAGE: i32 = 2;
const EXIT_NO_DEVICE: i32 = 3;

fn run(args: &[&str]) -> Output {
    let program = env!("CARGO_BIN_EXE_app_physics_sample");
    let output = Command::new(program).args(args).output();
    assert!(output.is_ok(), "{output:?}");
    output.unwrap_or_else(|_| unreachable!())
}

fn required(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.is_empty() && value != "0")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_bad_frame_count_exits_two_with_the_usage() {
    let output = run(&["--frames", "0"]);
    assert_eq!(output.status.code(), Some(EXIT_USAGE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage: app_physics_sample"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn an_unknown_argument_and_a_null_run_without_frames_exit_two() {
    assert_eq!(run(&["--nope"]).status.code(), Some(EXIT_USAGE));
    assert_eq!(run(&["--null"]).status.code(), Some(EXIT_USAGE));
}

#[test]
fn the_headless_run_draws_every_frame_and_exits_zero_with_a_stable_hash() {
    let first = run(&["--null", "--frames", "200"]);
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    let line = stdout(&first);
    assert!(line.starts_with("drew 200 frames, state hash "), "{line}");
    assert_eq!(stdout(&run(&["--null", "--frames", "200"])), line);
    assert_ne!(stdout(&run(&["--null", "--frames", "201"])), line);
}

#[test]
fn three_frames_are_drawn_in_a_window_where_a_device_exists() {
    let output = run(&["--frames", "3"]);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    match output.status.code() {
        Some(0) => assert!(stdout.starts_with("drew 3 frames"), "{stdout} {stderr}"),
        Some(EXIT_NO_DEVICE) => {
            assert!(
                !required("DUCK_REQUIRE_GL") && !required("DUCK_REQUIRE_GLFW"),
                "a device is required here but is not set up: {stderr}"
            );
            assert!(
                stderr.starts_with("not run: device not set up ("),
                "{stderr}"
            );
            eprintln!("{}", stderr.trim());
        }
        other => panic!("exit code {other:?}: {stdout} {stderr}"),
    }
}

#[test]
fn the_pyramid_scene_and_its_sub_steps_run_headless_with_their_own_stable_hashes() {
    let hash = |args: &[&str]| {
        let output = run(args);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        stdout(&output)
    };
    let stream = hash(&["--null", "--frames", "30"]);
    let pyramid = hash(&["--null", "--frames", "30", "--scene", "pyramid"]);
    let fine = hash(&[
        "--null",
        "--frames",
        "30",
        "--scene",
        "pyramid",
        "--sub-steps",
        "8",
    ]);
    assert_eq!(
        pyramid,
        hash(&["--null", "--frames", "30", "--scene", "pyramid"])
    );
    assert_ne!(pyramid, stream);
    assert_ne!(pyramid, fine);
    assert_eq!(
        stream,
        hash(&["--null", "--frames", "30", "--scene", "stream"])
    );
}

#[test]
fn a_bad_scene_or_sub_step_count_exits_two() {
    assert_eq!(run(&["--scene", "castle"]).status.code(), Some(EXIT_USAGE));
    assert_eq!(run(&["--sub-steps", "0"]).status.code(), Some(EXIT_USAGE));
}
