//! The built program: a bad command line is refused on any machine; a real run needs GLFW, a
//! display and an OpenGL 4.5 driver. Without them it exits with code 3 and the test reports
//! `not run`; `DUCK_REQUIRE_GL=1` or `DUCK_REQUIRE_GLFW=1` makes that a failure.

use std::process::{Command, Output};

const EXIT_USAGE: i32 = 2;
const EXIT_NO_DEVICE: i32 = 3;

fn run(args: &[&str]) -> Output {
    let program = env!("CARGO_BIN_EXE_app_sprite_sample");
    let output = Command::new(program).args(args).output();
    assert!(output.is_ok(), "{output:?}");
    output.unwrap_or_else(|_| unreachable!())
}

fn required(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.is_empty() && value != "0")
}

#[test]
fn a_bad_frame_count_exits_two_with_the_usage() {
    let output = run(&["--frames", "0"]);
    assert_eq!(output.status.code(), Some(EXIT_USAGE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage: app_sprite_sample"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn an_unknown_argument_exits_two() {
    assert_eq!(run(&["--nope"]).status.code(), Some(EXIT_USAGE));
}

#[test]
fn three_frames_are_drawn_where_a_device_exists() {
    let output = run(&["--frames", "3"]);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    match output.status.code() {
        Some(0) => assert_eq!(stdout.trim(), "drew 3 frames", "{stderr}"),
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
