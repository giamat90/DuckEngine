//! Opens a window and clears it to a slowly changing colour for three seconds (card 3-9, the
//! first visible result of the OpenGL adapter). Close the window to stop early.
//!
//! Needs GLFW (`DUCK_GLFW_LIB` or `glfw3.dll` / `libglfw.so.3` on the search path), a display and
//! an OpenGL 4.5 driver; without them it prints `not run: device not set up (...)`.
//!
//! ```text
//! cargo run -p adapter_render_gl --example clear_colour
//! ```

use adapter_render_gl::GlContext;
use adapter_window_glfw::{GlfwWindows, explain};
use platform::window::{Event, EventQueue, IWindowSystem, WindowDesc};
use std::time::{Duration, Instant};

const RUN_FOR: Duration = Duration::from_secs(3);
const FRAME: Duration = Duration::from_millis(16);

fn main() {
    if let Err(reason) = run() {
        println!("not run: device not set up ({reason})");
    }
}

/// Runs the demo; the error is the reason it could not start.
fn run() -> Result<(), String> {
    let mut windows = GlfwWindows::load_default(1).map_err(explain)?;
    let desc = WindowDesc {
        title: "DuckEngine 3-9: clear colour",
        width: 640,
        height: 360,
    };
    let id = windows
        .create_window(&desc)
        .map_err(|e| format!("window: {e}"))?;
    let token = windows
        .native_token(id)
        .map_err(|e| format!("token: {e}"))?;
    let context = GlContext::create(token).map_err(|e| format!("OpenGL 4.5 context: {e}"))?;
    let info = context.info();
    println!(
        "{} / {} / {}",
        info.vendor, info.renderer, info.version_text
    );

    let mut queue = EventQueue::with_capacity(64);
    let start = Instant::now();
    let mut open = true;
    while open && start.elapsed() < RUN_FOR {
        windows.pump(&mut queue);
        while let Some(event) = queue.pop() {
            open &= !matches!(event, Event::CloseRequested { .. } | Event::Quit);
        }
        frame(&windows, &context, id, start.elapsed());
        std::thread::sleep(FRAME);
    }
    drop(context);
    Ok(())
}

/// Draws one frame: the whole window in a colour that depends on the time.
fn frame(
    windows: &GlfwWindows,
    context: &GlContext,
    id: platform::window::WindowId,
    elapsed: Duration,
) {
    if let Ok(size) = windows.window_info(id) {
        context.set_viewport(size.framebuffer_width, size.framebuffer_height);
    }
    let phase = elapsed.as_secs_f32().fract();
    context.clear([phase, 0.4, 1.0 - phase, 1.0]);
    if !context.present() {
        println!("present failed");
    }
}
