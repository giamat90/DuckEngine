//! The Vulkan render device on a real window and a real GPU (card 6-9): the generic render
//! contract suite, and frames on the backbuffer.
//!
//! The tests need a window system (GLFW), a Vulkan 1.3 loader and a device that can present to
//! the window; without them they print `not run: device not set up (...)` and pass. A job meant to
//! have all of them sets `DUCK_REQUIRE_VK=1`, and then anything missing fails the test instead.
//! The validation layer is switched on when it is installed, and a message from it fails the test.

use adapter_render_vk::{VulkanDevice, VulkanError, VulkanOptions};
use adapter_window_glfw::GlfwWindows;
use assets::dshd::{Program, Stage};
use platform::window::{IWindowSystem, WindowDesc};
use render::contract::render_device_contract;
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, IndexType, PassDesc,
    PipelineDesc, Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc,
    TextureDesc, TextureFormat, VertexAttribute, VertexFormat,
};
use render::device::IRenderDevice;
use render::ids::{BufferId, PipelineId, SamplerId, TextureId};
use render::stream::CommandStream;
use std::sync::{Mutex, PoisonError};

/// The sprite image scene and its reference picture, shared with the OpenGL adapter's image test.
#[path = "../../adapter_render_gl/tests/support/sprite_image.rs"]
mod scene;

/// The mesh image scene (two cubes, a depth texture), shared with the OpenGL adapter's test.
#[path = "../../adapter_render_gl/tests/support/mesh_scene.rs"]
mod mesh;

/// GLFW is one library with global state: one test at a time uses it.
static GLFW_IN_USE: Mutex<()> = Mutex::new(());

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// Whether `DUCK_REQUIRE_VK` asks for a device: set to anything but empty or `0`.
fn device_required() -> bool {
    std::env::var("DUCK_REQUIRE_VK").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Reports why a test cannot run: a failure when a device is required, else a note.
fn not_run(reason: &str) {
    assert!(
        !device_required(),
        "a window and a Vulkan device are required here but are not set up: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
}

/// A device on a window, with the window system kept alive (and dropped last).
struct Rig {
    device: VulkanDevice,
    surface: SurfaceDesc,
    _windows: GlfwWindows,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Rig {
    /// The device and its surface description, or the reason there is no window or device here.
    fn start() -> Result<Self, String> {
        let turn = GLFW_IN_USE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut windows = GlfwWindows::load_default(1).map_err(|e| format!("GLFW: {e}"))?;
        let window = windows
            .create_window(&WindowDesc {
                title: "duckengine-vulkan-device-test",
                width: WIDTH,
                height: HEIGHT,
            })
            .map_err(|e| format!("window: {e}"))?;
        let token = windows
            .native_token(window)
            .map_err(|e| format!("native token: {e}"))?;
        let info = windows
            .window_info(window)
            .map_err(|e| format!("window info: {e}"))?;
        let device = match VulkanDevice::create(token, VulkanOptions { validation: true }) {
            Ok(device) => device,
            Err(error @ (VulkanError::NoLoader | VulkanError::NoSuitableDevice)) => {
                return Err(error.to_string());
            }
            Err(error) => panic!("making the device failed: {error}"),
        };
        Ok(Self {
            device,
            surface: SurfaceDesc {
                native_token: token.0,
                width: info.framebuffer_width,
                height: info.framebuffer_height,
            },
            _windows: windows,
            _turn: turn,
        })
    }

    /// The validation layer reported nothing; says so when the layer was not there to report.
    fn assert_quiet(&self) {
        if !self.device.validation_active() {
            eprintln!("note: the validation layer is not installed, so this run was not validated");
        }
        let report = self.device.validation_report();
        assert!(report.is_clean(), "validation said: {:#?}", report.messages);
    }
}

#[test]
fn the_vulkan_device_passes_the_render_contract_suite() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let caps = rig.device.caps();
    eprintln!(
        "device: {} on {}, caps {caps:?}",
        caps.device_name,
        rig.device.gpu_name()
    );
    assert!(caps.depth_zero_to_one, "Vulkan clips depth to 0..1");
    render_device_contract(&mut rig.device, rig.surface);
    rig.assert_quiet();
}

#[test]
fn many_frames_on_the_backbuffer_are_presented_without_a_validation_message() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let Ok(surface) = rig.device.create_surface(rig.surface) else {
        panic!("the surface should be made");
    };
    for round in 0..8_u8 {
        let shade = f32::from(round) / 8.0;
        let mut stream = CommandStream::new();
        stream.begin_pass(PassDesc {
            target: RenderTarget::Backbuffer,
            clear_color: Some([shade, 0.25, 0.5, 1.0]),
            clear_depth: None,
            depth: None,
        });
        stream.end_pass();
        let Ok(frame) = rig.device.begin_frame(surface) else {
            panic!("frame {round} should begin");
        };
        assert!(rig.device.submit(frame, &stream).is_ok(), "frame {round}");
        assert_eq!(rig.device.end_frame(frame), Ok(()), "frame {round}");
    }
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}

// ----- shaders, pipelines and draws (card 6-14) -----

/// The sprite shaders the engine ships, compiled to SPIR-V by `shaderc` (the golden file of the
/// shader compiler tests).
const MESH_SPIRV: &[u8] = include_bytes!("../../app_mesh_sample/assets/mesh.spv.dshd");
const SPRITE_SPIRV: &[u8] = include_bytes!("../../tools_shaderc/golden/sprite.spv.dshd");

const SIZE: u32 = 16;
const FLOATS_PER_VERTEX: usize = 8;
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// What the sprite pipeline draws with.
struct Sprite {
    pipeline: PipelineId,
    uniforms: BufferId,
    white: TextureId,
    sampler: SamplerId,
}

fn must<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what} failed: {error:?}"),
    }
}

/// The vertex and the fragment shader of the sprite program.
fn sprite_shaders(device: &mut VulkanDevice) -> Vec<render::ids::ShaderId> {
    let program = must(Program::parse(SPRITE_SPIRV), "reading the sprite shaders");
    let mut ids = Vec::new();
    for (stage, port_stage) in [
        (Stage::Vertex, ShaderStage::Vertex),
        (Stage::Fragment, ShaderStage::Fragment),
    ] {
        let Some(module) = program.module_for(stage) else {
            panic!("the sprite program has no {stage:?} module");
        };
        let Some(code) = module.spirv() else {
            panic!("the sprite module is not SPIR-V");
        };
        let desc = ShaderDesc {
            stage: port_stage,
            code,
            entry: "main",
        };
        ids.push(must(device.create_shader(desc), "making a shader"));
    }
    ids
}

fn sprite_pipeline(device: &mut VulkanDevice) -> PipelineId {
    let ids = sprite_shaders(device);
    let attributes = [
        VertexAttribute {
            location: 0,
            format: VertexFormat::Float2,
            offset: 0,
        },
        VertexAttribute {
            location: 1,
            format: VertexFormat::Float2,
            offset: 8,
        },
        VertexAttribute {
            location: 2,
            format: VertexFormat::Float4,
            offset: 16,
        },
    ];
    must(
        device.create_pipeline(PipelineDesc {
            vertex_shader: ids[0],
            fragment_shader: ids[1],
            vertex_stride: 32,
            attributes: &attributes,
            blend: BlendMode::Opaque,
            depth: DepthState {
                test: false,
                write: false,
            },
            color_format: TextureFormat::Rgba8,
            depth_format: None,
        }),
        "making the pipeline",
    )
}

fn sprite(device: &mut VulkanDevice) -> Sprite {
    let pipeline = sprite_pipeline(device);
    let uniforms = must(
        device.create_buffer(BufferDesc {
            size: 64,
            usage: BufferUsage::Uniform,
        }),
        "making the uniform buffer",
    );
    let identity: Vec<u8> = (0..16_usize)
        .flat_map(|at| if at % 5 == 0 { 1.0_f32 } else { 0.0 }.to_le_bytes())
        .collect();
    must(
        device.write_buffer(uniforms, 0, &identity),
        "writing the camera",
    );
    let white = must(
        device.create_texture(TextureDesc {
            width: 1,
            height: 1,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        }),
        "making the white texture",
    );
    let area = Rect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    };
    must(
        device.write_texture(white, area, &[255; 4]),
        "writing the texture",
    );
    let sampler = must(
        device.create_sampler(SamplerDesc {
            min_filter: Filter::Nearest,
            mag_filter: Filter::Nearest,
            address: AddressMode::Clamp,
        }),
        "making the sampler",
    );
    Sprite {
        pipeline,
        uniforms,
        white,
        sampler,
    }
}

/// A red quad over the clip-space rectangle `[x0, y0, x1, y1]`, as two triangles.
fn quad(device: &mut VulkanDevice, [x0, y0, x1, y1]: [f32; 4]) -> BufferId {
    let corner = |x: f32, y: f32| [x, y, 0.5, 0.5, 1.0, 0.0, 0.0, 1.0];
    let vertices: Vec<f32> = [
        corner(x0, y0),
        corner(x1, y0),
        corner(x1, y1),
        corner(x0, y0),
        corner(x1, y1),
        corner(x0, y1),
    ]
    .concat();
    assert_eq!(vertices.len(), 6 * FLOATS_PER_VERTEX);
    let bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_le_bytes()).collect();
    let buffer = must(
        device.create_buffer(BufferDesc {
            size: u32::try_from(bytes.len()).unwrap_or(0),
            usage: BufferUsage::Vertex,
        }),
        "making the vertex buffer",
    );
    must(
        device.write_buffer(buffer, 0, &bytes),
        "writing the vertices",
    );
    buffer
}

fn target_texture(device: &mut VulkanDevice) -> TextureId {
    must(
        device.create_texture(TextureDesc {
            width: SIZE,
            height: SIZE,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        }),
        "making the target",
    )
}

/// One pass: clear to blue, set the pipeline and the binds, draw the quad, end.
fn quad_pass(sprite: &Sprite, target: RenderTarget, vertices: BufferId) -> CommandStream {
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target,
        clear_color: Some([0.0, 0.0, 1.0, 1.0]),
        clear_depth: None,
        depth: None,
    });
    stream.set_pipeline(sprite.pipeline);
    stream.bind_uniforms(0, sprite.uniforms);
    stream.bind_texture(0, sprite.white, sprite.sampler);
    stream.bind_vertex_buffer(0, vertices, 0);
    stream.draw(6, 1);
    stream.end_pass();
    stream
}

/// Runs `stream` in a frame of its own.
fn run_frame(rig: &mut Rig, surface: render::ids::SurfaceId, stream: &CommandStream) {
    let frame = must(rig.device.begin_frame(surface), "begin_frame");
    must(rig.device.submit(frame, stream), "submit");
    must(rig.device.end_frame(frame), "end_frame");
}

fn read_target(rig: &mut Rig, target: TextureId) -> Vec<u8> {
    let mut rows = vec![0_u8; usize::try_from(SIZE * SIZE * 4).unwrap_or(0)];
    must(rig.device.read_back(target, &mut rows), "read_back");
    rows
}

fn pixel(rows: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = usize::try_from((y * SIZE + x) * 4).unwrap_or(0);
    [rows[at], rows[at + 1], rows[at + 2], rows[at + 3]]
}

#[test]
fn the_sprite_shaders_draw_a_quad_into_a_texture_with_row_0_at_the_bottom() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let surface = must(rig.device.create_surface(rig.surface), "making the surface");
    let sprite = sprite(&mut rig.device);
    // The lower left quarter of the clip space.
    let vertices = quad(&mut rig.device, [-1.0, -1.0, 0.0, 0.0]);
    let target = target_texture(&mut rig.device);
    let stream = quad_pass(&sprite, RenderTarget::Texture(target), vertices);
    run_frame(&mut rig, surface, &stream);
    let rows = read_target(&mut rig, target);
    // Row 0 is the bottom row of the picture, as on OpenGL.
    assert_eq!(pixel(&rows, 2, 2), RED, "lower left");
    assert_eq!(pixel(&rows, 13, 2), BLUE, "lower right");
    assert_eq!(pixel(&rows, 2, 13), BLUE, "upper left");
    assert_eq!(pixel(&rows, 13, 13), BLUE, "upper right");
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}

#[test]
fn a_viewport_command_limits_the_drawing_to_its_rectangle() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let surface = must(rig.device.create_surface(rig.surface), "making the surface");
    let sprite = sprite(&mut rig.device);
    // A quad over the whole clip space, seen through the upper right quarter of the target.
    let vertices = quad(&mut rig.device, [-1.0, -1.0, 1.0, 1.0]);
    let target = target_texture(&mut rig.device);
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(target),
        clear_color: Some([0.0, 0.0, 1.0, 1.0]),
        clear_depth: None,
        depth: None,
    });
    stream.set_pipeline(sprite.pipeline);
    stream.set_viewport(Rect {
        x: 8,
        y: 8,
        width: 8,
        height: 8,
    });
    stream.bind_uniforms(0, sprite.uniforms);
    stream.bind_texture(0, sprite.white, sprite.sampler);
    stream.bind_vertex_buffer(0, vertices, 0);
    stream.draw(6, 1);
    stream.end_pass();
    run_frame(&mut rig, surface, &stream);
    let rows = read_target(&mut rig, target);
    assert_eq!(pixel(&rows, 12, 12), RED, "inside the rectangle");
    assert_eq!(pixel(&rows, 2, 12), BLUE, "left of it");
    assert_eq!(pixel(&rows, 12, 2), BLUE, "below it");
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}

#[test]
fn the_sprite_shaders_draw_into_the_backbuffer_many_frames_in_a_row() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let surface = must(rig.device.create_surface(rig.surface), "making the surface");
    let sprite = sprite(&mut rig.device);
    let vertices = quad(&mut rig.device, [-0.5, -0.5, 0.5, 0.5]);
    let indices = must(
        rig.device.create_buffer(BufferDesc {
            size: 12,
            usage: BufferUsage::Index,
        }),
        "making the index buffer",
    );
    let order: Vec<u8> = [0_u16, 1, 2, 3, 4, 5]
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    must(
        rig.device.write_buffer(indices, 0, &order),
        "writing the indices",
    );
    // The same pass twice, drawn with a vertex count and then with indices.
    let plain = quad_pass(&sprite, RenderTarget::Backbuffer, vertices);
    let mut indexed = CommandStream::new();
    indexed.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
        depth: None,
    });
    indexed.set_pipeline(sprite.pipeline);
    indexed.bind_uniforms(0, sprite.uniforms);
    indexed.bind_texture(0, sprite.white, sprite.sampler);
    indexed.bind_vertex_buffer(0, vertices, 0);
    indexed.bind_index_buffer(indices, 0, IndexType::U16);
    indexed.draw_indexed(6, 1);
    indexed.end_pass();
    for _ in 0..6 {
        let frame = must(rig.device.begin_frame(surface), "begin_frame");
        must(rig.device.submit(frame, &plain), "submit");
        must(rig.device.submit(frame, &indexed), "submit indexed");
        must(rig.device.end_frame(frame), "end_frame");
    }
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}

/// The image test of card 6-10: the scene of the OpenGL image test (six sprites through the sprite
/// batcher, the camera, the render graph and the SPIR-V sprite shaders) gives the same reference
/// picture on the Vulkan device, row 0 at the bottom.
#[test]
fn a_sprite_scene_renders_the_expected_pixels_on_the_vulkan_device() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let surface = must(rig.device.create_surface(rig.surface), "making the surface");
    let target = must(
        rig.device.create_texture(TextureDesc {
            width: scene::SIDE,
            height: scene::SIDE,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        }),
        "making the target",
    );
    let codes = scene::stage_codes(SPRITE_SPIRV);
    let stream = scene::record_scene(&mut rig.device, target, codes);
    let frame = must(rig.device.begin_frame(surface), "begin_frame");
    let stats = must(rig.device.submit(frame, &stream), "submit");
    must(rig.device.end_frame(frame), "end_frame");
    assert_eq!(stats.draws, scene::DRAWS);
    let mut pixels = vec![0_u8; usize::try_from(scene::SIDE * scene::SIDE * 4).unwrap_or(0)];
    must(rig.device.read_back(target, &mut pixels), "read_back");
    scene::assert_reference(&pixels);
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}

/// The image test of card 6-11: the two-cube scene of the OpenGL mesh test (perspective camera,
/// depth texture, SPIR-V mesh shaders of the mesh sample) gives the same probes on the Vulkan
/// device, and the control frame without the depth test shows the far cube winning the overlap.
#[test]
fn two_cubes_render_with_the_near_one_in_front_on_the_vulkan_device() {
    let mut rig = match Rig::start() {
        Ok(rig) => rig,
        Err(reason) => return not_run(&reason),
    };
    let surface = must(rig.device.create_surface(rig.surface), "making the surface");
    let codes = mesh::stage_codes(MESH_SPIRV);
    let pixels = mesh::render(&mut rig.device, surface, codes, true);
    mesh::assert_probes(&pixels);
    let pixels = mesh::render(&mut rig.device, surface, codes, false);
    mesh::assert_control(&pixels);
    rig.device.destroy_surface(surface);
    rig.assert_quiet();
}
