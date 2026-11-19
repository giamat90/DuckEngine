//! Card 8-9: the lit mesh frontend on the Null device. Four generated primitives, each with its own
//! transform and material buffer and one shared light buffer, are drawn through the real device
//! calls (the Null device validates every command), and encoding the blocks and recording a
//! frame make no allocator call once the stream and the queue are sized.

use base::alloc::{CountingAllocator, count_allocations};
use base::math::{Mat4, Vec3};
use render::camera::PerspectiveCamera;
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc,
    RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SamplerId, SurfaceId, TextureId};
use render::lit::{
    DirectionalLight, LIGHT_BLOCK_BYTES, LitBindings, LitDraw, LitQueue, Material,
    TRANSFORM_BLOCK_BYTES, transform_block,
};
use render::mesh::{MESH_VERTEX_STRIDE, MeshVertex, encode_mesh_vertices, mesh_vertex_attributes};
use render::null::NullDevice;
use render::shapes::{Capsule, MeshCounts, write_box, write_capsule, write_plane, write_sphere};
use render::stream::CommandStream;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const SIDE: u32 = 8;
const MESHES: usize = 4;

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
        position: Vec3::new(3.0, 3.0, 6.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: 1.0,
        aspect: 1.0,
        near: 0.1,
        far: 40.0,
    }
}

fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(-1.0, -2.0, -1.0),
        colour: [1.0, 0.95, 0.9],
        ambient: [0.1, 0.1, 0.15],
    }
}

fn buffer(device: &mut NullDevice, usage: BufferUsage, bytes: &[u8]) -> BufferId {
    let size = u32::try_from(bytes.len()).unwrap_or(0);
    let id = ok(device.create_buffer(BufferDesc { size, usage }));
    ok(device.write_buffer(id, 0, bytes));
    id
}

fn lit_pipeline(device: &mut NullDevice) -> PipelineId {
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

/// A mesh in the device: its buffers and index count.
struct Mesh {
    vertices: BufferId,
    indices: BufferId,
    index_count: u32,
}

fn upload(device: &mut NullDevice, vertices: &[MeshVertex], indices: &[u16]) -> Mesh {
    let mut vertex_bytes = vec![0u8; vertices.len() * MESH_VERTEX_STRIDE as usize];
    assert!(encode_mesh_vertices(vertices, &mut vertex_bytes));
    let index_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    Mesh {
        vertices: buffer(device, BufferUsage::Vertex, &vertex_bytes),
        indices: buffer(device, BufferUsage::Index, &index_bytes),
        index_count: u32::try_from(indices.len()).unwrap_or(0),
    }
}

fn generated(
    device: &mut NullDevice,
    write: impl Fn(&mut [MeshVertex], &mut [u16]) -> Option<MeshCounts>,
) -> Mesh {
    let mut vertices = vec![MeshVertex::default(); 4096];
    let mut indices = vec![0u16; 16384];
    let counts = write(&mut vertices, &mut indices);
    assert!(counts.is_some(), "the generator refused its parameters");
    let counts = counts.unwrap_or(MeshCounts {
        vertices: 0,
        indices: 0,
    });
    upload(
        device,
        &vertices[..counts.vertices],
        &indices[..counts.indices],
    )
}

struct Scene {
    device: NullDevice,
    draws: Vec<LitDraw>,
    surface: SurfaceId,
    depth: TextureId,
    models: [Mat4; MESHES],
    transforms: [BufferId; MESHES],
}

fn object(
    device: &mut NullDevice,
    mesh: &Mesh,
    shared: Shared,
    model: &Mat4,
) -> (LitDraw, BufferId) {
    let mvp = camera().mvp(model).unwrap_or(Mat4::IDENTITY);
    let block = transform_block(&mvp, model).unwrap_or([0; TRANSFORM_BLOCK_BYTES]);
    let transform = buffer(device, BufferUsage::Uniform, &block);
    let material = Material {
        colour: [0.8, 0.4, 0.2, 1.0],
    }
    .block_bytes();
    let draw = LitDraw {
        bindings: LitBindings {
            pipeline: shared.pipeline,
            vertices: mesh.vertices,
            indices: mesh.indices,
            transform,
            light: shared.light,
            material: buffer(device, BufferUsage::Uniform, &material),
            texture: shared.texture,
            sampler: shared.sampler,
        },
        index_count: mesh.index_count,
    };
    (draw, transform)
}

#[derive(Clone, Copy)]
struct Shared {
    pipeline: PipelineId,
    light: BufferId,
    texture: TextureId,
    sampler: SamplerId,
}

fn make_shared(device: &mut NullDevice) -> Shared {
    let light_block = sun().block_bytes().unwrap_or([0; LIGHT_BLOCK_BYTES]);
    Shared {
        pipeline: lit_pipeline(device),
        light: buffer(device, BufferUsage::Uniform, &light_block),
        texture: ok(device.create_texture(TextureDesc {
            width: 2,
            height: 2,
            mip_count: 1,
            format: TextureFormat::Rgba8,
        })),
        sampler: ok(device.create_sampler(SamplerDesc {
            min_filter: Filter::Nearest,
            mag_filter: Filter::Nearest,
            address: AddressMode::Clamp,
        })),
    }
}

fn make_meshes(device: &mut NullDevice) -> [Mesh; MESHES] {
    let capsule = Capsule {
        radius: 0.3,
        half_height: 0.5,
        segments: 12,
        cap_rings: 4,
    };
    [
        generated(device, |v, i| write_sphere(0.5, 16, 8, v, i)),
        generated(device, |v, i| write_capsule(capsule, v, i)),
        generated(device, |v, i| write_box(Vec3::new(0.5, 0.25, 0.5), v, i)),
        generated(device, |v, i| write_plane(4.0, 4.0, v, i)),
    ]
}

fn scene() -> Scene {
    let mut device = NullDevice::new();
    let shared = make_shared(&mut device);
    let meshes = make_meshes(&mut device);
    let models = [
        Mat4::IDENTITY,
        Mat4::from_rotation_z(0.5),
        Mat4::from_rotation_y(0.7),
        Mat4::IDENTITY,
    ];
    let (mut draws, mut transforms) = (Vec::new(), [BufferId::from_raw(0, 0); MESHES]);
    for (n, (mesh, model)) in meshes.iter().zip(&models).enumerate() {
        let (draw, transform) = object(&mut device, mesh, shared, model);
        draws.push(draw);
        transforms[n] = transform;
    }
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
        draws,
        surface,
        depth,
        models,
        transforms,
    }
}

fn frame(stream: &mut CommandStream, queue: &mut LitQueue, scene: &Scene, depth: bool) {
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.0, 0.0, 0.25, 1.0]),
        clear_depth: Some(1.0),
        depth: depth.then_some(scene.depth),
    });
    queue.clear();
    for draw in &scene.draws {
        assert!(queue.push(*draw).is_ok());
    }
    queue.sort();
    queue.record(stream);
    stream.end_pass();
}

#[test]
fn the_null_device_accepts_a_lit_frame_of_four_primitives() {
    let mut scene = scene();
    let (mut stream, mut queue) = (CommandStream::new(), LitQueue::new(MESHES));
    frame(&mut stream, &mut queue, &scene, true);
    let begun = ok(scene.device.begin_frame(scene.surface));
    let stats = ok(scene.device.submit(begun, &stream));
    ok(scene.device.end_frame(begun));
    assert_eq!(stats.draws, 4);
}

#[test]
fn a_lit_pipeline_with_depth_is_refused_in_a_pass_without_a_depth_attachment() {
    let mut scene = scene();
    let (mut stream, mut queue) = (CommandStream::new(), LitQueue::new(MESHES));
    frame(&mut stream, &mut queue, &scene, false);
    let begun = ok(scene.device.begin_frame(scene.surface));
    let result = scene.device.submit(begun, &stream);
    ok(scene.device.end_frame(begun));
    assert_eq!(result.map(|_| ()), Err(RenderError::InvalidDescription));
}

#[test]
fn a_block_buffer_of_the_wrong_size_is_not_what_the_frame_writes() {
    let mut scene = scene();
    let wrong = vec![0u8; TRANSFORM_BLOCK_BYTES + 1];
    assert_eq!(
        scene.device.write_buffer(scene.transforms[0], 0, &wrong),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn recording_a_lit_frame_and_encoding_its_blocks_makes_no_allocator_call() {
    let mut scene = scene();
    let (mut stream, mut queue) = (CommandStream::new(), LitQueue::new(MESHES));
    for _ in 0..2 {
        frame(&mut stream, &mut queue, &scene, true);
        stream.clear();
    }
    let (written, counts) = count_allocations(|| {
        let mut written = 0;
        for (model, transform) in scene.models.iter().zip(scene.transforms) {
            let mvp = camera().mvp(model).unwrap_or(Mat4::IDENTITY);
            let block = transform_block(&mvp, model).unwrap_or([0; TRANSFORM_BLOCK_BYTES]);
            written += usize::from(scene.device.write_buffer(transform, 0, &block).is_ok());
        }
        let light = sun().block_bytes().unwrap_or([0; LIGHT_BLOCK_BYTES]);
        written += light.len();
        frame(&mut stream, &mut queue, &scene, true);
        written
    });
    assert_eq!(written, MESHES + LIGHT_BLOCK_BYTES);
    assert_eq!(counts.calls(), 0);
}
