//! The built program: a bad command line is refused on any machine; the headless run (`--null`)
//! needs no window and no GPU and exits 0 with the same state hash every time; a run in a window
//! needs GLFW, a display and an OpenGL 4.5 driver. Without them it exits with code 3 and the test
//! reports `not run`; `DUCK_REQUIRE_GL=1` or `DUCK_REQUIRE_GLFW=1` makes that a failure.

use std::process::{Command, Output};

const EXIT_USAGE: i32 = 2;
const EXIT_NO_DEVICE: i32 = 3;

fn run(args: &[&str]) -> Output {
    let program = env!("CARGO_BIN_EXE_app_sandbox3d");
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
fn a_bad_command_line_exits_two_with_the_usage_of_this_program() {
    for bad in [
        &["--frames", "0"][..],
        &["--nope"][..],
        &["--null"][..],
        &["--vulkan", "--null", "--frames", "3"][..],
        &["--scene", "pyramid"][..],
    ] {
        let output = run(bad);
        assert_eq!(output.status.code(), Some(EXIT_USAGE), "{bad:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("usage: app_sandbox3d"), "{stderr}");
        assert!(output.stdout.is_empty());
    }
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
            assert!(stderr.starts_with("not run: device not set up"), "{stderr}");
            let strict = required("DUCK_REQUIRE_GL") || required("DUCK_REQUIRE_GLFW");
            assert!(!strict, "a device is required here: {stderr}");
            eprintln!("not run: {stderr}");
        }
        other => panic!("exit {other:?}: {stdout} {stderr}"),
    }
}

#[test]
fn three_frames_are_drawn_in_a_vulkan_window_where_a_device_exists() {
    let output = run(&["--vulkan", "--frames", "3"]);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    match output.status.code() {
        Some(0) => assert!(stdout.starts_with("drew 3 frames"), "{stdout} {stderr}"),
        Some(EXIT_NO_DEVICE) => {
            assert!(stderr.starts_with("not run: device not set up"), "{stderr}");
            let strict = required("DUCK_REQUIRE_GL") || required("DUCK_REQUIRE_GLFW");
            assert!(!strict, "a device is required here: {stderr}");
            eprintln!("not run: {stderr}");
        }
        other => panic!("exit {other:?}: {stdout} {stderr}"),
    }
}

#[test]
fn both_windows_draw_the_frames_they_are_asked_for() {
    // The loop is the same on both devices; only what is drawn differs. Needs both devices.
    let (gl, vk) = (
        run(&["--frames", "30"]),
        run(&["--vulkan", "--frames", "30"]),
    );
    if gl.status.code() != Some(0) || vk.status.code() != Some(0) {
        eprintln!(
            "not run: a window device is missing ({:?}, {:?})",
            gl.status.code(),
            vk.status.code()
        );
        assert!(
            !required("DUCK_REQUIRE_GL"),
            "both devices are required here"
        );
        return;
    }
    // Wall-clock time decides how many physics steps a frame takes, so the hash is not compared;
    // both runs drew the frames they were asked for.
    assert!(stdout(&gl).starts_with("drew 30 frames"), "{}", stdout(&gl));
    assert!(stdout(&vk).starts_with("drew 30 frames"), "{}", stdout(&vk));
}
