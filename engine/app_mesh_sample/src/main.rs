//! The mesh sample: `app_mesh_sample [--frames N] [--backend gl|vulkan]`.
//!
//! Opens a window and draws a spinning textured cube with a depth buffer until the window is closed
//! or `N` frames are drawn. Needs GLFW (`DUCK_GLFW_LIB` or `glfw3.dll` / `libglfw.so.3`), a display
//! and an OpenGL 4.5 or Vulkan 1.3 driver; without them it prints `not run: device not set up (...)`
//! and exits with code 3. Exit codes as in `app_sprite_sample`: 0 done, 1 failed, 2 bad arguments,
//! 3 no device.

use app_mesh_sample::app;
use app_sprite_sample::cli::{EXIT_OK, execute};
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
