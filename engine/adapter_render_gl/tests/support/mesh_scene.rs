//! The mesh image scene shared by the image tests of the OpenGL and the Vulkan adapter (cards 6-3
//! and 6-11): two textured cubes drawn through the mesh frontend, a perspective camera and a depth
//! texture, the probe table, and the comparison. A test includes it with `#[path]` and brings its
//! own device, window and shader code (GLSL text or SPIR-V).
//!
//! The near cube is drawn first and the far one second, so only the depth test keeps the near cube
//! in front where they overlap; a control frame with the depth test off shows the far cube winning
//! there, which proves the probe can tell the two apart. Rows of the read-back image are stored
//! bottom-up (row 0 is the bottom of the picture).

use assets::dshd::{Module, Program, Stage};
use base::math::{Mat4, Vec3};
use render::camera::{PerspectiveCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc,
    Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SurfaceId, TextureId};
use render::mesh::{
    CUBE_INDEX_COUNT, MESH_VERTEX_STRIDE, MeshBindings, cube_indices, cube_vertices,
    encode_mesh_vertices, mesh_vertex_attributes, record_mesh,
};
use render::stream::CommandStream;

pub const SIDE: u32 = 64;
pub const CLEAR: [u8; 4] = [0, 0, 255, 255];
pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const GREEN: [u8; 4] = [0, 255, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];
pub const YELLOW: [u8; 4] = [255, 255, 0, 255];
pub const MAGENTA: [u8; 4] = [255, 0, 255, 255];

pub fn must<T>(result: Result<T, RenderError>) -> T {
    assert_eq!(result.as_ref().err(), None);
    result.unwrap_or_else(|_| unreachable!())
}

/// The code of one module: SPIR-V when the asset holds it, else GLSL text.
fn stage_code(module: Module<'_>) -> &[u8] {
    module.spirv().unwrap_or_else(|| module.code().as_bytes())
}

/// The code of the vertex and the fragment module of a `mesh.dshd` asset: GLSL text or SPIR-V,
/// whichever the file holds.
pub fn stage_codes(asset: &[u8]) -> (&[u8], &[u8]) {
    let program = Program::parse(asset).unwrap_or_else(|_| unreachable!());
    let (Some(vertex), Some(fragment)) = (
        program.module_for(Stage::Vertex),
        program.module_for(Stage::Fragment),
    ) else {
        panic!("the asset lacks a stage");
    };
    (stage_code(vertex), stage_code(fragment))
}

/// A pixel to look at (from the bottom-left corner) and the colour it must have when the depth
/// test is on.
///
/// The camera is at (0, 0, 10) looking down -Z with a quarter-turn vertical field of view, so at
/// distance `d` the view is `d` units from the middle to each edge. The near cube (edge 4, centre
/// (-1.5, 0, 0), front face at distance 8) covers pixels x 18..34, y 24..40 with its 2x2 texture;
/// the far cube (edge 4, centre (1.5, 0, -3), front face at distance 11, one solid colour) covers
/// x 31..42, y 27..37. They overlap at x 31..34.
pub const PROBES: [(u32, u32, [u8; 4], &str); 8] = [
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

fn buffer(device: &mut dyn IRenderDevice, usage: BufferUsage, bytes: &[u8]) -> BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let id = must(device.create_buffer(BufferDesc { size, usage }));
    must(device.write_buffer(id, 0, bytes));
    id
}

fn texture(
    device: &mut dyn IRenderDevice,
    side: u32,
    format: TextureFormat,
    pixels: &[u8],
) -> TextureId {
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

fn pipeline(device: &mut dyn IRenderDevice, codes: (&[u8], &[u8]), depth_on: bool) -> PipelineId {
    let mut shader = |stage, code: &[u8]| {
        must(device.create_shader(ShaderDesc {
            stage,
            code,
            entry: "main",
        }))
    };
    let vertex_shader = shader(ShaderStage::Vertex, codes.0);
    let fragment_shader = shader(ShaderStage::Fragment, codes.1);
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
pub fn record_scene(
    device: &mut dyn IRenderDevice,
    target: TextureId,
    codes: (&[u8], &[u8]),
    depth_on: bool,
) -> CommandStream {
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
    let pipeline = pipeline(device, codes, depth_on);
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
    let draw = |device: &mut dyn IRenderDevice, centre: Vec3, texture: TextureId| {
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

fn texture_of_depth(device: &mut dyn IRenderDevice) -> TextureId {
    texture(device, SIDE, TextureFormat::Depth32F, &[])
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

/// Renders the scene on `device` (a frame on `surface`) and returns the read-back pixels, bottom
/// row first.
pub fn render(
    device: &mut dyn IRenderDevice,
    surface: SurfaceId,
    codes: (&[u8], &[u8]),
    depth_on: bool,
) -> Vec<u8> {
    let target = texture(device, SIDE, TextureFormat::Rgba8, &[]);
    let stream = record_scene(device, target, codes, depth_on);
    let frame = must(device.begin_frame(surface));
    let stats = must(device.submit(frame, &stream));
    must(device.end_frame(frame));
    assert_eq!(stats.draws, 2);
    let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
    must(device.read_back(target, &mut pixels));
    pixels
}

/// The depth-on picture matches every probe.
pub fn assert_probes(pixels: &[u8]) {
    let wrong: Vec<_> = PROBES
        .iter()
        .filter(|&&(x, y, want, _)| at(pixels, x, y) != want)
        .map(|&(x, y, want, what)| (x, y, at(pixels, x, y), want, what))
        .collect();
    assert!(
        wrong.is_empty(),
        "wrong pixels (x, y, got, want, what): {wrong:?}"
    );
}

/// The control picture (depth test off): the far cube, drawn second, wins the overlap. It makes
/// the overlap probe of [`assert_probes`] mean something.
pub fn assert_control(pixels: &[u8]) {
    assert_eq!(at(pixels, 32, 34), MAGENTA, "control: far cube over near");
    assert_eq!(
        at(pixels, 21, 36),
        RED,
        "control: near cube outside the overlap"
    );
}
