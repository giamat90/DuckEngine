//! The 3D sandbox: `app_sandbox3d [--frames N] [--null] [--vulkan]`.
//!
//! Opens a window; spheres, boxes and capsules fall into a walled box, every one an entity with a
//! transform, a mesh and a body, drawn lit. 1, 2, 3 choose the piece, Space or the primary button
//! drops it, the secondary button and a drag orbit the camera, the wheel zooms, R resets, Escape or
//! closing the window ends it. `--null` runs the same loop on the Null render device without a
//! window (needs `--frames`); `--vulkan` draws with the Vulkan 1.3 device instead of OpenGL 4.5
//! (not with `--null`). Needs GLFW (`DUCK_GLFW_LIB` or `glfw3.dll` / `libglfw.so.3`), a
//! display and an OpenGL 4.5 driver for the window; without them it prints `not run: device not
//! set up (...)` and exits with code 3. Exit codes: 0 done, 1 failed, 2 bad arguments, 3 no device.

use app_sandbox3d::app;
use app_sandbox3d::cli::{EXIT_OK, execute};
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
