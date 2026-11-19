//! The lit mesh frontend (card 8-9; design: `wiki/architecture/phase8-3d-v0.md` section 4).
//!
//! One directional light, an ambient term, Lambert diffuse, a material colour and a texture, on
//! the render port as it is: no new port call. Two uniform blocks, one per slot, each a whole
//! buffer the caller writes with `write_buffer`. Each shader module reads one block (the shader
//! tool accepts one per module), so the vertex stage gets the transform and the colour of the
//! object and hands the colour on, and the fragment stage gets the light:
//!
//! | slot | block | bytes | content (std140) |
//! |---|---|---|---|
//! | [`SLOT_OBJECT`] | [`object_block`] | 192 | `mat4 mvp`, `mat4 model`, `mat3 normal`, `vec4 colour` (vertex stage) |
//! | [`SLOT_LIGHT`] | [`DirectionalLight::block_bytes`] | 48 | `vec4 to_light`, `vec4 colour`, `vec4 ambient` (fragment stage) |
//!
//! A draw binds one buffer per slot, so the transform and the colour of each object live in
//! their own buffer (the stream cannot write buffer data between draws). [`LitQueue`] collects
//! the draws of a frame, orders them by pipeline, then texture, then insertion index (a total
//! order, so no stable sort is needed, as in the sprite batcher) and records them. After `new`,
//! nothing here allocates.

use crate::desc::IndexType;
use crate::ids::{BufferId, PipelineId, SamplerId, TextureId};
use crate::stream::CommandStream;
use base::math::{Mat3, Mat4, Vec3};

/// The slot of the object block (vertex stage).
pub const SLOT_OBJECT: u32 = 0;
/// The slot of the light block (fragment stage).
pub const SLOT_LIGHT: u32 = 1;

/// Bytes of one std140 `vec4` (and of one `mat3` column).
const VEC4_BYTES: usize = 16;
/// Bytes of a `mat4`.
const MAT4_BYTES: usize = 4 * VEC4_BYTES;
/// Bytes of a `mat3` in std140: three columns padded to four floats.
const MAT3_BYTES: usize = 3 * VEC4_BYTES;
/// Bytes of the object block: `mvp`, `model`, `normal`, `colour`.
pub const OBJECT_BLOCK_BYTES: usize = 2 * MAT4_BYTES + MAT3_BYTES + VEC4_BYTES;
/// Bytes of the light block: direction, colour, ambient.
pub const LIGHT_BLOCK_BYTES: usize = 3 * VEC4_BYTES;

/// Writes `values` as little-endian floats at the start of `out`.
fn put(out: &mut [u8], values: &[f32]) {
    for (slot, value) in out.chunks_exact_mut(4).zip(values) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

/// The matrix that carries normals of a surface placed by `model` (the inverse transpose of its
/// upper three by three part), not normalised: the shader normalises the transformed normal.
/// `None` when that part cannot be inverted or is not finite.
#[must_use]
pub fn normal_matrix(model: &Mat4) -> Option<Mat3> {
    let [x, y, z, _] = model.cols;
    let upper = Mat3::from_cols(
        Vec3::new(x.x, x.y, x.z),
        Vec3::new(y.x, y.y, y.z),
        Vec3::new(z.x, z.y, z.z),
    );
    if !upper.is_finite() {
        return None;
    }
    Some(upper.inverse()?.transpose())
}

/// The 192 bytes of the object block for an object placed by `model`, seen through `mvp`, painted
/// with `material`; `None` when `model` has no normal matrix ([`normal_matrix`]), `mvp` is not
/// finite or the colour is not.
#[must_use]
pub fn object_block(
    mvp: &Mat4,
    model: &Mat4,
    material: &Material,
) -> Option<[u8; OBJECT_BLOCK_BYTES]> {
    let normal = normal_matrix(model)?;
    if !mvp.is_finite() || !model.is_finite() || !material.colour.iter().all(|c| c.is_finite()) {
        return None;
    }
    let mut out = [0u8; OBJECT_BLOCK_BYTES];
    put(&mut out[..MAT4_BYTES], &mvp.to_cols_array());
    put(&mut out[MAT4_BYTES..2 * MAT4_BYTES], &model.to_cols_array());
    let normal_at = 2 * MAT4_BYTES;
    for (column, axis) in out[normal_at..normal_at + MAT3_BYTES]
        .chunks_exact_mut(VEC4_BYTES)
        .zip(normal.cols)
    {
        put(column, &[axis.x, axis.y, axis.z]);
    }
    put(&mut out[normal_at + MAT3_BYTES..], &material.colour);
    Some(out)
}

/// One directional light and the ambient term.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    /// The way the light travels (from the light into the scene); a length between about 1e-19 and
    /// 1e19 (outside it the block is refused, as for zero).
    pub direction: Vec3,
    /// Light colour, linear RGB.
    pub colour: [f32; 3],
    /// Ambient light colour, linear RGB, added to every surface.
    pub ambient: [f32; 3],
}

impl DirectionalLight {
    /// The 48 bytes of the light block: the unit vector from a surface towards the light, the
    /// colour and the ambient term (each padded to four floats); `None` when the direction has no
    /// length or is not finite.
    #[must_use]
    pub fn block_bytes(&self) -> Option<[u8; LIGHT_BLOCK_BYTES]> {
        let to_light = (-self.direction).normalized()?;
        if ![to_light.x, to_light.y, to_light.z]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        let mut out = [0u8; LIGHT_BLOCK_BYTES];
        put(
            &mut out[..VEC4_BYTES],
            &[to_light.x, to_light.y, to_light.z],
        );
        put(&mut out[VEC4_BYTES..2 * VEC4_BYTES], &self.colour);
        put(&mut out[2 * VEC4_BYTES..], &self.ambient);
        Some(out)
    }
}

/// The surface colour of a mesh; it multiplies the texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// RGBA, linear.
    pub colour: [f32; 4],
}

/// The resources one lit mesh draw binds. Created by the caller through the device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LitBindings {
    /// Pipeline with [`crate::mesh::mesh_vertex_attributes`], [`crate::mesh::MESH_VERTEX_STRIDE`]
    /// and a depth format.
    pub pipeline: PipelineId,
    /// Vertex buffer holding [`crate::mesh::MeshVertex`] bytes.
    pub vertices: BufferId,
    /// Index buffer (16-bit).
    pub indices: BufferId,
    /// Uniform buffer holding the [`object_block`].
    pub object: BufferId,
    /// Uniform buffer holding the [`DirectionalLight::block_bytes`].
    pub light: BufferId,
    /// The texture sampled by the fragment shader.
    pub texture: TextureId,
    /// Its sampler.
    pub sampler: SamplerId,
}

/// Writes one indexed draw of `index_count` indices (inside a pass): pipeline, the two uniform
/// blocks, index and vertex buffers, texture. Writes nothing for an empty mesh.
pub fn record_lit_mesh(stream: &mut CommandStream, bindings: &LitBindings, index_count: u32) {
    if index_count == 0 {
        return;
    }
    stream.set_pipeline(bindings.pipeline);
    stream.bind_uniforms(SLOT_OBJECT, bindings.object);
    stream.bind_uniforms(SLOT_LIGHT, bindings.light);
    stream.bind_index_buffer(bindings.indices, 0, IndexType::U16);
    stream.bind_vertex_buffer(0, bindings.vertices, 0);
    stream.bind_texture(0, bindings.texture, bindings.sampler);
    stream.draw_indexed(index_count, 1);
}

/// One draw of a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LitDraw {
    /// What the draw binds.
    pub bindings: LitBindings,
    /// Indices to draw.
    pub index_count: u32,
}

/// A draw and the place it was pushed at, the last key of the order.
#[derive(Clone, Copy, Debug)]
struct Queued {
    draw: LitDraw,
    inserted: u32,
}

impl Queued {
    fn key(&self) -> (u32, u32, u32, u32, u32) {
        let bindings = &self.draw.bindings;
        (
            bindings.pipeline.index(),
            bindings.pipeline.generation(),
            bindings.texture.index(),
            bindings.texture.generation(),
            self.inserted,
        )
    }
}

/// The queue is full: the draw was not added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull;

/// The draws of a frame, with a fixed capacity.
pub struct LitQueue {
    draws: Vec<LitDraw>,
    queued: Vec<Queued>,
    capacity: usize,
}

impl LitQueue {
    /// A queue for up to `capacity` draws. Allocates its space once.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            draws: Vec::with_capacity(capacity),
            queued: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Adds a draw after the ones already there.
    ///
    /// # Errors
    /// [`QueueFull`] when the queue holds `capacity` draws; the queue is unchanged.
    pub fn push(&mut self, draw: LitDraw) -> Result<(), QueueFull> {
        if self.queued.len() >= self.capacity {
            return Err(QueueFull);
        }
        let inserted = u32::try_from(self.queued.len()).unwrap_or(u32::MAX);
        self.queued.push(Queued { draw, inserted });
        Ok(())
    }

    /// Removes every draw; the capacity stays.
    pub fn clear(&mut self) {
        self.queued.clear();
        self.draws.clear();
    }

    /// Orders the draws by pipeline, then texture, then insertion; the result is [`Self::draws`].
    pub fn sort(&mut self) {
        self.queued.sort_unstable_by_key(Queued::key);
        self.draws.clear();
        self.draws.extend(self.queued.iter().map(|q| q.draw));
    }

    /// The draws, in the order of the last [`Self::sort`] (empty before the first one).
    #[must_use]
    pub fn draws(&self) -> &[LitDraw] {
        &self.draws
    }

    /// How many draws the queue holds at most.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Writes every draw of [`Self::draws`] into `stream` (inside a pass).
    pub fn record(&self, stream: &mut CommandStream) {
        for draw in &self.draws {
            record_lit_mesh(stream, &draw.bindings, draw.index_count);
        }
    }
}

#[cfg(test)]
mod tests;
