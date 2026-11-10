//! The OpenGL device (card 3-10): the generic render contract suite on a real GL 4.5 context, and
//! picture tests (clear and draw into a texture, then read the pixels back).
//!
//! The tests need GLFW, a display and an OpenGL 4.5 driver; without them they print
//! `not run: device not set up (...)` and pass. A job that is meant to have a device sets
//! `DUCK_REQUIRE_GL=1`, and then a missing device fails the test instead.

use adapter_render_gl::GlDevice;
use adapter_window_glfw::GlfwWindows;
use platform::window::{IWindowSystem, NativeToken, WindowDesc};
use render::contract::render_device_contract;
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, IndexType, PassDesc,
    PipelineDesc, Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc,
    TextureDesc, TextureFormat, VertexAttribute, VertexFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SurfaceId, TextureId};
use render::stream::CommandStream;

/// Whether `DUCK_REQUIRE_GL` asks for a device: set to anything but empty or `0`.
fn device_required() -> bool {
    std::env::var("DUCK_REQUIRE_GL").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Reports why a device test cannot run: a failure when a device is required, else a note.
fn not_run(reason: &str) {
    assert!(
        !device_required(),
        "an OpenGL device is required here but is not set up: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
}

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// GLFW serves one set of windows per process, so the tests that open one take turns.
static ONE_WINDOW_SET: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn take_turn() -> std::sync::MutexGuard<'static, ()> {
    ONE_WINDOW_SET
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A window and a device on it, or the reason there is none.
fn open(windows: &mut GlfwWindows) -> Result<(GlDevice, SurfaceDesc), String> {
    let desc = WindowDesc {
        title: "DuckEngine 3-10: GL device",
        width: WIDTH,
        height: HEIGHT,
    };
    let id = windows
        .create_window(&desc)
        .map_err(|e| format!("window: {e}"))?;
    let token = windows
        .native_token(id)
        .map_err(|e| format!("native token: {e}"))?;
    let device = GlDevice::create(token).map_err(|e| format!("OpenGL 4.5 device: {e}"))?;
    let surface = SurfaceDesc {
        native_token: token.0,
        width: WIDTH,
        height: HEIGHT,
    };
    Ok((device, surface))
}

#[test]
fn the_null_token_names_no_window() {
    assert!(GlDevice::create(NativeToken::NULL).is_err());
}

#[test]
fn the_gl_device_passes_the_render_contract_suite() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let (mut device, surface) = match open(&mut windows) {
        Ok(opened) => opened,
        Err(reason) => return not_run(&reason),
    };
    let caps = device.caps();
    eprintln!("device: {}, caps {caps:?}", caps.device_name);
    assert!(caps.depth_zero_to_one, "OpenGL runs with glClipControl");
    render_device_contract(&mut device, surface);
    assert_eq!(device.take_gl_error(), 0, "no GL error after the suite");
}

/// GLSL 450 source, as the shader tool will emit it once card 3-11 exists: this card hands the
/// device the text itself (the blob of a GL shader module is GLSL source, entry point `main`).
const VERTEX: &str = "#version 450 core\n\
    layout(location = 0) in vec2 position;\n\
    layout(location = 1) in vec4 colour;\n\
    layout(location = 0) out vec4 v_colour;\n\
    void main() { v_colour = colour; gl_Position = vec4(position, 0.0, 1.0); }\n";

const FRAGMENT: &str = "#version 450 core\n\
    layout(location = 0) in vec4 v_colour;\n\
    layout(location = 0) out vec4 out_colour;\n\
    void main() { out_colour = v_colour; }\n";

/// Ignores the vertex colour and paints the texel bound to slot 0.
const FRAGMENT_TEXTURED: &str = "#version 450 core\n\
    layout(binding = 0) uniform sampler2D tex;\n\
    layout(location = 0) in vec4 v_colour;\n\
    layout(location = 0) out vec4 out_colour;\n\
    void main() { out_colour = texture(tex, vec2(0.5, 0.5)); }\n";

/// A full-screen triangle made from `gl_VertexID`: no vertex buffer, no attributes.
const VERTEX_ID: &str = "#version 450 core\n\
    layout(location = 0) out vec4 v_colour;\n\
    void main() {\n\
        vec2 p = vec2(float((gl_VertexID & 1) * 4 - 1), float((gl_VertexID >> 1) * 4 - 1));\n\
        v_colour = vec4(1.0, 0.0, 0.0, 1.0);\n\
        gl_Position = vec4(p, 0.0, 1.0);\n\
    }\n";

/// Paints the colour in the uniform buffer bound to slot 0.
const FRAGMENT_UNIFORM: &str = "#version 450 core\n\
    layout(std140, binding = 0) uniform Paint { vec4 colour; };\n\
    layout(location = 0) in vec4 v_colour;\n\
    layout(location = 0) out vec4 out_colour;\n\
    void main() { out_colour = colour; }\n";

/// The shader sources of a scene and whether its pipeline has the position/colour attributes.
#[derive(Clone, Copy)]
struct Sources {
    vertex: &'static str,
    fragment: &'static str,
    attributes: bool,
}

const COLOURED: Sources = Sources {
    vertex: VERTEX,
    fragment: FRAGMENT,
    attributes: true,
};
const TEXTURED: Sources = Sources {
    vertex: VERTEX,
    fragment: FRAGMENT_TEXTURED,
    attributes: true,
};
const GENERATED: Sources = Sources {
    vertex: VERTEX_ID,
    fragment: FRAGMENT,
    attributes: false,
};
const UNIFORM: Sources = Sources {
    vertex: VERTEX_ID,
    fragment: FRAGMENT_UNIFORM,
    attributes: false,
};

/// Two floats of position then four unorm bytes of colour.
const STRIDE: u32 = 12;
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn must<T>(result: Result<T, RenderError>) -> T {
    assert_eq!(result.as_ref().err(), None);
    result.unwrap_or_else(|_| unreachable!())
}

fn vertex_bytes(points: &[([f32; 2], [u8; 4])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (position, colour) in points {
        bytes.extend_from_slice(&position[0].to_le_bytes());
        bytes.extend_from_slice(&position[1].to_le_bytes());
        bytes.extend_from_slice(colour);
    }
    bytes
}

fn count(pixels: &[u8], colour: [u8; 4]) -> usize {
    pixels.chunks_exact(4).filter(|px| **px == colour).count()
}

/// Every pixel is one of `allowed`.
fn assert_only(pixels: &[u8], allowed: &[[u8; 4]]) {
    let stray = pixels
        .chunks_exact(4)
        .filter(|px| !allowed.iter().any(|colour| **px == *colour))
        .count();
    assert_eq!(stray, 0, "pixels of a colour that was not drawn");
}

fn texel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = usize::try_from((y * WIDTH + x) * 4).unwrap_or(usize::MAX);
    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
}

/// Everything a picture test needs: a device with a surface, a render target texture and a
/// pipeline made from the vertex shader above and `fragment`.
struct Scene {
    device: GlDevice,
    surface: SurfaceId,
    target: TextureId,
    pipeline: PipelineId,
}

impl Scene {
    fn new(mut device: GlDevice, surface: SurfaceDesc, sources: Sources) -> Self {
        let surface = must(device.create_surface(surface));
        let target = must(device.create_texture(TextureDesc {
            width: WIDTH,
            height: HEIGHT,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        }));
        let vertex_shader = must(device.create_shader(ShaderDesc {
            stage: ShaderStage::Vertex,
            code: sources.vertex.as_bytes(),
            entry: "main",
        }));
        let fragment_shader = must(device.create_shader(ShaderDesc {
            stage: ShaderStage::Fragment,
            code: sources.fragment.as_bytes(),
            entry: "main",
        }));
        let all = [
            VertexAttribute {
                location: 0,
                format: VertexFormat::Float2,
                offset: 0,
            },
            VertexAttribute {
                location: 1,
                format: VertexFormat::Unorm8x4,
                offset: 8,
            },
        ];
        let attributes = if sources.attributes { &all[..] } else { &[] };
        let pipeline = must(device.create_pipeline(PipelineDesc {
            vertex_shader,
            fragment_shader,
            vertex_stride: if sources.attributes { STRIDE } else { 0 },
            attributes,
            blend: BlendMode::Opaque,
            depth: DepthState {
                test: false,
                write: false,
            },
            color_format: TextureFormat::Rgba8,
            depth_format: None,
        }));
        Self {
            device,
            surface,
            target,
            pipeline,
        }
    }

    fn buffer(&mut self, usage: BufferUsage, bytes: &[u8]) -> BufferId {
        let size = u32::try_from(bytes.len()).unwrap_or(0);
        let buffer = must(self.device.create_buffer(BufferDesc { size, usage }));
        must(self.device.write_buffer(buffer, 0, bytes));
        buffer
    }

    /// Runs a stream in one frame; `fill` records between the pass begin and end, after the
    /// pipeline and the full-target viewport are set.
    fn draw(&mut self, fill: impl FnOnce(&mut CommandStream)) -> u32 {
        let mut stream = CommandStream::new();
        stream.begin_pass(PassDesc {
            target: RenderTarget::Texture(self.target),
            clear_color: Some([0.0, 0.0, 1.0, 1.0]),
            clear_depth: None,
            depth: None,
        });
        stream.set_pipeline(self.pipeline);
        stream.set_viewport(Rect {
            x: 0,
            y: 0,
            width: WIDTH,
            height: HEIGHT,
        });
        fill(&mut stream);
        stream.end_pass();
        let frame = must(self.device.begin_frame(self.surface));
        let stats = must(self.device.submit(frame, &stream));
        must(self.device.end_frame(frame));
        stats.draws
    }

    fn pixels(&mut self) -> Vec<u8> {
        let mut pixels = vec![0u8; usize::try_from(WIDTH * HEIGHT * 4).unwrap_or(0)];
        must(self.device.read_back(self.target, &mut pixels));
        pixels
    }
}

/// Opens a window and a scene on it, or notes why not and gives `None`.
fn scene(windows: &mut GlfwWindows, sources: Sources) -> Option<Scene> {
    match open(windows) {
        Ok((device, surface)) => Some(Scene::new(device, surface, sources)),
        Err(reason) => {
            not_run(&reason);
            None
        }
    }
}

#[test]
fn a_triangle_drawn_into_a_texture_has_the_right_pixels() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, COLOURED) else {
        return;
    };
    // A red triangle that covers the lower left half of the clip square.
    let points = [([-1.0, -1.0], RED), ([1.0, -1.0], RED), ([-1.0, 1.0], RED)];
    let buffer = scene.buffer(BufferUsage::Vertex, &vertex_bytes(&points));
    let draws = scene.draw(|stream| {
        stream.bind_vertex_buffer(0, buffer, 0);
        stream.draw(3, 1);
    });
    assert_eq!(draws, 1);

    let pixels = scene.pixels();
    // Every pixel is the triangle or the clear colour, and the triangle is about half of it.
    assert_only(&pixels, &[RED, BLUE]);
    let red = count(&pixels, RED);
    assert!((1900..=2300).contains(&red), "{red} red pixels");
    // Rows are in OpenGL order: row 0 is the bottom of the picture.
    assert_eq!(texel(&pixels, 4, 4), RED, "near the lower left corner");
    assert_eq!(texel(&pixels, 12, 40), RED, "inside, left of the diagonal");
    assert_eq!(texel(&pixels, 60, 60), BLUE, "the clear colour beyond");
    assert_eq!(scene.device.take_gl_error(), 0, "no GL error");
}

#[test]
fn an_indexed_draw_honours_the_vertex_and_index_offsets() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, COLOURED) else {
        return;
    };
    // Three vertices to skip (vertex offset), then the red triangle.
    let green = [0, 255, 0, 255];
    let points = [
        ([-1.0, -1.0], green),
        ([1.0, -1.0], green),
        ([1.0, 1.0], green),
        ([-1.0, -1.0], RED),
        ([1.0, -1.0], RED),
        ([-1.0, 1.0], RED),
    ];
    let vertices = scene.buffer(BufferUsage::Vertex, &vertex_bytes(&points));
    // Two indices to skip (index offset 4 bytes), then 0, 1, 2 relative to the vertex offset.
    let indices: Vec<u8> = [7u16, 7, 0, 1, 2]
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    let index_buffer = scene.buffer(BufferUsage::Index, &indices);
    let draws = scene.draw(|stream| {
        stream.bind_vertex_buffer(0, vertices, 3 * STRIDE);
        stream.bind_index_buffer(index_buffer, 4, IndexType::U16);
        stream.draw_indexed(3, 1);
    });
    assert_eq!(draws, 1);

    let pixels = scene.pixels();
    assert_eq!(
        texel(&pixels, 12, 40),
        RED,
        "the triangle after the offsets"
    );
    assert_eq!(
        texel(&pixels, 60, 60),
        BLUE,
        "the skipped green one is not drawn"
    );
    assert_eq!(scene.device.take_gl_error(), 0, "no GL error");
}

#[test]
fn a_bound_texture_and_sampler_reach_the_fragment_shader() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, TEXTURED) else {
        return;
    };
    let source = must(scene.device.create_texture(TextureDesc {
        width: 1,
        height: 1,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let green = [0, 255, 0, 255];
    must(scene.device.write_texture(
        source,
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        &green,
    ));
    let sampler = must(scene.device.create_sampler(SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }));
    // The whole clip square as one triangle that overshoots it.
    let points = [([-1.0, -1.0], RED), ([3.0, -1.0], RED), ([-1.0, 3.0], RED)];
    let buffer = scene.buffer(BufferUsage::Vertex, &vertex_bytes(&points));
    scene.draw(|stream| {
        stream.bind_texture(0, source, sampler);
        stream.bind_vertex_buffer(0, buffer, 0);
        stream.draw(3, 1);
    });

    let pixels = scene.pixels();
    assert_only(&pixels, &[green]);
    assert_eq!(
        texel(&pixels, 4, 4),
        green,
        "the texel, not the vertex colour"
    );
    assert_eq!(texel(&pixels, 60, 60), green);
    assert_eq!(scene.device.take_gl_error(), 0, "no GL error");
}

#[test]
fn a_shader_that_does_not_compile_is_rejected_and_leaves_no_handle() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let (mut device, _surface) = match open(&mut windows) {
        Ok(opened) => opened,
        Err(reason) => return not_run(&reason),
    };
    let broken = device.create_shader(ShaderDesc {
        stage: ShaderStage::Fragment,
        code: b"#version 450 core\nthis is not glsl",
        entry: "main",
    });
    assert_eq!(broken, Err(RenderError::ShaderRejected));
    let wrong_entry = device.create_shader(ShaderDesc {
        stage: ShaderStage::Fragment,
        code: FRAGMENT.as_bytes(),
        entry: "frag_main",
    });
    assert_eq!(wrong_entry, Err(RenderError::ShaderRejected));
    assert_eq!(device.take_gl_error(), 0, "no GL error");
}

#[test]
fn a_draw_without_a_vertex_buffer_runs_and_both_devices_accept_it() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, GENERATED) else {
        return;
    };
    let draws = scene.draw(|stream| stream.draw(3, 1));
    assert_eq!(draws, 1);
    let pixels = scene.pixels();
    assert_only(&pixels, &[RED]);
    assert_eq!(scene.device.take_gl_error(), 0, "no GL error");

    // Parity: the Null device (the validator inside the GL device) takes the same stream.
    let mut null = render::null::NullDevice::new();
    let surface = must(null.create_surface(SurfaceDesc {
        native_token: 1,
        width: WIDTH,
        height: HEIGHT,
    }));
    let target = must(null.create_texture(TextureDesc {
        width: WIDTH,
        height: HEIGHT,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let vertex_shader = must(null.create_shader(ShaderDesc {
        stage: ShaderStage::Vertex,
        code: VERTEX_ID.as_bytes(),
        entry: "main",
    }));
    let fragment_shader = must(null.create_shader(ShaderDesc {
        stage: ShaderStage::Fragment,
        code: FRAGMENT.as_bytes(),
        entry: "main",
    }));
    let pipeline = must(null.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: 0,
        attributes: &[],
        blend: BlendMode::Opaque,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: None,
    }));
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(target),
        clear_color: None,
        clear_depth: None,
        depth: None,
    });
    stream.set_pipeline(pipeline);
    stream.draw(3, 1);
    stream.end_pass();
    let frame = must(null.begin_frame(surface));
    assert_eq!(must(null.submit(frame, &stream)).draws, 1);
}

#[test]
fn a_32_bit_index_draw_works() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, COLOURED) else {
        return;
    };
    let points = [([-1.0, -1.0], RED), ([1.0, -1.0], RED), ([-1.0, 1.0], RED)];
    let vertices = scene.buffer(BufferUsage::Vertex, &vertex_bytes(&points));
    let indices: Vec<u8> = [2u32, 1, 0]
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    let index_buffer = scene.buffer(BufferUsage::Index, &indices);
    scene.draw(|stream| {
        stream.bind_vertex_buffer(0, vertices, 0);
        stream.bind_index_buffer(index_buffer, 0, IndexType::U32);
        stream.draw_indexed(3, 1);
    });
    let pixels = scene.pixels();
    assert_only(&pixels, &[RED, BLUE]);
    assert!((1900..=2300).contains(&count(&pixels, RED)));
    assert_eq!(texel(&pixels, 12, 40), RED);
    assert_eq!(texel(&pixels, 60, 60), BLUE);
    assert_eq!(scene.device.take_gl_error(), 0, "no GL error");
}

#[test]
fn a_shader_reads_a_bound_uniform_buffer() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, UNIFORM) else {
        return;
    };
    // A vec4 (std140) of four floats: green.
    let paint: Vec<u8> = [0.0f32, 1.0, 0.0, 1.0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let buffer = scene.buffer(BufferUsage::Uniform, &paint);
    scene.draw(|stream| {
        stream.bind_uniforms(0, buffer);
        stream.draw(3, 1);
    });
    let pixels = scene.pixels();
    assert_only(&pixels, &[[0, 255, 0, 255]]);
    assert_eq!(scene.device.take_gl_error(), 0, "no GL error");
}

#[test]
fn textures_with_one_and_two_mip_levels_sample_with_a_mipmapped_filter() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let Some(mut scene) = scene(&mut windows, TEXTURED) else {
        return;
    };
    let sampler = must(scene.device.create_sampler(SamplerDesc {
        min_filter: Filter::Linear,
        mag_filter: Filter::Linear,
        address: AddressMode::Repeat,
    }));
    let points = [([-1.0, -1.0], RED), ([3.0, -1.0], RED), ([-1.0, 3.0], RED)];
    let buffer = scene.buffer(BufferUsage::Vertex, &vertex_bytes(&points));
    // The port writes level 0 only; a 2x2 texture with two levels must still sample level 0.
    for mips in [1, 2] {
        let texture = must(scene.device.create_texture(TextureDesc {
            width: 2,
            height: 2,
            mip_count: mips,
            format: TextureFormat::Rgba8,
        }));
        let full = Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        must(scene.device.write_texture(texture, full, &[255u8; 16]));
        scene.draw(|stream| {
            stream.bind_texture(0, texture, sampler);
            stream.bind_vertex_buffer(0, buffer, 0);
            stream.draw(3, 1);
        });
        assert_only(&scene.pixels(), &[[255; 4]]);
        assert_eq!(
            scene.device.take_gl_error(),
            0,
            "no GL error with {mips} levels"
        );
    }
}

#[test]
fn a_refused_shader_leaves_the_drivers_message_and_no_stale_flag() {
    let _turn = take_turn();
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let (mut device, _surface) = match open(&mut windows) {
        Ok(opened) => opened,
        Err(reason) => return not_run(&reason),
    };
    assert_eq!(device.last_shader_log(), "");
    let broken = device.create_shader(ShaderDesc {
        stage: ShaderStage::Fragment,
        code: b"#version 450 core\nthis is not glsl",
        entry: "main",
    });
    assert_eq!(broken, Err(RenderError::ShaderRejected));
    assert!(
        !device.last_shader_log().is_empty(),
        "the compiler said why"
    );
    assert!(device.last_shader_log().len() <= 1024);
    let not_text = device.create_shader(ShaderDesc {
        stage: ShaderStage::Fragment,
        code: &[0xFF, 0xFE, 0x00, 0x01],
        entry: "main",
    });
    assert_eq!(not_text, Err(RenderError::ShaderRejected));
    assert!(device.last_shader_log().contains("UTF-8"));
    assert_eq!(device.take_gl_error(), 0, "no stale GL error");
}
