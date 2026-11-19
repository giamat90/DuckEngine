//! The image test of the lit shader (card 8-11): a sphere and a box, lit by one directional light,
//! are drawn by the renderer of the sample on the OpenGL device into an offscreen target, read
//! back, and checked for what the shader is meant to do with the two blocks: the side of the
//! sphere that faces the light is brighter than the side that does not, and the side that does
//! not is not black (the ambient term); the colour of each object comes from its own block; the
//! depth test puts the near box in front.
//!
//! The test needs GLFW, a display and an OpenGL 4.5 driver; without them it prints
//! `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` makes a missing device a
//! failure.

use adapter_render_gl::GlDevice;
use adapter_render_vk::VulkanDevice;
use app_sandbox3d::mesh::{BOX, SPHERE};
use app_sandbox3d::renderer::{LIT_GLSL, LIT_SPIRV, Renderer};
use app_sprite_sample::app::Opened;
use app_sprite_sample::backend::Backend;
use app_sprite_sample::cli::RunError;
use assets::dshd::Program;
use base::math::{Mat4, Vec3};
use render::camera::PerspectiveCamera;
use render::desc::{SurfaceDesc, TextureDesc, TextureFormat};
use render::lit::{DirectionalLight, Material};
use render::mesh_scene::{MeshInstance, MeshRef, MeshScene};

/// GLFW is one per process: the two tests open their windows one after the other.
static ONE_WINDOW: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Side of the offscreen image, in pixels.
const SIDE: u32 = 64;
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

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

/// The scene: a red sphere at the origin and a small green box in front of it, seen from +Z, lit
/// from the +X side (the light travels towards -X).
fn scene(texture: render::ids::TextureId) -> Result<MeshScene, String> {
    let camera = PerspectiveCamera {
        position: Vec3::new(0.0, 0.0, 6.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: 0.96,
        aspect: 1.0,
        near: 0.1,
        far: 100.0,
    };
    let light = DirectionalLight {
        direction: Vec3::new(-1.0, 0.0, -0.2),
        colour: [1.0, 1.0, 1.0],
        ambient: [0.2, 0.2, 0.2],
    };
    let mut scene = MeshScene::new(4, camera, light).map_err(|e| format!("{e:?}"))?;
    let instances = [
        (SPHERE, Mat4::from_scale(Vec3::splat(1.5)), RED),
        (
            BOX,
            Mat4::from_translation(Vec3::new(0.0, 0.0, 2.5)) * Mat4::from_scale(Vec3::splat(0.4)),
            GREEN,
        ),
    ];
    for (mesh, model, colour) in instances {
        scene
            .push(MeshInstance {
                mesh: MeshRef(mesh),
                model,
                material: Material { colour },
                texture,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(scene)
}

fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * SIDE + x) * 4) as usize;
    [image[at], image[at + 1], image[at + 2], image[at + 3]]
}

/// The sum of the red of the sphere's pixels in the columns `from..to`, where a sphere pixel is
/// one that is red and not green or blue, and the number of such pixels.
fn sphere_red(image: &[u8], from: u32, to: u32) -> (u32, u32) {
    let reds = (0..SIDE)
        .flat_map(|y| (from..to).map(move |x| (x, y)))
        .map(|(x, y)| pixel(image, x, y))
        .filter(|[r, g, b, _]| *r >= 4 && *g == 0 && *b == 0)
        .map(|[r, ..]| u32::from(r));
    reds.fold((0, 0), |(sum, count), r| (sum + r, count + 1))
}

/// Draws the scene on the device `D` into an offscreen image and reads it back; `None` after
/// saying why not when there is no device.
fn render<D: Backend>(asset: &[u8], title: &'static str) -> Option<Vec<u8>> {
    let _only = ONE_WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut opened = match Opened::<D>::open_with(title) {
        Ok(opened) => opened,
        Err(RunError::NoDevice(reason)) => {
            not_run(&reason);
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
    let scene = scene(renderer.texture()).unwrap_or_else(|e| panic!("{e}"));
    renderer
        .draw(device, &scene)
        .unwrap_or_else(|e| panic!("{e}"));
    let mut image = vec![0u8; (SIDE * SIDE * 4) as usize];
    device
        .read_back(target, &mut image)
        .unwrap_or_else(|e| panic!("{e:?}"));
    device.check().unwrap_or_else(|e| panic!("{e}"));
    Some(image)
}

/// What the shader is meant to do with the two blocks, read off the image.
fn assert_lit(image: &[u8]) {
    // Depth: the green box is in front of the middle of the sphere.
    let [r, g, b, _] = pixel(image, SIDE / 2, SIDE / 2);
    assert!(
        g >= 8 && r == 0 && b == 0,
        "the box is in front: {r} {g} {b}"
    );
    // The corner shows the clear colour.
    let corner = pixel(image, 0, 0);
    assert!(
        corner[2] > corner[0],
        "cleared to the dark blue: {corner:?}"
    );
    // The side that faces the light (+X is the right of the image) is brighter than the other,
    // which is not black.
    let (lit, lit_count) = sphere_red(image, SIDE / 2 + 8, SIDE);
    let (shaded, shaded_count) = sphere_red(image, 0, SIDE / 2 - 8);
    assert!(
        lit_count > 50 && shaded_count > 50,
        "{lit_count} {shaded_count}"
    );
    // Means compared as products: lit / lit_count > 1.5 * shaded / shaded_count.
    assert!(
        lit * shaded_count * 2 > shaded * lit_count * 3,
        "lit {lit} over {lit_count}, shaded {shaded} over {shaded_count}"
    );
    assert!(shaded > 0, "the ambient term lights the dark side");
}

#[test]
fn a_lit_scene_shows_the_light_the_colours_and_the_depth_on_the_gl_device() {
    if let Some(image) = render::<GlDevice>(LIT_GLSL, "DuckEngine 8-11: lit image (GL)") {
        assert_lit(&image);
    }
}

#[test]
fn a_lit_scene_shows_the_light_the_colours_and_the_depth_on_the_vulkan_device() {
    if let Some(image) = render::<VulkanDevice>(LIT_SPIRV, "DuckEngine 8-11: lit image (VK)") {
        assert_lit(&image);
    }
}
