//! The sprite image scene shared by the image tests of the OpenGL and the Vulkan adapter (cards
//! 3-15 and 6-10): one scene of six sprites, the reference picture painted on the CPU, and the
//! comparison. A test includes it with `#[path]` and brings its own device, window and shaders.
//!
//! Rows of a read-back image are stored bottom-up (row 0 is the bottom of the picture). Flat
//! opaque areas must match exactly; tinted and blended areas may differ by 2 per channel (driver
//! rounding, `llvmpipe` on the Linux job).

use assets::dshd::{Module, Program, Stage};
use base::math::Vec2;
use render::camera::{OrthographicCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PipelineDesc, Rect,
    ResourceState, SamplerDesc, ShaderDesc, ShaderStage, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::graph::RenderGraph;
use render::ids::{BufferId, PipelineId, TextureId};
use render::sprite::{
    Sprite, SpriteBatcher, SpriteBindings, SpriteVertex, UvRect, VERTEX_STRIDE, encode_vertices,
    quad_indices, vertex_attributes,
};
use render::stream::CommandStream;

pub const SIDE: u32 = 64;
pub const CLEAR: [u8; 4] = [0, 0, 255, 255];
pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const GREEN: [u8; 4] = [0, 255, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];
pub const YELLOW: [u8; 4] = [255, 255, 0, 255];
pub const ORANGE: [u8; 4] = [255, 128, 0, 255];
/// White at half alpha over the blue clear colour: 128 of red and green, blue unchanged.
pub const HALF_WHITE_OVER_BLUE: [u8; 4] = [128, 128, 255, 255];
/// White at half alpha over the red square.
pub const HALF_WHITE_OVER_RED: [u8; 4] = [255, 128, 128, 255];
pub const EXACT: u8 = 0;
pub const ROUNDING: u8 = 2;

/// A rectangle of the reference image, in pixels from the bottom-left corner.
#[derive(Clone, Copy)]
pub struct Patch {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub color: [u8; 4],
    pub tolerance: u8,
}

/// The expected picture, back to front. The camera has one unit per pixel and its origin in the
/// middle, so a sprite at (px, py) of size s covers pixels `32 + p - s/2 .. 32 + p + s/2`.
pub const REFERENCE: [Patch; 9] = [
    // Red square (layer 0), partly covered by the green one (layer 1).
    Patch {
        x: 8,
        y: 8,
        width: 16,
        height: 16,
        color: RED,
        tolerance: EXACT,
    },
    Patch {
        x: 16,
        y: 16,
        width: 16,
        height: 16,
        color: GREEN,
        tolerance: EXACT,
    },
    // Orange (a tint on white), top left.
    Patch {
        x: 8,
        y: 40,
        width: 16,
        height: 16,
        color: ORANGE,
        tolerance: ROUNDING,
    },
    // The 2x2 texture, top right: data row 0 (red, green) is the top of the sprite.
    Patch {
        x: 40,
        y: 48,
        width: 8,
        height: 8,
        color: RED,
        tolerance: EXACT,
    },
    Patch {
        x: 48,
        y: 48,
        width: 8,
        height: 8,
        color: GREEN,
        tolerance: EXACT,
    },
    Patch {
        x: 40,
        y: 40,
        width: 8,
        height: 8,
        color: BLUE,
        tolerance: EXACT,
    },
    Patch {
        x: 48,
        y: 40,
        width: 8,
        height: 8,
        color: YELLOW,
        tolerance: EXACT,
    },
    // Half-transparent white over the clear colour, bottom right (layer 2).
    Patch {
        x: 40,
        y: 8,
        width: 16,
        height: 16,
        color: HALF_WHITE_OVER_BLUE,
        tolerance: ROUNDING,
    },
    // Half-transparent white (layer 3) over the red square: red stays, green and blue go to 128.
    Patch {
        x: 8,
        y: 8,
        width: 8,
        height: 8,
        color: HALF_WHITE_OVER_RED,
        tolerance: ROUNDING,
    },
];

/// The reference image with its per-pixel tolerance, rows from the bottom.
pub fn paint_reference() -> Vec<([u8; 4], u8)> {
    let mut image = vec![(CLEAR, EXACT); (SIDE * SIDE) as usize];
    for patch in REFERENCE {
        for y in patch.y..patch.y + patch.height {
            let first = (y * SIDE + patch.x) as usize;
            image[first..first + patch.width as usize].fill((patch.color, patch.tolerance));
        }
    }
    image
}

/// The pixels (index, actual, expected) that differ by more than their tolerance.
pub fn mismatches(actual: &[u8], expected: &[([u8; 4], u8)]) -> Vec<(usize, [u8; 4], [u8; 4])> {
    actual
        .chunks_exact(4)
        .zip(expected)
        .enumerate()
        .filter_map(|(index, (px, &(want, tolerance)))| {
            let got = [px[0], px[1], px[2], px[3]];
            let off = got.iter().zip(want).any(|(g, w)| g.abs_diff(w) > tolerance);
            off.then_some((index, got, want))
        })
        .collect()
}

pub fn must<T>(result: Result<T, RenderError>) -> T {
    assert_eq!(result.as_ref().err(), None);
    result.unwrap_or_else(|_| unreachable!())
}

pub fn texture(device: &mut dyn IRenderDevice, side: u32, pixels: &[u8]) -> TextureId {
    let id = must(device.create_texture(TextureDesc {
        width: side,
        height: side,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let all = Rect {
        x: 0,
        y: 0,
        width: side,
        height: side,
    };
    must(device.write_texture(id, all, pixels));
    id
}

/// The code of one module: SPIR-V when the asset holds it, else GLSL text.
fn stage_code(module: Module<'_>) -> &[u8] {
    module.spirv().unwrap_or_else(|| module.code().as_bytes())
}

/// The code of the vertex and the fragment module of a `sprite.dshd` asset: GLSL text or SPIR-V,
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

pub fn pipeline(device: &mut dyn IRenderDevice, codes: (&[u8], &[u8])) -> PipelineId {
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
        vertex_stride: VERTEX_STRIDE,
        attributes: &vertex_attributes(),
        blend: BlendMode::Alpha,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: None,
    }))
}

pub fn sprite(texture: TextureId, at: (f32, f32), size: f32, tint: [u8; 4], layer: i16) -> Sprite {
    Sprite {
        texture,
        position: Vec2::new(at.0, at.1),
        size: Vec2::new(size, size),
        angle: 0.0,
        uv: UvRect::FULL,
        tint,
        layer,
    }
}

/// The scene, in an order that is not the drawing order (the batcher sorts by layer, then texture).
pub fn scene(white: TextureId, quad: TextureId) -> Vec<Sprite> {
    vec![
        sprite(white, (-8.0, -8.0), 16.0, [0, 255, 0, 255], 1),
        sprite(white, (16.0, -16.0), 16.0, [255, 255, 255, 128], 2),
        sprite(quad, (16.0, 16.0), 16.0, [255, 255, 255, 255], 0),
        sprite(white, (-16.0, -16.0), 16.0, [255, 0, 0, 255], 0),
        sprite(white, (-16.0, 16.0), 16.0, [255, 128, 0, 255], 0),
        sprite(white, (-20.0, -20.0), 8.0, [255, 255, 255, 128], 3),
    ]
}

pub fn buffer(device: &mut dyn IRenderDevice, usage: BufferUsage, bytes: &[u8]) -> BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let id = must(device.create_buffer(BufferDesc { size, usage }));
    must(device.write_buffer(id, 0, bytes));
    id
}

/// Sorts and stages the sprites, uploads them, and records one cleared pass into `target`.
pub fn record_scene(
    device: &mut dyn IRenderDevice,
    target: TextureId,
    codes: (&[u8], &[u8]),
) -> CommandStream {
    let white = texture(device, 1, &[255, 255, 255, 255]);
    let quad = texture(device, 2, &[RED, GREEN, BLUE, YELLOW].concat());
    let sampler = must(device.create_sampler(SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }));
    let sprites = scene(white, quad);
    let mut batcher = SpriteBatcher::new(sprites.len()).unwrap_or_else(|_| unreachable!());
    let mut staging = vec![SpriteVertex::default(); sprites.len() * 4];
    assert!(batcher.prepare(&sprites, &mut staging).is_ok());
    let mut vertex_bytes = vec![0u8; sprites.len() * 4 * VERTEX_STRIDE as usize];
    assert!(encode_vertices(&staging, &mut vertex_bytes));
    let index_bytes: Vec<u8> = quad_indices(sprites.len())
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    let camera = OrthographicCamera::pixel_perfect(SIDE, SIDE, Vec2::new(0.0, 0.0), 1.0)
        .and_then(|camera| camera.view_projection())
        .unwrap_or_else(|| unreachable!());
    let bindings = SpriteBindings {
        pipeline: pipeline(device, codes),
        uniforms: buffer(device, BufferUsage::Uniform, &matrix_bytes(&camera)),
        vertices: buffer(device, BufferUsage::Vertex, &vertex_bytes),
        indices: buffer(device, BufferUsage::Index, &index_bytes),
        sampler,
    };
    let mut graph = RenderGraph::new();
    let output = graph
        .import_texture(target, ResourceState::Undefined, ResourceState::ShaderRead)
        .unwrap_or_else(|_| unreachable!());
    assert!(
        graph
            .add_pass("sprites", output, Some([0.0, 0.0, 1.0, 1.0]), &[])
            .is_ok()
    );
    let mut stream = CommandStream::new();
    let view = Rect {
        x: 0,
        y: 0,
        width: SIDE,
        height: SIDE,
    };
    let recorded = graph.compile().record(&[], &mut stream, |_, stream| {
        stream.set_viewport(view);
        batcher.record(stream, &bindings);
    });
    assert!(recorded.is_ok());
    stream
}

/// Draws that the scene takes: sorted by layer then texture, white (layer 0), quad (layer 0), then
/// layers 1 to 3 on white, which merge into one run.
pub const DRAWS: u32 = 3;

/// Asserts that `pixels` (the read-back target) show the reference picture.
pub fn assert_reference(pixels: &[u8]) {
    let wrong = mismatches(pixels, &paint_reference());
    assert!(
        wrong.is_empty(),
        "{} wrong pixels, first: {:?}",
        wrong.len(),
        wrong.first()
    );
}
