//! The mesh image test (card 6-3): two textured cubes are drawn through the mesh frontend, a
//! perspective camera and a depth texture on the OpenGL device, read back, and compared with the
//! pixels the geometry predicts. The near cube is drawn first and the far one second, so only the
//! depth test keeps the near cube in front where they overlap; a control frame with the depth test
//! off shows the far cube winning there, which proves the probe can tell the two apart.
//!
//! The scene, the probes and the comparison are shared with the Vulkan adapter's image test (card
//! 6-11) in `support/mesh_scene.rs`. The shaders are the GLSL of the golden `mesh.dshd` of the mesh
//! sample (the GL device takes GLSL text; the tool chain is not needed). The test needs GLFW, a
//! display and an OpenGL 4.5 driver; without them it prints
//! `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` makes a missing device a
//! failure.

use adapter_render_gl::GlDevice;
use adapter_window_glfw::{GlfwWindows, explain};
use platform::window::{IWindowSystem, WindowDesc};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::ids::SurfaceId;

#[path = "support/mesh_scene.rs"]
mod scene;

const GOLDEN: &[u8] = include_bytes!("../../app_mesh_sample/assets/mesh.dshd");

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
        title: "DuckEngine 6-3: mesh image",
        width: scene::SIDE,
        height: scene::SIDE,
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
            width: scene::SIDE,
            height: scene::SIDE,
        })
        .map_err(|e| format!("surface: {e}"))?;
    Ok(Rig {
        _windows: windows,
        device,
        surface,
    })
}

#[test]
fn two_cubes_render_with_the_near_one_in_front_on_the_gl_device() {
    let mut rig = match open_rig() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let codes = scene::stage_codes(GOLDEN);
    let pixels = scene::render(&mut rig.device, rig.surface, codes, true);
    scene::assert_probes(&pixels);
    assert_eq!(rig.device.take_gl_error(), 0, "no GL error");

    // The control, on the same device (one GLFW user at a time): with the depth test off the far
    // cube, drawn second, wins the overlap.
    let pixels = scene::render(&mut rig.device, rig.surface, codes, false);
    scene::assert_control(&pixels);
    assert_eq!(rig.device.take_gl_error(), 0, "no GL error");
}
