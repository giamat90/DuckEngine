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
fn an_unknown_backend_exits_two_with_the_usage() {
    let output = run(&["--backend", "metal"]);
    assert_eq!(output.status.code(), Some(EXIT_USAGE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("`--backend metal`"), "{stderr}");
    assert!(stderr.contains("--backend gl|vulkan"), "{stderr}");
}

/// Three frames on the Vulkan device where one exists: `DUCK_REQUIRE_VK=1` or `DUCK_REQUIRE_GLFW=1`
/// makes a missing device a failure. A validation message is a failure of the run (exit 1).
#[test]
fn three_frames_are_drawn_on_vulkan_where_a_device_exists() {
    let output = run(&["--frames", "3", "--backend", "vulkan"]);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    match output.status.code() {
        Some(0) => assert_eq!(stdout.trim(), "drew 3 frames", "{stderr}"),
        Some(EXIT_NO_DEVICE) => {
            assert!(
                !required("DUCK_REQUIRE_VK") && !required("DUCK_REQUIRE_GLFW"),
                "a Vulkan device is required here but is not set up: {stderr}"
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
