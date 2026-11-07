//! The falling-boxes sample: `app_physics_sample [--frames N] [--null]`.
//!
//! Opens a window; boxes and discs fall into a walled arena under the 2D physics world, drawn with
//! the sprite batcher, with the physics debug drawing (outlines, centres, contact points and
//! normals) on top. Click to drop a body at the cursor, R resets, D toggles the debug drawing,
//! Escape or closing the window ends it. `--null` runs the same loop on the Null render device
//! without a window (needs `--frames`). Needs GLFW (`DUCK_GLFW_LIB` or `glfw3.dll` /
//! `libglfw.so.3`), a display and an OpenGL 4.5 driver for the window; without them it prints
//! `not run: device not set up (...)` and exits with code 3. Exit codes: 0 done, 1 failed, 2 bad
//! arguments, 3 no device.

use app_physics_sample::app;
use app_physics_sample::cli::{EXIT_OK, execute};
use std::process::ExitCode;

fn main() -> ExitCode {
    let outcome = execute(std::env::args().skip(1), app::run);
    if outcome.code == EXIT_OK {
        println!("{}", outcome.message);
    } else {
        eprintln!("{}", outcome.message);
    }
    ExitCode::from(outcome.code)
}
