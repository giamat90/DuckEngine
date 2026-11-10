//! Card 6-3: the mesh frontend on the Null device. A cube is drawn with depth testing through the
//! real device calls (the Null device validates every command), and recording a frame makes no
//! allocator call once the stream is sized.

use base::alloc::{CountingAllocator, count_allocations};
use base::math::{Mat4, Vec3};
use render::camera::{PerspectiveCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc,
    Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc,
    TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::mesh::{
    CUBE_INDEX_COUNT, MESH_VERTEX_STRIDE, MeshBindings, cube_indices, cube_vertices,
    encode_mesh_vertices, mesh_vertex_attributes, record_mesh,
};
use render::null::NullDevice;
use render::stream::CommandStream;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const SIDE: u32 = 8;

fn ok<T>(result: Result<T, RenderError>) -> T {
    assert!(
        result.is_ok(),
        "device refused: {:?}",
        result.as_ref().err()
    );
    result.unwrap_or_else(|_| unreachable!())
}

fn camera() -> PerspectiveCamera {
    PerspectiveCamera {
        position: Vec3::new(2.0, 2.0, 3.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: 1.0,
        aspect: 1.0,
        near: 0.1,
        far: 20.0,
    }
}

struct Scene {
    device: NullDevice,
    bindings: MeshBindings,
    surface: render::ids::SurfaceId,
    depth: render::ids::TextureId,
}

fn buffer(device: &mut NullDevice, usage: BufferUsage, bytes: &[u8]) -> render::ids::BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let id = ok(device.create_buffer(BufferDesc { size, usage }));
    ok(device.write_buffer(id, 0, bytes));
    id
}

fn depth_pipeline(device: &mut NullDevice) -> render::ids::PipelineId {
    let shader = |d: &mut NullDevice, stage| {
        ok(d.create_shader(ShaderDesc {
            stage,
            code: &[1],
            entry: "main",
        }))
    };
    let vertex_shader = shader(device, ShaderStage::Vertex);
    let fragment_shader = shader(device, ShaderStage::Fragment);
    ok(device.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: MESH_VERTEX_STRIDE,
        attributes: &mesh_vertex_attributes(),
        blend: BlendMode::Opaque,
        depth: DepthState {
            test: true,
            write: true,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: Some(TextureFormat::Depth32F),
    }))
}

fn scene() -> Scene {
    let mut device = NullDevice::new();
    let pipeline = depth_pipeline(&mut device);
    let mut vertex_bytes = vec![0u8; 24 * MESH_VERTEX_STRIDE as usize];
    assert!(encode_mesh_vertices(&cube_vertices(), &mut vertex_bytes));
    let index_bytes: Vec<u8> = cube_indices()
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    let mvp = camera().mvp(&Mat4::IDENTITY).unwrap_or(Mat4::IDENTITY);
    let texture = ok(device.create_texture(TextureDesc {
        width: 2,
        height: 2,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let sampler = ok(device.create_sampler(SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }));
    let bindings = MeshBindings {
        pipeline,
        vertices: buffer(&mut device, BufferUsage::Vertex, &vertex_bytes),
        uniforms: buffer(&mut device, BufferUsage::Uniform, &matrix_bytes(&mvp)),
        indices: buffer(&mut device, BufferUsage::Index, &index_bytes),
        texture,
        sampler,
    };
    let surface = ok(device.create_surface(SurfaceDesc {
        native_token: 1,
        width: SIDE,
        height: SIDE,
    }));
    let depth = ok(device.create_texture(TextureDesc {
        width: SIDE,
        height: SIDE,
        mip_count: 1,
        format: TextureFormat::Depth32F,
    }));
    Scene {
        device,
        bindings,
        surface,
        depth,
    }
}

fn frame(stream: &mut CommandStream, scene: &Scene) {
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.0, 0.0, 0.25, 1.0]),
        clear_depth: Some(1.0),
        depth: Some(scene.depth),
    });
    stream.set_viewport(Rect {
        x: 0,
        y: 0,
        width: SIDE,
        height: SIDE,
    });
    record_mesh(stream, &scene.bindings, CUBE_INDEX_COUNT);
    stream.end_pass();
}

#[test]
fn the_null_device_accepts_a_depth_tested_cube_frame_and_counts_its_draw() {
    let mut scene = scene();
    let mut stream = CommandStream::new();
    frame(&mut stream, &scene);
    let begun = ok(scene.device.begin_frame(scene.surface));
    let stats = ok(scene.device.submit(begun, &stream));
    ok(scene.device.end_frame(begun));
    assert_eq!(stats.draws, 1);
    assert_eq!(stats.commands, 9);
}

#[test]
fn a_cube_pipeline_with_depth_is_refused_in_a_pass_without_a_depth_attachment() {
    let mut scene = scene();
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
        depth: None,
    });
    record_mesh(&mut stream, &scene.bindings, CUBE_INDEX_COUNT);
    stream.end_pass();
    let begun = ok(scene.device.begin_frame(scene.surface));
    let result = scene.device.submit(begun, &stream);
    ok(scene.device.end_frame(begun));
    assert_eq!(result.map(|_| ()), Err(RenderError::InvalidDescription));
}

#[test]
fn recording_a_mesh_frame_makes_no_allocator_call() {
    let scene = scene();
    let mut stream = CommandStream::new();
    for _ in 0..2 {
        frame(&mut stream, &scene);
        stream.clear();
    }
    let (matrix, counts) = count_allocations(|| {
        let matrix = camera()
            .mvp(&Mat4::from_rotation_y(0.5))
            .map(|m| matrix_bytes(&m));
        frame(&mut stream, &scene);
        matrix
    });
    assert!(matrix.is_some());
    assert_eq!(counts.calls(), 0);
}
