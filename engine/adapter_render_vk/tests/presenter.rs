//! The presenter on a real window and a real device (card 6-7): the swapchain, the frames in
//! flight and the clear colour, read back from the image.
//!
//! The tests need a window system (GLFW), a Vulkan 1.3 loader and a device that can present to
//! the window; without them they print `not run: device not set up (...)` and pass. A job meant to
//! have all of them sets `DUCK_REQUIRE_VK=1`, and then anything missing fails the test instead.
//! The validation layer is switched on when it is installed, and a message from it fails the test.

use adapter_render_vk::{FrameOutcome, Presenter, VulkanContext, VulkanError, VulkanOptions};
use adapter_window_glfw::GlfwWindows;
use platform::window::{EventQueue, IWindowSystem, WindowDesc, WindowId};
use std::sync::{Mutex, PoisonError};

/// GLFW is one library with global state: one test at a time uses it.
static GLFW_IN_USE: Mutex<()> = Mutex::new(());

const WIDTH: u32 = 320;
const HEIGHT: u32 = 200;

/// Whether `DUCK_REQUIRE_VK` asks for a device: set to anything but empty or `0`.
fn device_required() -> bool {
    std::env::var("DUCK_REQUIRE_VK").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Reports why a test cannot run: a failure when a device is required, else a note.
fn not_run(reason: &str) {
    assert!(
        !device_required(),
        "a window and a Vulkan device are required here but are not set up: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
}

/// A window, a context and a presenter on it, kept together so they are dropped in a safe order
/// (presenter, context, then the window system).
struct Rig {
    presenter: Presenter,
    context: VulkanContext,
    windows: GlfwWindows,
    window: WindowId,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Rig {
    /// Everything, or the reason there is no window or device on this machine.
    fn start() -> Result<Self, String> {
        let turn = GLFW_IN_USE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut windows = GlfwWindows::load_default(1).map_err(|e| format!("GLFW: {e}"))?;
        let window = windows
            .create_window(&WindowDesc {
                title: "duckengine-vulkan-test",
                width: WIDTH,
                height: HEIGHT,
            })
            .map_err(|e| format!("window: {e}"))?;
        let token = windows
            .native_token(window)
            .map_err(|e| format!("native token: {e}"))?;
        let context = match VulkanContext::create(VulkanOptions { validation: true }) {
            Ok(context) => context,
            Err(error @ (VulkanError::NoLoader | VulkanError::NoSuitableDevice)) => {
                return Err(error.to_string());
            }
            Err(error) => panic!("starting Vulkan failed: {error}"),
        };
        let info = windows
            .window_info(window)
            .map_err(|e| format!("window info: {e}"))?;
        let presenter = match context.create_presenter(
            token,
            info.framebuffer_width,
            info.framebuffer_height,
        ) {
            Ok(presenter) => presenter,
            Err(error @ VulkanError::Unsupported(_)) => return Err(error.to_string()),
            Err(error) => panic!("making the presenter failed: {error}"),
        };
        Ok(Self {
            presenter,
            context,
            windows,
            window,
            _turn: turn,
        })
    }

    /// Lets the window system run, so the window is shown and resizes are noticed.
    fn pump(&mut self) {
        let mut queue = EventQueue::with_capacity(64);
        self.windows.pump(&mut queue);
    }

    fn frame(&mut self, color: [f32; 4]) -> FrameOutcome {
        self.pump();
        match self.presenter.present_clear(color) {
            Ok(outcome) => outcome,
            Err(error) => panic!("a frame failed: {error}"),
        }
    }

    fn pixel(&self) -> [u8; 4] {
        match self.presenter.captured_pixel() {
            Ok(pixel) => pixel,
            Err(error) => panic!("reading the pixel failed: {error}"),
        }
    }

    fn assert_quiet(&self) {
        if !self.context.info().validation_active {
            eprintln!("note: the validation layer is not installed, so this check proves little");
        }
        let report = self.context.validation_report();
        assert!(report.is_clean(), "validation said: {:#?}", report.messages);
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // The presenter and the context drop before the window system by field order; the window
        // goes first, once nothing presents to it.
        let _ = self.windows.destroy_window(self.window);
    }
}

fn started() -> Option<Rig> {
    match Rig::start() {
        Ok(mut rig) => {
            if let Err(error) = rig.presenter.enable_capture() {
                panic!("capture should work on a real device: {error}");
            }
            Some(rig)
        }
        Err(reason) => {
            not_run(&reason);
            None
        }
    }
}

#[test]
fn a_cleared_frame_shows_its_colour_in_the_middle_of_the_window() {
    let Some(mut rig) = started() else { return };
    // Blue, green, red, alpha bytes of the Bgra8 image.
    for (color, expected) in [
        ([1.0, 0.0, 0.0, 1.0], [0, 0, 255, 255]),
        ([0.0, 1.0, 0.0, 1.0], [0, 255, 0, 255]),
        ([0.0, 0.0, 1.0, 1.0], [255, 0, 0, 255]),
    ] {
        assert_eq!(rig.frame(color), FrameOutcome::Presented);
        assert_eq!(rig.pixel(), expected, "clear {color:?}");
    }
    rig.assert_quiet();
}

#[test]
fn many_frames_in_flight_run_clean_and_show_the_last_colour() {
    let Some(mut rig) = started() else { return };
    for round in 0..120_u32 {
        let shade = f32::from(u8::try_from(round % 2).unwrap_or(0));
        assert_eq!(
            rig.frame([shade, 0.0, 1.0 - shade, 1.0]),
            FrameOutcome::Presented
        );
    }
    assert_eq!(
        rig.pixel(),
        [0, 0, 255, 255],
        "the last frame was round 119: shade 1, so red (blue, green, red, alpha bytes)"
    );
    rig.assert_quiet();
}

#[test]
fn the_presenter_keeps_drawing_after_a_resize_request() {
    let Some(mut rig) = started() else { return };
    let (width, height) = rig.presenter.extent();
    assert!(width > 0 && height > 0);
    assert!(rig.presenter.image_count() >= 2);
    assert_eq!(rig.frame([0.0, 0.0, 1.0, 1.0]), FrameOutcome::Presented);
    // A resize request is handed to the presenter, which rebuilds. The window itself is not
    // resized here (the window port has no call for it), so this checks the rebuild path on a
    // real device, not a size change.
    assert_eq!(rig.presenter.resize(width / 2, height / 2), Ok(()));
    for _ in 0..3 {
        let outcome = rig.frame([0.0, 1.0, 0.0, 1.0]);
        assert!(matches!(
            outcome,
            FrameOutcome::Presented | FrameOutcome::Recreated
        ));
    }
    assert_eq!(rig.pixel(), [0, 255, 0, 255]);
    rig.assert_quiet();
}
