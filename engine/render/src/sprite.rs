//! The 2D sprite batcher (design: `wiki/architecture/render-port-v0.md` section 5).
//!
//! Sprites are plain data. [`SpriteBatcher::prepare`] orders them by layer, then texture, then
//! insertion index (a total order, so the result is deterministic and no stable sort is needed),
//! writes four vertices per sprite into a caller-supplied staging slice and groups runs of one
//! texture into [`Batch`]es. [`SpriteBatcher::record`] writes the bindings and one `draw_indexed`
//! per batch into a [`CommandStream`]. The index buffer is shared: two triangles per quad
//! ([`quad_indices`]). After `new`, nothing here allocates.

use crate::desc::{IndexType, VertexAttribute, VertexFormat};
use crate::ids::{BufferId, PipelineId, SamplerId, TextureId};
use crate::stream::CommandStream;
use base::math::Vec2;

/// Most sprites per batcher: four vertices each must be addressable by 16-bit indices.
pub const MAX_SPRITES: usize = 16_384;
/// Bytes of one [`SpriteVertex`] as the shader reads it.
pub const VERTEX_STRIDE: u32 = 20;
const VERTICES_PER_SPRITE: usize = 4;
const INDICES_PER_SPRITE: u32 = 6;
const SPRITE_BYTES: u32 = 4 * VERTEX_STRIDE;

/// A texture rectangle in normalised coordinates; `min` is the top-left corner of the image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvRect {
    /// Top-left.
    pub min: Vec2,
    /// Bottom-right.
    pub max: Vec2,
}

impl UvRect {
    /// The whole texture.
    pub const FULL: Self = Self {
        min: Vec2::new(0.0, 0.0),
        max: Vec2::new(1.0, 1.0),
    };
}

/// One sprite: a textured rectangle around `position`, turned by `angle` radians counter-clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// The texture to sample.
    pub texture: TextureId,
    /// World position of the centre.
    pub position: Vec2,
    /// World size; both components must be positive (a negative one flips the winding).
    pub size: Vec2,
    /// Rotation about the centre, radians, counter-clockwise.
    pub angle: f32,
    /// Texture rectangle.
    pub uv: UvRect,
    /// Colour multiplied with the texel (RGBA, 8 bits each).
    pub tint: [u8; 4],
    /// Drawing order: lower layers first.
    pub layer: i16,
}

/// A vertex as staged for the GPU: 20 bytes, see [`vertex_attributes`] and [`encode_vertices`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpriteVertex {
    /// World position.
    pub position: [f32; 2],
    /// Texture coordinate.
    pub uv: [f32; 2],
    /// Tint.
    pub color: [u8; 4],
}

/// The vertex layout for the pipeline: location 0 position, 1 uv, 2 colour.
#[must_use]
pub fn vertex_attributes() -> [VertexAttribute; 3] {
    [
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
            format: VertexFormat::Unorm8x4,
            offset: 16,
        },
    ]
}

/// Writes `vertices` as little-endian bytes into `out`; `false` (nothing written) when `out` is
/// not exactly big enough.
#[must_use]
pub fn encode_vertices(vertices: &[SpriteVertex], out: &mut [u8]) -> bool {
    let stride = VERTEX_STRIDE as usize;
    if out.len() != vertices.len() * stride {
        return false;
    }
    for (bytes, v) in out.chunks_exact_mut(stride).zip(vertices) {
        let floats = [v.position[0], v.position[1], v.uv[0], v.uv[1]];
        for (slot, value) in bytes.chunks_exact_mut(4).zip(floats) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        bytes[16..20].copy_from_slice(&v.color);
    }
    true
}

/// The shared index buffer for `sprites` quads (at most [`MAX_SPRITES`]): corners 0 1 2 and 2 1 3 of each, counter-clockwise.
#[must_use]
pub fn quad_indices(sprites: usize) -> Vec<u16> {
    (0..sprites.min(MAX_SPRITES))
        .flat_map(|quad| {
            let base = u16::try_from(quad * VERTICES_PER_SPRITE).unwrap_or(u16::MAX);
            [0, 1, 2, 2, 1, 3].map(|corner| base.saturating_add(corner))
        })
        .collect()
}

/// A run of sprites that share a texture: one draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    /// The texture of the run.
    pub texture: TextureId,
    /// Position of the first sprite in sorted order.
    pub first_sprite: u32,
    /// Number of sprites.
    pub sprite_count: u32,
}

/// What can go wrong preparing a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    /// More sprites than the batcher was built for (or than [`MAX_SPRITES`]).
    TooManySprites,
    /// The staging slice has fewer than four vertices per sprite.
    StagingTooSmall,
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManySprites => write!(f, "too many sprites for this batcher"),
            Self::StagingTooSmall => write!(f, "vertex staging buffer too small"),
        }
    }
}

impl std::error::Error for BatchError {}

/// The resources a sprite draw binds. Created by the caller through the device.
#[derive(Clone, Copy, Debug)]
pub struct SpriteBindings {
    /// Pipeline with [`vertex_attributes`] and [`VERTEX_STRIDE`].
    pub pipeline: PipelineId,
    /// Uniform buffer holding the camera matrix ([`crate::camera::matrix_bytes`]).
    pub uniforms: BufferId,
    /// Vertex buffer the staged vertices were uploaded to.
    pub vertices: BufferId,
    /// Index buffer filled from [`quad_indices`] (16-bit).
    pub indices: BufferId,
    /// Sampler for all sprites.
    pub sampler: SamplerId,
}

/// Total order of a sprite: layer (biased to unsigned), texture, insertion index.
type SortKey = (u16, u32, u32, u32);

/// Sorts and batches sprites; owns its scratch space.
pub struct SpriteBatcher {
    keys: Vec<SortKey>,
    batches: Vec<Batch>,
    max_sprites: usize,
}

fn sort_key(sprite: &Sprite, index: usize) -> SortKey {
    let layer = u16::try_from(i32::from(sprite.layer) + 32_768).unwrap_or(0);
    let index = u32::try_from(index).unwrap_or(u32::MAX);
    (
        layer,
        sprite.texture.index(),
        sprite.texture.generation(),
        index,
    )
}

fn corners(sprite: &Sprite) -> [[f32; 2]; 4] {
    let (sin, cos) = sprite.angle.sin_cos();
    let (hx, hy) = (sprite.size.x * 0.5, sprite.size.y * 0.5);
    [(-hx, -hy), (hx, -hy), (-hx, hy), (hx, hy)].map(|(x, y)| {
        [
            sprite.position.x + x * cos - y * sin,
            sprite.position.y + x * sin + y * cos,
        ]
    })
}

fn vertices_of(sprite: &Sprite) -> [SpriteVertex; 4] {
    let (min, max) = (sprite.uv.min, sprite.uv.max);
    let uvs = [
        [min.x, max.y],
        [max.x, max.y],
        [min.x, min.y],
        [max.x, min.y],
    ];
    let positions = corners(sprite);
    std::array::from_fn(|i| SpriteVertex {
        position: positions[i],
        uv: uvs[i],
        color: sprite.tint,
    })
}

/// Writes the vertices of the sprite `key` names into `slot`.
fn stage(slot: &mut [SpriteVertex], sprites: &[Sprite], key: &SortKey) {
    if let Some(sprite) = sprites.get(key.3 as usize) {
        slot.copy_from_slice(&vertices_of(sprite));
    }
}

impl SpriteBatcher {
    /// A batcher for up to `max_sprites` sprites per frame. Allocates its scratch space once.
    ///
    /// # Errors
    /// [`BatchError::TooManySprites`] when `max_sprites` exceeds [`MAX_SPRITES`].
    pub fn new(max_sprites: usize) -> Result<Self, BatchError> {
        if max_sprites > MAX_SPRITES {
            return Err(BatchError::TooManySprites);
        }
        Ok(Self {
            keys: Vec::with_capacity(max_sprites),
            batches: Vec::with_capacity(max_sprites),
            max_sprites,
        })
    }

    /// The batches of the last successful [`Self::prepare`].
    #[must_use]
    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    /// Orders `sprites`, writes their vertices into `staging` (four per sprite, in drawing
    /// order) and returns the batches. Nothing is written when it fails.
    ///
    /// # Errors
    /// [`BatchError::TooManySprites`] or [`BatchError::StagingTooSmall`].
    pub fn prepare(
        &mut self,
        sprites: &[Sprite],
        staging: &mut [SpriteVertex],
    ) -> Result<&[Batch], BatchError> {
        self.batches.clear();
        if sprites.len() > self.max_sprites {
            return Err(BatchError::TooManySprites);
        }
        if staging.len() < sprites.len() * VERTICES_PER_SPRITE {
            return Err(BatchError::StagingTooSmall);
        }
        self.keys.clear();
        self.keys
            .extend(sprites.iter().enumerate().map(|(i, s)| sort_key(s, i)));
        self.keys.sort_unstable();
        let slots = staging.chunks_exact_mut(VERTICES_PER_SPRITE);
        for (slot, key) in slots.zip(&self.keys) {
            stage(slot, sprites, key);
        }
        self.group(sprites);
        Ok(&self.batches)
    }

    /// Groups the sorted sprites into runs of one texture.
    fn group(&mut self, sprites: &[Sprite]) {
        for (position, key) in self.keys.iter().enumerate() {
            let texture = sprites[key.3 as usize].texture;
            match self.batches.last_mut() {
                Some(batch) if batch.texture == texture => batch.sprite_count += 1,
                _ => self.batches.push(Batch {
                    texture,
                    first_sprite: u32::try_from(position).unwrap_or(u32::MAX),
                    sprite_count: 1,
                }),
            }
        }
    }

    /// Writes the draws of the last [`Self::prepare`] into `stream` (inside a pass): pipeline,
    /// camera uniforms and index buffer once, then per batch the vertex range, the texture and
    /// one `draw_indexed`. Writes nothing when there is nothing to draw.
    pub fn record(&self, stream: &mut CommandStream, bindings: &SpriteBindings) {
        if self.batches.is_empty() {
            return;
        }
        stream.set_pipeline(bindings.pipeline);
        stream.bind_uniforms(0, bindings.uniforms);
        stream.bind_index_buffer(bindings.indices, 0, IndexType::U16);
        for batch in &self.batches {
            let offset = batch.first_sprite * SPRITE_BYTES;
            stream.bind_vertex_buffer(0, bindings.vertices, offset);
            stream.bind_texture(0, batch.texture, bindings.sampler);
            stream.draw_indexed(batch.sprite_count * INDICES_PER_SPRITE, 1);
        }
    }
}

#[cfg(test)]
mod tests;
