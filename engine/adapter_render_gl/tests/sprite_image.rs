//! The image test (card 3-15, split off from 3-13): an offscreen sprite scene is rendered by the
//! OpenGL device through the sprite batcher, the orthographic camera, the render graph and the
//! golden `sprite.dshd` shader, read back, and compared with a reference image painted on the CPU.
//!
//! Rows of the read-back image are stored bottom-up (row 0 is the bottom of the picture). Flat
//! opaque areas must match exactly; tinted and blended areas may differ by 2 per channel (driver
//! rounding, `llvmpipe` on the Linux job). The test needs GLFW, a display and an OpenGL 4.5 driver;
//! without them it prints `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` makes
//! a missing device a failure. The reference painter and the comparison run everywhere.

use adapter_render_gl::GlDevice;
use adapter_window_glfw::{GlfwWindows, explain};
use assets::dshd::{Program, Stage};
use base::math::Vec2;
use platform::window::{IWindowSystem, WindowDesc};
use render::camera::{OrthographicCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PipelineDesc, Rect,
    ResourceState, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::graph::RenderGraph;
use render::ids::{BufferId, PipelineId, SurfaceId, TextureId};
use render::sprite::{
    Sprite, SpriteBatcher, SpriteBindings, SpriteVertex, UvRect, VERTEX_STRIDE, encode_vertices,
    quad_indices, vertex_attributes,
};
use render::stream::CommandStream;

const GOLDEN: &[u8] = include_bytes!("../../tools_shaderc/golden/sprite.dshd");
const SIDE: u32 = 64;
const CLEAR: [u8; 4] = [0, 0, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const ORANGE: [u8; 4] = [255, 128, 0, 255];
/// White at half alpha over the blue clear colour: 128 of red and green, blue unchanged.
const HALF_WHITE_OVER_BLUE: [u8; 4] = [128, 128, 255, 255];
/// White at half alpha over the red square.
const HALF_WHITE_OVER_RED: [u8; 4] = [255, 128, 128, 255];
const EXACT: u8 = 0;
const ROUNDING: u8 = 2;

/// A rectangle of the reference image, in pixels from the bottom-left corner.
#[derive(Clone, Copy)]
struct Patch {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
    tolerance: u8,
}

/// The expected picture, back to front. The camera has one unit per pixel and its origin in the
/// middle, so a sprite at (px, py) of size s covers pixels `32 + p - s/2 .. 32 + p + s/2`.
const REFERENCE: [Patch; 9] = [
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
fn paint_reference() -> Vec<([u8; 4], u8)> {
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
fn mismatches(actual: &[u8], expected: &[([u8; 4], u8)]) -> Vec<(usize, [u8; 4], [u8; 4])> {
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

fn must<T>(result: Result<T, RenderError>) -> T {
    assert_eq!(result.as_ref().err(), None);
    result.unwrap_or_else(|_| unreachable!())
}

fn texture(device: &mut GlDevice, side: u32, pixels: &[u8]) -> TextureId {
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

fn pipeline(device: &mut GlDevice) -> PipelineId {
    let program = Program::parse(GOLDEN).unwrap_or_else(|_| unreachable!());
    let (Some(vertex), Some(fragment)) = (
        program.module_for(Stage::Vertex),
        program.module_for(Stage::Fragment),
    ) else {
        panic!("the golden asset lacks a stage");
    };
    let mut shader = |stage, code: &str| {
        must(device.create_shader(ShaderDesc {
            stage,
            code: code.as_bytes(),
            entry: "main",
        }))
    };
    let vertex_shader = shader(ShaderStage::Vertex, vertex.code());
    let fragment_shader = shader(ShaderStage::Fragment, fragment.code());
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

fn sprite(texture: TextureId, at: (f32, f32), size: f32, tint: [u8; 4], layer: i16) -> Sprite {
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
fn scene(white: TextureId, quad: TextureId) -> Vec<Sprite> {
    vec![
        sprite(white, (-8.0, -8.0), 16.0, [0, 255, 0, 255], 1),
        sprite(white, (16.0, -16.0), 16.0, [255, 255, 255, 128], 2),
        sprite(quad, (16.0, 16.0), 16.0, [255, 255, 255, 255], 0),
        sprite(white, (-16.0, -16.0), 16.0, [255, 0, 0, 255], 0),
        sprite(white, (-16.0, 16.0), 16.0, [255, 128, 0, 255], 0),
        sprite(white, (-20.0, -20.0), 8.0, [255, 255, 255, 128], 3),
    ]
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

fn buffer(device: &mut GlDevice, usage: BufferUsage, bytes: &[u8]) -> BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let id = must(device.create_buffer(BufferDesc { size, usage }));
    must(device.write_buffer(id, 0, bytes));
    id
}

/// Sorts and stages the sprites, uploads them, and records one cleared pass into `target`.
fn record_scene(device: &mut GlDevice, target: TextureId) -> CommandStream {
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
        pipeline: pipeline(device),
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
    let stream = record_scene(device, target);
    let frame = must(device.begin_frame(rig.surface));
    let stats = must(device.submit(frame, &stream));
    must(device.end_frame(frame));
    // Sorted by layer then texture: white (layer 0), quad (layer 0), then layers 1 to 3 on white,
    // which merge into one run.
    assert_eq!(stats.draws, 3);

    let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
    must(device.read_back(target, &mut pixels));
    let wrong = mismatches(&pixels, &paint_reference());
    assert!(
        wrong.is_empty(),
        "{} wrong pixels, first: {:?}",
        wrong.len(),
        wrong.first()
    );
    assert_eq!(device.take_gl_error(), 0, "no GL error");
}
