//! The mesh image test (card 6-3): two textured cubes are drawn through the mesh frontend, a
//! perspective camera and a depth texture on the OpenGL device, read back, and compared with the
//! pixels the geometry predicts. The near cube is drawn first and the far one second, so only the
//! depth test keeps the near cube in front where they overlap; a control frame with the depth test
//! off shows the far cube winning there, which proves the probe can tell the two apart.
//!
//! Rows of the read-back image are stored bottom-up (row 0 is the bottom of the picture). The
//! shaders are inline GLSL 450 (the GL device takes GLSL text; the tool chain is not needed). The
//! test needs GLFW, a display and an OpenGL 4.5 driver; without them it prints
//! `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` makes a missing device a
//! failure.

use adapter_render_gl::GlDevice;
use adapter_window_glfw::{GlfwWindows, explain};
use base::math::{Mat4, Vec3};
use platform::window::{IWindowSystem, WindowDesc};
use render::camera::{PerspectiveCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc,
    Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc,
    TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SurfaceId, TextureId};
use render::mesh::{
    CUBE_INDEX_COUNT, MESH_VERTEX_STRIDE, MeshBindings, cube_indices, cube_vertices,
    encode_mesh_vertices, mesh_vertex_attributes, record_mesh,
};
use render::stream::CommandStream;

const SIDE: u32 = 64;
const CLEAR: [u8; 4] = [0, 0, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

const VERTEX_SHADER: &str = "#version 450
layout(binding = 0, std140) uniform Transform { mat4 mvp; };
layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;
layout(location = 0) out vec2 v_uv;
void main() {
    gl_Position = mvp * vec4(in_pos, 1.0);
    v_uv = in_uv;
}
";

const FRAGMENT_SHADER: &str = "#version 450
layout(binding = 0) uniform sampler2D tex;
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv);
}
";

/// A pixel to look at (from the bottom-left corner) and the colour it must have when the depth
/// test is on.
///
/// The camera is at (0, 0, 10) looking down -Z with a quarter-turn vertical field of view, so at
/// distance `d` the view is `d` units from the middle to each edge. The near cube (edge 4, centre
/// (-1.5, 0, 0), front face at distance 8) covers pixels x 18..34, y 24..40 with its 2x2 texture;
/// the far cube (edge 4, centre (1.5, 0, -3), front face at distance 11, one solid colour) covers
/// x 31..42, y 27..37. They overlap at x 31..34.
const PROBES: [(u32, u32, [u8; 4], &str); 8] = [
    (2, 2, CLEAR, "clear colour outside both cubes"),
    (21, 36, RED, "near cube, upper left texel"),
    (28, 36, GREEN, "near cube, upper right texel"),
    (21, 28, BLUE, "near cube, lower left texel"),
    (28, 28, YELLOW, "near cube, lower right texel"),
    (38, 32, MAGENTA, "far cube alone"),
    (
        32,
        34,
        GREEN,
        "overlap: the near cube stays in front of the far one",
    ),
    (
        32,
        28,
        YELLOW,
        "overlap, lower right texel of the near cube",
    ),
];

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

fn must<T>(result: Result<T, RenderError>) -> T {
    assert_eq!(result.as_ref().err(), None);
    result.unwrap_or_else(|_| unreachable!())
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

fn buffer(device: &mut GlDevice, usage: BufferUsage, bytes: &[u8]) -> BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let id = must(device.create_buffer(BufferDesc { size, usage }));
    must(device.write_buffer(id, 0, bytes));
    id
}

fn texture(device: &mut GlDevice, side: u32, format: TextureFormat, pixels: &[u8]) -> TextureId {
    let id = must(device.create_texture(TextureDesc {
        width: side,
        height: side,
        mip_count: 1,
        format,
    }));
    if !pixels.is_empty() {
        let all = Rect {
            x: 0,
            y: 0,
            width: side,
            height: side,
        };
        must(device.write_texture(id, all, pixels));
    }
    id
}

fn pipeline(device: &mut GlDevice, depth_on: bool) -> PipelineId {
    let mut shader = |stage, code: &str| {
        must(device.create_shader(ShaderDesc {
            stage,
            code: code.as_bytes(),
            entry: "main",
        }))
    };
    let vertex_shader = shader(ShaderStage::Vertex, VERTEX_SHADER);
    let fragment_shader = shader(ShaderStage::Fragment, FRAGMENT_SHADER);
    must(device.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: MESH_VERTEX_STRIDE,
        attributes: &mesh_vertex_attributes(),
        blend: BlendMode::Opaque,
        depth: DepthState {
            test: depth_on,
            write: depth_on,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: depth_on.then_some(TextureFormat::Depth32F),
    }))
}

/// The camera of the probe table.
fn camera() -> PerspectiveCamera {
    PerspectiveCamera {
        position: Vec3::new(0.0, 0.0, 10.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: core::f32::consts::FRAC_PI_2,
        aspect: 1.0,
        near: 0.5,
        far: 50.0,
    }
}

/// `place * scale(4)`: a cube of edge 4 centred at `centre`.
fn model(centre: Vec3) -> Mat4 {
    Mat4::from_translation(centre) * Mat4::from_scale(Vec3::new(4.0, 4.0, 4.0))
}

/// Records the near cube, then the far one, into one pass on `target`.
fn record_scene(device: &mut GlDevice, target: TextureId, depth_on: bool) -> CommandStream {
    let mut vertex_bytes = vec![0u8; cube_vertices().len() * MESH_VERTEX_STRIDE as usize];
    assert!(encode_mesh_vertices(&cube_vertices(), &mut vertex_bytes));
    let index_bytes: Vec<u8> = cube_indices()
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    let (vertices, indices) = (
        buffer(device, BufferUsage::Vertex, &vertex_bytes),
        buffer(device, BufferUsage::Index, &index_bytes),
    );
    let pipeline = pipeline(device, depth_on);
    let sampler = must(device.create_sampler(SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }));
    let near_texture = texture(
        device,
        2,
        TextureFormat::Rgba8,
        &[RED, GREEN, BLUE, YELLOW].concat(),
    );
    let far_texture = texture(device, 1, TextureFormat::Rgba8, &MAGENTA);
    let draw = |device: &mut GlDevice, centre: Vec3, texture: TextureId| {
        let mvp = camera()
            .mvp(&model(centre))
            .unwrap_or_else(|| unreachable!());
        MeshBindings {
            pipeline,
            vertices,
            uniforms: buffer(device, BufferUsage::Uniform, &matrix_bytes(&mvp)),
            indices,
            texture,
            sampler,
        }
    };
    let near = draw(device, Vec3::new(-1.5, 0.0, 0.0), near_texture);
    let far = draw(device, Vec3::new(1.5, 0.0, -3.0), far_texture);
    let depth = depth_on.then(|| texture_of_depth(device));
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(target),
        clear_color: Some([0.0, 0.0, 1.0, 1.0]),
        clear_depth: Some(1.0),
        depth,
    });
    stream.set_viewport(Rect {
        x: 0,
        y: 0,
        width: SIDE,
        height: SIDE,
    });
    record_mesh(&mut stream, &near, CUBE_INDEX_COUNT);
    record_mesh(&mut stream, &far, CUBE_INDEX_COUNT);
    stream.end_pass();
    stream
}

fn texture_of_depth(device: &mut GlDevice) -> TextureId {
    texture(device, SIDE, TextureFormat::Depth32F, &[])
}

/// Renders the scene and returns the read-back pixels, bottom row first.
fn render(rig: &mut Rig, depth_on: bool) -> Vec<u8> {
    let device = &mut rig.device;
    let target = texture(device, SIDE, TextureFormat::Rgba8, &[]);
    let stream = record_scene(device, target, depth_on);
    let frame = must(device.begin_frame(rig.surface));
    let stats = must(device.submit(frame, &stream));
    must(device.end_frame(frame));
    assert_eq!(stats.draws, 2);
    let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
    must(device.read_back(target, &mut pixels));
    assert_eq!(device.take_gl_error(), 0, "no GL error");
    pixels
}

fn at(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let first = ((y * SIDE + x) * 4) as usize;
    [
        pixels[first],
        pixels[first + 1],
        pixels[first + 2],
        pixels[first + 3],
    ]
}

#[test]
fn two_cubes_render_with_the_near_one_in_front_on_the_gl_device() {
    let mut rig = match open_rig() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let pixels = render(&mut rig, true);
    let wrong: Vec<_> = PROBES
        .iter()
        .filter(|&&(x, y, want, _)| at(&pixels, x, y) != want)
        .map(|&(x, y, want, what)| (x, y, at(&pixels, x, y), want, what))
        .collect();
    assert!(
        wrong.is_empty(),
        "wrong pixels (x, y, got, want, what): {wrong:?}"
    );

    // The control, on the same device (one GLFW user at a time): with the depth test off the far
    // cube, drawn second, wins the overlap. It makes the overlap probe above mean something.
    let pixels = render(&mut rig, false);
    assert_eq!(at(&pixels, 32, 34), MAGENTA, "control: far cube over near");
    assert_eq!(
        at(&pixels, 21, 36),
        RED,
        "control: near cube outside the overlap"
    );
}
