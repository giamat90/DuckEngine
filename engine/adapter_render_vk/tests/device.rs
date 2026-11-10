//! The Vulkan render device on a real window and a real GPU (card 6-9): the generic render
//! contract suite, and frames on the backbuffer.
//!
//! The tests need a window system (GLFW), a Vulkan 1.3 loader and a device that can present to
//! the window; without them they print `not run: device not set up (...)` and pass. A job meant to
//! have all of them sets `DUCK_REQUIRE_VK=1`, and then anything missing fails the test instead.
//! The validation layer is switched on when it is installed, and a message from it fails the test.

use adapter_render_vk::{VulkanDevice, VulkanError, VulkanOptions};
use adapter_window_glfw::GlfwWindows;
use platform::window::{IWindowSystem, WindowDesc};
use render::contract::render_device_contract;
use render::desc::{PassDesc, RenderTarget, SurfaceDesc};
use render::device::IRenderDevice;
use render::stream::CommandStream;
use std::sync::{Mutex, PoisonError};

/// GLFW is one library with global state: one test at a time uses it.
static GLFW_IN_USE: Mutex<()> = Mutex::new(());

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

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

/// A device on a window, with the window system kept alive (and dropped last).
struct Rig {
    device: VulkanDevice,
    surface: SurfaceDesc,
    _windows: GlfwWindows,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Rig {
    /// The device and its surface description, or the reason there is no window or device here.
    fn start() -> Result<Self, String> {
        let turn = GLFW_IN_USE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut windows = GlfwWindows::load_default(1).map_err(|e| format!("GLFW: {e}"))?;
        let window = windows
            .create_window(&WindowDesc {
                title: "duckengine-vulkan-device-test",
                width: WIDTH,
                height: HEIGHT,
            })
            .map_err(|e| format!("window: {e}"))?;
        let token = windows
            .native_token(window)
            .map_err(|e| format!("native token: {e}"))?;
        let info = windows
            .window_info(window)
            .map_err(|e| format!("window info: {e}"))?;
        let device = match VulkanDevice::create(token, VulkanOptions { validation: true }) {
            Ok(device) => device,
            Err(error @ (VulkanError::NoLoader | VulkanError::NoSuitableDevice)) => {
                return Err(error.to_string());
            }
            Err(error) => panic!("making the device failed: {error}"),
        };
        Ok(Self {
            device,
            surface: SurfaceDesc {
                native_token: token.0,
                width: info.framebuffer_width,
                height: info.framebuffer_height,
            },
            _windows: windows,
            _turn: turn,
        })
    }

    /// The validation layer reported nothing; says so when the layer was not there to report.
    fn assert_quiet(&self) {
        if !self.device.validation_active() {
            eprintln!("note: the validation layer is not installed, so this run was not validated");
        }
        let report = self.device.validation_report();
        assert!(report.is_clean(), "validation said: {:#?}", report.messages);
    }
}

#[test]
fn the_vulkan_device_passes_the_render_contract_suite() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let caps = rig.device.caps();
    eprintln!(
        "device: {} on {}, caps {caps:?}",
        caps.device_name,
        rig.device.gpu_name()
    );
    assert!(caps.depth_zero_to_one, "Vulkan clips depth to 0..1");
    render_device_contract(&mut rig.device, rig.surface);
    rig.assert_quiet();
}

#[test]
fn many_frames_on_the_backbuffer_are_presented_without_a_validation_message() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let Ok(surface) = rig.device.create_surface(rig.surface) else {
        panic!("the surface should be made");
    };
    for round in 0..8_u8 {
        let shade = f32::from(round) / 8.0;
        let mut stream = CommandStream::new();
        stream.begin_pass(PassDesc {
            target: RenderTarget::Backbuffer,
            clear_color: Some([shade, 0.25, 0.5, 1.0]),
            clear_depth: None,
            depth: None,
        });
        stream.end_pass();
        let Ok(frame) = rig.device.begin_frame(surface) else {
            panic!("frame {round} should begin");
        };
        assert!(rig.device.submit(frame, &stream).is_ok(), "frame {round}");
        assert_eq!(rig.device.end_frame(frame), Ok(()), "frame {round}");
    }
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}
