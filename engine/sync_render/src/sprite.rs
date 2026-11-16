//! The component that makes an entity a sprite.

use base::math::Vec2;
use render::ids::TextureId;
use render::sprite::UvRect;

/// How an entity is drawn as a sprite. The place and the turn come from the world transform of
/// the entity; `size` is the size at scale one, and the scale of the world transform multiplies it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteRenderer {
    /// The texture to sample.
    pub texture: TextureId,
    /// World size at scale one; both components must be positive.
    pub size: Vec2,
    /// Texture rectangle.
    pub uv: UvRect,
    /// Colour multiplied with the texel (RGBA, 8 bits each).
    pub tint: [u8; 4],
    /// Drawing order: lower layers first.
    pub layer: i16,
}

impl SpriteRenderer {
    /// The whole texture, white tint, layer zero.
    #[must_use]
    pub fn new(texture: TextureId, size: Vec2) -> Self {
        Self {
            texture,
            size,
            uv: UvRect::FULL,
            tint: [255; 4],
            layer: 0,
        }
    }
}
