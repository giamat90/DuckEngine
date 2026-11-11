//! The built program: a bad command line is refused on any machine; a real run needs GLFW, a
//! display and a driver. Without them it exits with code 3 and the test reports `not run`;
//! `DUCK_REQUIRE_GL`, `DUCK_REQUIRE_VK` or `DUCK_REQUIRE_GLFW` make that a failure. On the Vulkan
//! device a validation message fails the run (exit 1).

use std::process::{Command, Output};

const EXIT_USAGE: i32 = 2;
const EXIT_NO_DEVICE: i32 = 3;

fn run(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_app_mesh_sample"))
        .args(args)
        .output();
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
    assert!(output.stdout.is_empty());
}

#[test]
fn an_unknown_backend_exits_two() {
    let output = run(&["--backend", "metal"]);
    assert_eq!(output.status.code(), Some(EXIT_USAGE));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--backend gl|vulkan"), "{stderr}");
}

fn three_frames(backend: &str, need: &str) {
    let output = run(&["--frames", "3", "--backend", backend]);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    match output.status.code() {
        Some(0) => assert_eq!(stdout.trim(), "drew 3 frames", "{stderr}"),
        Some(EXIT_NO_DEVICE) => {
            assert!(
                !required(need) && !required("DUCK_REQUIRE_GLFW"),
                "a {backend} device is required here but is not set up: {stderr}"
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
fn three_frames_are_drawn_on_gl_where_a_device_exists() {
    three_frames("gl", "DUCK_REQUIRE_GL");
}

#[test]
fn three_frames_are_drawn_on_vulkan_where_a_device_exists() {
    three_frames("vulkan", "DUCK_REQUIRE_VK");
}
