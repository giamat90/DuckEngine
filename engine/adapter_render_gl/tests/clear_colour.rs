//! The first visible result of the OpenGL adapter (card 3-9): a window, a 4.5 core context made
//! from its native token, a colour cleared and read back, and a present.
//!
//! The device tests need GLFW, a display and an OpenGL 4.5 driver; without them they print
//! `not run: device not set up (...)` and pass. A job that is meant to have a device sets
//! `DUCK_REQUIRE_GL=1`, and then a missing device fails the test instead.

use adapter_render_gl::GlContext;
use adapter_window_glfw::GlfwWindows;
use platform::error::PlatformError;
use platform::window::{EventQueue, IWindowSystem, NativeToken, WindowDesc};

/// `GL_LOWER_LEFT` and `GL_ZERO_TO_ONE`: the clip space the engine uses.
const LOWER_LEFT: u32 = 0x8CA1;
const ZERO_TO_ONE: u32 = 0x935F;

/// A context is tied to its thread, so the type must not be `Send`: this fails to compile
/// ("type annotations needed") if `GlContext` ever becomes `Send`.
const _: fn() = || {
    trait AmbiguousIfSend<A> {
        fn some() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    let _ = <GlContext as AmbiguousIfSend<_>>::some;
};

/// Whether `DUCK_REQUIRE_GL` asks for a device: set to anything but empty or `0`.
fn device_required() -> bool {
    std::env::var("DUCK_REQUIRE_GL").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Reports why a device test cannot run: a failure when a device is required, else a note.
fn not_run(reason: &str) {
    assert!(
        !device_required(),
        "an OpenGL device is required here but is not set up: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
}

fn near(actual: [u8; 4], wanted: [u8; 4]) -> bool {
    actual.iter().zip(wanted).all(|(a, w)| a.abs_diff(w) <= 1)
}

#[test]
fn the_null_token_names_no_window() {
    assert!(matches!(
        GlContext::create(NativeToken::NULL),
        Err(PlatformError::NotFound)
    ));
}

#[test]
fn a_window_gets_a_core_context_that_clears_reads_back_and_presents() {
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let desc = WindowDesc {
        title: "DuckEngine 3-9: clear colour",
        width: 320,
        height: 200,
    };
    let Ok(id) = windows.create_window(&desc) else {
        return not_run("no window");
    };
    let Ok(token) = windows.native_token(id) else {
        return not_run("no native token");
    };
    let context = match GlContext::create(token) {
        Ok(context) => context,
        Err(error) => return not_run(&format!("OpenGL 4.5 context: {error}")),
    };
    let info = context.info();
    eprintln!(
        "GL_VENDOR {:?}, GL_RENDERER {:?}, GL_VERSION {:?}",
        info.vendor, info.renderer, info.version_text
    );
    assert_eq!(context.take_error(), 0, "no error after making the context");
    assert_eq!(
        context.clip_control_state(),
        [LOWER_LEFT, ZERO_TO_ONE],
        "the engine's clip space (bottom-left origin, depth 0 to 1) is set"
    );

    let Ok(size) = windows.window_info(id) else {
        panic!("the window exists");
    };
    context.set_viewport(size.framebuffer_width, size.framebuffer_height);
    let (x, y) = (size.framebuffer_width / 2, size.framebuffer_height / 2);

    assert_eq!(
        context.viewport(),
        [0, 0, size.framebuffer_width, size.framebuffer_height]
    );
    context.set_viewport(160, 100);
    assert_eq!(context.viewport(), [0, 0, 160, 100]);
    context.set_viewport(size.framebuffer_width, size.framebuffer_height);

    context.clear([0.25, 0.5, 0.75, 1.0]);
    let pixel = context.read_pixel(x, y);
    assert!(near(pixel, [64, 128, 191, 255]), "read back {pixel:?}");

    context.clear([1.0, 0.0, 0.0, 1.0]);
    assert_eq!(context.read_pixel(x, y), [255, 0, 0, 255]);

    let mut queue = EventQueue::with_capacity(64);
    windows.pump(&mut queue);
    assert!(context.present(), "the back buffer is shown");
    assert_eq!(context.take_error(), 0, "no error after the frame");

    // The context goes before the window.
    drop(context);
    assert!(windows.destroy_window(id).is_ok());
}
