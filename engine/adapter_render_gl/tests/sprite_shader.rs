//! The first shader asset on a real OpenGL device: the golden `sprite.dshd` (made by this tool
//! from `sprite.vert` and `sprite.frag`) is read back with `assets::dshd`, its two modules are
//! handed to the GL device as they are, and a sprite triangle is drawn into a texture.
//!
//! The test needs GLFW, a display and an OpenGL 4.5 driver; without them it prints
//! `not run: device not set up (...)` and passes. `DUCK_REQUIRE_GL=1` makes a missing device a
//! failure. The tools (glslang, SPIRV-Cross) are not needed: the golden file is the input, read as
//! data from `engine/tools_shaderc/golden/`. This crate depends on `assets` (layer 2) in its tests
//! only; no crate depends on the tool, and the tool depends on no adapter.

use adapter_render_gl::GlDevice;
use adapter_window_glfw::GlfwWindows;
use assets::dshd::{Module, Program, Stage};
use platform::window::{IWindowSystem, WindowDesc};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc,
    Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc,
    TextureFormat, VertexAttribute, VertexFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SamplerId, ShaderId, TextureId};
use render::stream::CommandStream;

const GOLDEN: &[u8] = include_bytes!("../../tools_shaderc/golden/sprite.dshd");
const SIZE: u32 = 64;
/// Position (2 floats), uv (2 floats), colour (4 floats).
const STRIDE: u32 = 32;
const GREEN: [u8; 4] = [0, 255, 0, 255];

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

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn open(windows: &mut GlfwWindows) -> Result<(GlDevice, SurfaceDesc), String> {
    let desc = WindowDesc {
        title: "DuckEngine 3-11: sprite shader",
        width: SIZE,
        height: SIZE,
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
        width: SIZE,
        height: SIZE,
    };
    Ok((device, surface))
}

/// The vertex layout, from the reflection stored in the asset (not from the shader text): the
/// engine's sprite vertex is position, uv, colour, all floats, tightly packed.
fn attributes_of(vertex: &Module<'_>) -> Vec<VertexAttribute> {
    vertex
        .inputs()
        .map(|input| VertexAttribute {
            location: u32::from(input.location),
            format: match input.components {
                2 => VertexFormat::Float2,
                3 => VertexFormat::Float3,
                _ => VertexFormat::Float4,
            },
            offset: match input.location {
                0 => 0,
                1 => 8,
                _ => 16,
            },
        })
        .collect()
}

fn shader(device: &mut GlDevice, stage: ShaderStage, module: &Module<'_>) -> ShaderId {
    must(device.create_shader(ShaderDesc {
        stage,
        code: module.code().as_bytes(),
        entry: "main",
    }))
}

fn sprite_pipeline(
    device: &mut GlDevice,
    vertex: &Module<'_>,
    fragment: &Module<'_>,
) -> PipelineId {
    let vertex_shader = shader(device, ShaderStage::Vertex, vertex);
    let fragment_shader = shader(device, ShaderStage::Fragment, fragment);
    let attributes = attributes_of(vertex);
    assert_eq!(attributes.len(), 3);
    must(device.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: STRIDE,
        attributes: &attributes,
        blend: BlendMode::Opaque,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: None,
    }))
}

fn buffer(device: &mut GlDevice, usage: BufferUsage, bytes: &[u8]) -> BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let buffer = must(device.create_buffer(BufferDesc { size, usage }));
    must(device.write_buffer(buffer, 0, bytes));
    buffer
}

/// The identity matrix as a std140 `mat4` (column-major).
#[rustfmt::skip]
fn identity_matrix() -> Vec<u8> {
    floats(&[
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ])
}

/// One triangle that covers the clip square: white vertex colour, uv in the middle.
fn covering_triangle() -> Vec<u8> {
    let corner = |x: f32, y: f32| [x, y, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
    floats(&[corner(-1.0, -1.0), corner(3.0, -1.0), corner(-1.0, 3.0)].concat())
}

/// A 1x1 green texture and a nearest sampler.
fn green_texture(device: &mut GlDevice) -> (TextureId, SamplerId) {
    let texture = must(device.create_texture(TextureDesc {
        width: 1,
        height: 1,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let all = Rect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    };
    must(device.write_texture(texture, all, &GREEN));
    let sampler = must(device.create_sampler(SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }));
    (texture, sampler)
}

#[test]
fn the_sprite_shaders_from_the_golden_asset_draw_a_textured_triangle() {
    let program = Program::parse(GOLDEN);
    assert!(program.is_ok(), "{program:?}");
    let program = program.unwrap_or_else(|_| unreachable!());
    let (Some(vertex), Some(fragment)) = (
        program.module_for(Stage::Vertex),
        program.module_for(Stage::Fragment),
    ) else {
        panic!("the golden asset lacks a stage");
    };

    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => return not_run(&format!("GLFW: {error}")),
    };
    let (mut device, surface) = match open(&mut windows) {
        Ok(opened) => opened,
        Err(reason) => return not_run(&reason),
    };
    let surface = must(device.create_surface(surface));
    let target = must(device.create_texture(TextureDesc {
        width: SIZE,
        height: SIZE,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let pipeline = sprite_pipeline(&mut device, &vertex, &fragment);

    // Camera: the identity matrix (std140 mat4, column-major), in the binding the asset names.
    let camera = vertex.uniform().map_or(0, |block| block.binding);
    let camera_buffer = buffer(&mut device, BufferUsage::Uniform, &identity_matrix());
    let vertices = buffer(&mut device, BufferUsage::Vertex, &covering_triangle());
    let (texture, sampler) = green_texture(&mut device);

    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(target),
        clear_color: Some([0.0, 0.0, 1.0, 1.0]),
        clear_depth: None,
        depth: None,
    });
    stream.set_pipeline(pipeline);
    stream.set_viewport(Rect {
        x: 0,
        y: 0,
        width: SIZE,
        height: SIZE,
    });
    stream.bind_uniforms(u32::from(camera), camera_buffer);
    let unit = u32::from(fragment.texture_binding().unwrap_or(0));
    stream.bind_texture(unit, texture, sampler);
    stream.bind_vertex_buffer(0, vertices, 0);
    stream.draw(3, 1);
    stream.end_pass();
    let frame = must(device.begin_frame(surface));
    must(device.submit(frame, &stream));
    must(device.end_frame(frame));

    let mut pixels = vec![0u8; usize::try_from(SIZE * SIZE * 4).unwrap_or(0)];
    must(device.read_back(target, &mut pixels));
    let wrong = pixels.chunks_exact(4).filter(|px| **px != GREEN).count();
    assert_eq!(wrong, 0, "every pixel is the sampled texel times white");
    assert_eq!(device.take_gl_error(), 0, "no GL error");
}
