//! The image test (card 3-15, split off from 3-13): an offscreen sprite scene is rendered by the
//! OpenGL device through the sprite batcher, the orthographic camera, the render graph and the
//! golden `sprite.dshd` shader, read back, and compared with a reference image painted on the CPU.
//! The scene and the reference are shared with the Vulkan adapter's image test (card 6-10).
//!
//! The test needs GLFW, a display and an OpenGL 4.5 driver; without them it prints
//! `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` makes a missing device a
//! failure. The reference painter and the comparison run everywhere.

use adapter_render_gl::GlDevice;
use adapter_window_glfw::{GlfwWindows, explain};
use platform::window::{IWindowSystem, WindowDesc};
use render::desc::{SurfaceDesc, TextureDesc, TextureFormat};
use render::device::IRenderDevice;
use render::ids::SurfaceId;

#[path = "../../render/tests/support/sprite_image.rs"]
mod scene;

use scene::{
    CLEAR, DRAWS, EXACT, GREEN, HALF_WHITE_OVER_BLUE, HALF_WHITE_OVER_RED, RED, ROUNDING, SIDE,
    YELLOW, assert_reference, mismatches, must, paint_reference, record_scene, stage_codes,
};

const GOLDEN: &[u8] = include_bytes!("../../tools_shaderc/golden/sprite.dshd");

#[test]
fn the_comparison_finds_pixels_outside_their_tolerance() {
    let expected = [(RED, EXACT), (RED, ROUNDING), (RED, ROUNDING), (RED, EXACT)];
    let mut actual = Vec::new();
    actual.extend_from_slice(&RED);
    actual.extend_from_slice(&[253, 2, 0, 255]);
    actual.extend_from_slice(&[252, 0, 0, 255]);
    actual.extend_from_slice(&[255, 1, 0, 255]);
    let found = mismatches(&actual, &expected);
    let indices: Vec<usize> = found.iter().map(|m| m.0).collect();
    assert_eq!(indices, [2, 3]);
}

#[test]
fn the_reference_image_is_painted_back_to_front() {
    let image = paint_reference();
    let at = |x: u32, y: u32| image[(y * SIDE + x) as usize].0;
    assert_eq!(at(0, 0), CLEAR);
    assert_eq!(at(20, 10), RED);
    assert_eq!(at(10, 10), HALF_WHITE_OVER_RED);
    assert_eq!(at(20, 20), GREEN);
    assert_eq!(at(10, 20), RED);
    assert_eq!(at(44, 52), RED);
    assert_eq!(at(52, 44), YELLOW);
    assert_eq!(at(45, 10), HALF_WHITE_OVER_BLUE);
}

fn device_required() -> bool {
    std::env::var("DUCK_REQUIRE_GL").is_ok_and(|value| !value.is_empty() && value != "0")
}

fn not_run(reason: &str) {
    assert!(
        !device_required(),
        "an OpenGL device is required here but is not set up: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
}

/// The device on its window, the surface, and the window system kept so the window outlives the
/// device (fields drop in order: the device first).
struct Rig {
    device: GlDevice,
    surface: SurfaceId,
    _windows: GlfwWindows,
}

fn open_rig() -> Result<Rig, String> {
    let mut windows = GlfwWindows::load_default(1).map_err(explain)?;
    let desc = WindowDesc {
        title: "DuckEngine 3-15: sprite image",
        width: SIDE,
        height: SIDE,
    };
    let id = windows
        .create_window(&desc)
        .map_err(|e| format!("window: {e}"))?;
    let token = windows
        .native_token(id)
        .map_err(|e| format!("native token: {e}"))?;
    let mut device = GlDevice::create(token).map_err(|e| format!("OpenGL 4.5 device: {e}"))?;
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: token.0,
            width: SIDE,
            height: SIDE,
        })
        .map_err(|e| format!("surface: {e}"))?;
    Ok(Rig {
        _windows: windows,
        device,
        surface,
    })
}

#[test]
fn a_sprite_scene_renders_the_expected_pixels_on_the_gl_device() {
    let mut rig = match open_rig() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let device = &mut rig.device;
    let target = must(device.create_texture(TextureDesc {
        width: SIDE,
        height: SIDE,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let stream = record_scene(device, target, stage_codes(GOLDEN));
    let frame = must(device.begin_frame(rig.surface));
    let stats = must(device.submit(frame, &stream));
    must(device.end_frame(frame));
    assert_eq!(stats.draws, DRAWS);

    let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
    must(device.read_back(target, &mut pixels));
    assert_reference(&pixels);
    assert_eq!(device.take_gl_error(), 0, "no GL error");
}
