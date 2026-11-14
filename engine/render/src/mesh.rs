//! The 3D mesh frontend (card 6-3; design: `wiki/architecture/vulkan-and-mesh-v0.md` section 5).
//!
//! The 3D counterpart of the sprite batcher, with the same split: plain data in, bytes and
//! commands out, no new port call. A [`MeshVertex`] is position, normal and uv in one interleaved
//! vertex buffer (slot 0, [`MESH_VERTEX_STRIDE`] bytes, [`mesh_vertex_attributes`]); the
//! model-view-projection matrix is a uniform buffer written from
//! [`crate::camera::matrix_bytes`]; [`record_mesh`] writes the bindings and one `draw_indexed`.
//! [`cube_vertices`] and [`cube_indices`] are the one built-in mesh.
//! Nothing here allocates.

use crate::desc::{IndexType, VertexAttribute, VertexFormat};
use crate::ids::{BufferId, PipelineId, SamplerId, TextureId};
use crate::stream::CommandStream;

/// Bytes of a position or a normal (three floats).
const VECTOR_BYTES: u32 = 12;
/// Bytes of a texture coordinate (two floats).
const UV_BYTES: u32 = 8;
/// Bytes of one [`MeshVertex`] as the shader reads it.
pub const MESH_VERTEX_STRIDE: u32 = 2 * VECTOR_BYTES + UV_BYTES;
/// Vertices of [`cube_vertices`]: four per face, so that each face has its own normal and uv.
pub const CUBE_VERTEX_COUNT: usize = 24;
/// Indices of [`cube_indices`]: two triangles per face.
pub const CUBE_INDEX_COUNT: u32 = 36;

const FACES: usize = 6;
const VERTICES_PER_FACE: usize = 4;
const INDICES_PER_FACE: usize = 6;
const HALF: f32 = 0.5;
/// The two triangles of a face, as corners of its quad: 0 1 2 and 2 1 3.
const QUAD_CORNERS: [u16; INDICES_PER_FACE] = [0, 1, 2, 2, 1, 3];

/// A vertex as staged for the GPU: 32 bytes, see [`mesh_vertex_attributes`] and
/// [`encode_mesh_vertices`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshVertex {
    /// Model-space position.
    pub position: [f32; 3],
    /// Model-space unit normal.
    pub normal: [f32; 3],
    /// Texture coordinate; (0, 0) is the top-left corner of the image.
    pub uv: [f32; 2],
}

/// The vertex layout for the pipeline: location 0 position, 1 normal, 2 uv.
#[must_use]
pub fn mesh_vertex_attributes() -> [VertexAttribute; 3] {
    [
        VertexAttribute {
            location: 0,
            format: VertexFormat::Float3,
            offset: 0,
        },
        VertexAttribute {
            location: 1,
            format: VertexFormat::Float3,
            offset: VECTOR_BYTES,
        },
        VertexAttribute {
            location: 2,
            format: VertexFormat::Float2,
            offset: 2 * VECTOR_BYTES,
        },
    ]
}

/// Writes `vertices` as little-endian bytes into `out`; `false` (nothing written) when `out` is
/// not exactly big enough.
#[must_use]
pub fn encode_mesh_vertices(vertices: &[MeshVertex], out: &mut [u8]) -> bool {
    let stride = MESH_VERTEX_STRIDE as usize;
    if out.len() != vertices.len() * stride {
        return false;
    }
    for (bytes, v) in out.chunks_exact_mut(stride).zip(vertices) {
        let floats = v.position.into_iter().chain(v.normal).chain(v.uv);
        for (slot, value) in bytes.chunks_exact_mut(4).zip(floats) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
    }
    true
}

/// One face: its outward normal and two tangents with `u x v = normal`, so corners taken as
/// (-u -v), (+u -v), (-u +v), (+u +v) run counter-clockwise seen from outside.
const FACE_AXES: [([f32; 3], [f32; 3], [f32; 3]); FACES] = [
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
];

/// The corner signs along (u, v) and the texture coordinate of each: the whole image per face.
const CORNERS: [([f32; 2], [f32; 2]); VERTICES_PER_FACE] = [
    ([-1.0, -1.0], [0.0, 1.0]),
    ([1.0, -1.0], [1.0, 1.0]),
    ([-1.0, 1.0], [0.0, 0.0]),
    ([1.0, 1.0], [1.0, 0.0]),
];

/// The unit cube (edge 1, centred on the origin) as 24 vertices, four per face in the order
/// +X, -X, +Y, -Y, +Z, -Z.
#[must_use]
pub fn cube_vertices() -> [MeshVertex; CUBE_VERTEX_COUNT] {
    std::array::from_fn(|i| {
        let (normal, u, v) = FACE_AXES[i / VERTICES_PER_FACE];
        let (sign, uv) = CORNERS[i % VERTICES_PER_FACE];
        MeshVertex {
            position: std::array::from_fn(|axis| {
                (normal[axis] + sign[0] * u[axis] + sign[1] * v[axis]) * HALF
            }),
            normal,
            uv,
        }
    })
}

/// The 36 indices of [`cube_vertices`]: per face the corners 0 1 2 and 2 1 3, counter-clockwise
/// seen from outside.
#[must_use]
pub fn cube_indices() -> [u16; CUBE_INDEX_COUNT as usize] {
    std::array::from_fn(|i| {
        let face = u16::try_from(i / INDICES_PER_FACE * VERTICES_PER_FACE).unwrap_or(u16::MAX);
        let corner = QUAD_CORNERS[i % INDICES_PER_FACE];
        face.saturating_add(corner)
    })
}

/// The resources one mesh draw binds. Created by the caller through the device.
#[derive(Clone, Copy, Debug)]
pub struct MeshBindings {
    /// Pipeline with [`mesh_vertex_attributes`], [`MESH_VERTEX_STRIDE`] and a depth format.
    pub pipeline: PipelineId,
    /// Vertex buffer holding [`MeshVertex`] bytes.
    pub vertices: BufferId,
    /// Uniform buffer holding the model-view-projection matrix ([`crate::camera::matrix_bytes`]).
    pub uniforms: BufferId,
    /// Index buffer (16-bit).
    pub indices: BufferId,
    /// The texture sampled by the fragment shader.
    pub texture: TextureId,
    /// Its sampler.
    pub sampler: SamplerId,
}

/// Writes one indexed draw of `index_count` indices (inside a pass): pipeline, matrix uniform,
/// index and vertex buffers, texture. Writes nothing for an empty mesh.
pub fn record_mesh(stream: &mut CommandStream, bindings: &MeshBindings, index_count: u32) {
    if index_count == 0 {
        return;
    }
    stream.set_pipeline(bindings.pipeline);
    stream.bind_uniforms(0, bindings.uniforms);
    stream.bind_index_buffer(bindings.indices, 0, IndexType::U16);
    stream.bind_vertex_buffer(0, bindings.vertices, 0);
    stream.bind_texture(0, bindings.texture, bindings.sampler);
    stream.draw_indexed(index_count, 1);
}

#[cfg(test)]
mod tests;
