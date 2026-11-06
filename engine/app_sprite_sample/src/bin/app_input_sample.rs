//! The interactive example: `app_input_sample [--frames N]`.
//!
//! Opens a window; the arrow keys or WASD move a checker sprite, the mouse places a small square
//! and a left click changes its colour; Escape or closing the window ends it. Needs GLFW
//! (`DUCK_GLFW_LIB` or `glfw3.dll` / `libglfw.so.3`), a display and an OpenGL 4.5 driver; without
//! them it prints `not run: device not set up (...)` and exits with code 3. Exit codes as in
//! `app_sprite_sample`: 0 done, 1 failed, 2 bad arguments, 3 no device.

use app_sprite_sample::app;
use app_sprite_sample::cli::{EXIT_OK, execute};
use std::process::ExitCode;

fn main() -> ExitCode {
    let outcome = execute(std::env::args().skip(1), app::run_interactive);
    if outcome.code == EXIT_OK {
        println!("{}", outcome.message);
    } else {
        eprintln!("{}", outcome.message);
    }
    ExitCode::from(outcome.code)
}
