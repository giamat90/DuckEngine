//! What the mip chain of the checker texture does to the sandbox's own floor (card 8-62): the
//! floor block of the scenery is drawn by the renderer of the sample on a real device, once with
//! the texture the renderer makes (all its levels) and once with the same checker as a single
//! level, from the start view of the sandbox and from the lowest, farthest view the orbit camera
//! allows (two views, one yaw; the views between them are not sampled). Measured: from the start view not one pixel differs, because the floor is not
//! minified there (a texel is 1.5 m, about 20 pixels); only at the horizon of the extreme view do
//! the levels change the picture. So from the start view a shimmer of the floor is not a missing
//! level. The light is the test's own and the walls are not drawn.
//!
//! The test needs GLFW, a display and an OpenGL 4.5 or Vulkan driver; without them it prints
//! `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` and `DUCK_REQUIRE_VK=1`
//! make a missing device a failure.

use adapter_render_gl::GlDevice;
use adapter_render_vk::VulkanDevice;
use app_sandbox3d::mesh::SCENERY_FIRST;
use app_sandbox3d::orbit::{MAX_DISTANCE, MIN_PITCH, OrbitCamera};
use app_sandbox3d::renderer::{LIT_GLSL, LIT_SPIRV, Renderer};
use app_sandbox3d::spawn::scenery;
use app_sprite_sample::app::Opened;
use app_sprite_sample::backend::Backend;
use app_sprite_sample::cli::RunError;
use app_sprite_sample::scene::{CHECKER_SIDE, checker_pixels};
use assets::dshd::Program;
use base::math::{Mat4, Vec3};
use render::desc::{Rect, SurfaceDesc, TextureDesc, TextureFormat};
use render::device::IRenderDevice;
use render::ids::TextureId;
use render::lit::{DirectionalLight, Material};
use render::mesh_scene::{MeshInstance, MeshRef, MeshScene};

/// GLFW is one per process: the two tests open their windows one after the other.
static ONE_WINDOW: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Side of the offscreen image, in pixels.
const SIDE: u32 = 256;
/// The start view of the sandbox: where it looks, and the yaw, pitch and distance. A copy of the
/// private `CAMERA_*` constants of `sandbox.rs` (they are not public): change them together.
const START_TARGET: Vec3 = Vec3::new(0.0, 1.5, 0.0);
const START_YAW: f32 = 0.6;
const START_PITCH: f32 = 0.55;
const START_DISTANCE: f32 = 18.0;
/// Floor pixels the start view must show for the comparison to mean something (measured 22282).
const MIN_FLOOR_PIXELS: usize = 10_000;
/// Pixels the levels must change at the extreme view (measured 437 of 1367).
const MIN_CHANGED_AT_THE_HORIZON: usize = 100;

fn device_required(variable: &str) -> bool {
    std::env::var(variable).is_ok_and(|value| !value.is_empty() && value != "0")
}

/// The checker of the sandbox as a texture of one level.
fn single_level_checker(device: &mut dyn IRenderDevice) -> TextureId {
    let texture = device
        .create_texture(TextureDesc {
            width: CHECKER_SIDE,
            height: CHECKER_SIDE,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let level0 = Rect {
        x: 0,
        y: 0,
        width: CHECKER_SIDE,
        height: CHECKER_SIDE,
    };
    device
        .write_texture(texture, level0, &checker_pixels())
        .unwrap_or_else(|e| panic!("{e:?}"));
    texture
}

/// What a floor picture is drawn with.
struct Rig<'a> {
    device: &'a mut dyn IRenderDevice,
    renderer: Renderer,
    target: TextureId,
}

impl Rig<'_> {
    /// The floor block of the scenery drawn with `texture` from `view`, read back.
    fn floor(&mut self, texture: TextureId, view: OrbitCamera) -> Vec<u8> {
        let floor = scenery()[0];
        let light = DirectionalLight {
            direction: Vec3::new(-1.0, -1.0, -0.5),
            colour: [1.0; 3],
            ambient: [0.3; 3],
        };
        let mut scene =
            MeshScene::new(1, view.camera(1.0), light).unwrap_or_else(|e| panic!("{e:?}"));
        scene
            .push(MeshInstance {
                mesh: MeshRef(SCENERY_FIRST),
                model: Mat4::from_translation(floor.position) * Mat4::from_scale(floor.half),
                material: Material { colour: [1.0; 4] },
                texture,
            })
            .unwrap_or_else(|e| panic!("{e:?}"));
        self.renderer
            .draw(self.device, &scene)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut image = vec![0u8; (SIDE * SIDE * 4) as usize];
        self.device
            .read_back(self.target, &mut image)
            .unwrap_or_else(|e| panic!("{e:?}"));
        image
    }
}

/// The floor seen from each of `views`, with the renderer's texture and with the single-level
/// checker; `None` after saying why not when there is no device.
fn floor_pictures<D: Backend>(
    asset: &[u8],
    title: &'static str,
    variable: &str,
    views: [OrbitCamera; 2],
) -> Option<[[Vec<u8>; 2]; 2]> {
    let _only = ONE_WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut opened = match Opened::<D>::open_with(title) {
        Ok(opened) => opened,
        Err(RunError::NoDevice(reason)) => {
            assert!(
                !device_required(variable),
                "a device is required here but is not set up: {reason}"
            );
            eprintln!("not run: device not set up ({reason})");
            return None;
        }
        Err(other) => panic!("{other}"),
    };
    let token = opened.token.0;
    let device = &mut opened.device;
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: token,
            width: SIDE,
            height: SIDE,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let program = Program::parse(asset).unwrap_or_else(|e| panic!("{e:?}"));
    let target = device
        .create_texture(TextureDesc {
            width: SIDE,
            height: SIDE,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut renderer =
        Renderer::new(device, &program, surface, (SIDE, SIDE)).unwrap_or_else(|e| panic!("{e}"));
    renderer.set_target(target);
    let chain = renderer.texture();
    let single = single_level_checker(device);
    let mut rig = Rig {
        device: &mut *device,
        renderer,
        target,
    };
    let pictures = views.map(|view| [rig.floor(chain, view), rig.floor(single, view)]);
    device.check().unwrap_or_else(|e| panic!("{e}"));
    Some(pictures)
}

/// The pixels that are not the clear colour (the one at the corner), and how many of them differ
/// between the two pictures, and the largest difference of any channel.
fn compare(pictures: &[Vec<u8>; 2]) -> (usize, usize, u8) {
    let clear = &pictures[0][..4];
    let drawn = pictures[0]
        .chunks_exact(4)
        .filter(|pixel| *pixel != clear)
        .count();
    let differing = pictures[0]
        .chunks_exact(4)
        .zip(pictures[1].chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    let largest = pictures[0]
        .iter()
        .zip(&pictures[1])
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    (drawn, differing, largest)
}

fn start_view() -> OrbitCamera {
    OrbitCamera::new(START_TARGET, START_YAW, START_PITCH, START_DISTANCE)
}

fn horizon_view() -> OrbitCamera {
    OrbitCamera::new(START_TARGET, START_YAW, MIN_PITCH, MAX_DISTANCE)
}

fn assert_floor_is_not_minified_at_the_start_and_is_at_the_horizon(pictures: &[[Vec<u8>; 2]; 2]) {
    let (drawn, differing, _) = compare(&pictures[0]);
    assert!(drawn > MIN_FLOOR_PIXELS, "the floor is drawn: {drawn}");
    assert_eq!(differing, 0, "start view: the levels change nothing");
    let (_, changed, _) = compare(&pictures[1]);
    assert!(
        changed > MIN_CHANGED_AT_THE_HORIZON,
        "horizon: the levels change {changed} pixels"
    );
}

#[test]
fn the_mip_levels_change_nothing_of_the_floor_from_the_start_view_on_the_gl_device() {
    let title = "DuckEngine 8-62: floor mips (GL)";
    let views = [start_view(), horizon_view()];
    if let Some(pictures) = floor_pictures::<GlDevice>(LIT_GLSL, title, "DUCK_REQUIRE_GL", views) {
        assert_floor_is_not_minified_at_the_start_and_is_at_the_horizon(&pictures);
    }
}

#[test]
fn the_mip_levels_change_nothing_of_the_floor_from_the_start_view_on_the_vulkan_device() {
    let title = "DuckEngine 8-62: floor mips (VK)";
    let views = [start_view(), horizon_view()];
    if let Some(pictures) =
        floor_pictures::<VulkanDevice>(LIT_SPIRV, title, "DUCK_REQUIRE_VK", views)
    {
        assert_floor_is_not_minified_at_the_start_and_is_at_the_horizon(&pictures);
    }
}
