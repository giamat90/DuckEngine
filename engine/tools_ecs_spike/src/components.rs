//! The component types and the entity kinds of the sandbox shape.
//!
//! Every value is an integer held in an `f32`, so a sum over the entities is exact whatever the
//! iteration order and all storages can be compared by a checksum.

/// Position, rotation and scale (16 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// X position.
    pub x: f32,
    /// Y position.
    pub y: f32,
    /// Rotation.
    pub rot: f32,
    /// Scale.
    pub scale: f32,
}

/// Velocity (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Velocity {
    /// X speed.
    pub x: f32,
    /// Y speed.
    pub y: f32,
}

/// What the sprite batcher needs (16 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprite {
    /// Texture id.
    pub texture: u32,
    /// Draw layer.
    pub layer: u32,
    /// Packed colour.
    pub color: u32,
    /// Padding, keeps the size at 16 bytes.
    pub pad: u32,
}

/// The physics body handle (8 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Body {
    /// Handle index.
    pub index: u32,
    /// Handle generation.
    pub generation: u32,
}

/// Disc radius (4 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radius(pub f32);

/// A marker added and removed often (a "was hit" flag, 4 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit(pub u32);

/// The bits of a component, for the digest that compares storages on every field.
pub trait Words {
    /// Four words that hold every field (zero padded).
    fn words(&self) -> [u32; 4];
}

impl Words for Transform {
    fn words(&self) -> [u32; 4] {
        [
            self.x.to_bits(),
            self.y.to_bits(),
            self.rot.to_bits(),
            self.scale.to_bits(),
        ]
    }
}

impl Words for Velocity {
    fn words(&self) -> [u32; 4] {
        [self.x.to_bits(), self.y.to_bits(), 0, 0]
    }
}

impl Words for Sprite {
    fn words(&self) -> [u32; 4] {
        [self.texture, self.layer, self.color, self.pad]
    }
}

impl Words for Body {
    fn words(&self) -> [u32; 4] {
        [self.index, self.generation, 0, 0]
    }
}

impl Words for Radius {
    fn words(&self) -> [u32; 4] {
        [self.0.to_bits(), 0, 0, 0]
    }
}

impl Words for Hit {
    fn words(&self) -> [u32; 4] {
        [self.0, 0, 0, 0]
    }
}

/// Contribution of one component of one entity slot to the digest; the digest is the wrapping
/// sum of these, so it does not depend on iteration order.
#[must_use]
pub fn digest_of(id: usize, index: u32, value: &impl Words) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325_u64 ^ (id as u64 + 1);
    hash = (hash ^ u64::from(index)).wrapping_mul(0x0100_0000_01B3);
    for word in value.words() {
        hash = (hash ^ u64::from(word)).wrapping_mul(0x0100_0000_01B3);
    }
    hash ^ (hash >> 32)
}

/// Component ids, used as indices and as bits of an archetype mask.
pub const TRANSFORM: usize = 0;
/// Id of [`Velocity`].
pub const VELOCITY: usize = 1;
/// Id of [`Sprite`].
pub const SPRITE: usize = 2;
/// Id of [`Body`].
pub const BODY: usize = 3;
/// Id of [`Radius`].
pub const RADIUS: usize = 4;
/// Id of [`Hit`].
pub const HIT: usize = 5;
/// Number of component types.
pub const COMPONENT_COUNT: usize = 6;

/// Archetype mask of a set of component ids.
#[must_use]
pub const fn bit(id: usize) -> u8 {
    1 << id
}

/// The kinds of entity of the sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Transform, velocity, sprite, body.
    Box,
    /// Box plus a radius.
    Disc,
    /// Transform, velocity, sprite (no body).
    Particle,
    /// Transform, sprite (static).
    Wall,
}

impl Kind {
    /// All kinds.
    pub const ALL: [Self; 4] = [Self::Box, Self::Disc, Self::Particle, Self::Wall];

    /// Mask of the components of the kind, without the [`Hit`] marker.
    #[must_use]
    pub const fn mask(self) -> u8 {
        let base = bit(TRANSFORM) | bit(SPRITE);
        match self {
            Self::Box => base | bit(VELOCITY) | bit(BODY),
            Self::Disc => base | bit(VELOCITY) | bit(BODY) | bit(RADIUS),
            Self::Particle => base | bit(VELOCITY),
            Self::Wall => base,
        }
    }

    /// The kind of the `serial`th entity: 40% boxes, 30% discs, 25% particles, 5% walls.
    #[must_use]
    pub const fn of_serial(serial: u32) -> Self {
        match serial % 20 {
            0..=7 => Self::Box,
            8..=13 => Self::Disc,
            14..=18 => Self::Particle,
            _ => Self::Wall,
        }
    }
}

/// Small integer in `0..modulus` derived from the serial and a salt.
#[must_use]
pub fn small(serial: u32, salt: u32, modulus: u16) -> f32 {
    let mixed = serial.wrapping_mul(2_654_435_761).wrapping_add(salt) >> 8;
    f32::from(u16::try_from(mixed % u32::from(modulus)).unwrap_or(0))
}

/// The transform of the `serial`th entity.
#[must_use]
pub fn transform_of(serial: u32) -> Transform {
    Transform {
        x: small(serial, 1, 100),
        y: small(serial, 2, 37),
        rot: 0.0,
        scale: 1.0,
    }
}

/// The velocity of the `serial`th entity (integers in -2..=2).
#[must_use]
pub fn velocity_of(serial: u32) -> Velocity {
    Velocity {
        x: small(serial, 3, 5) - 2.0,
        y: small(serial, 4, 5) - 2.0,
    }
}

/// The sprite of the `serial`th entity.
#[must_use]
pub const fn sprite_of(serial: u32) -> Sprite {
    Sprite {
        texture: serial % 8,
        layer: serial % 4,
        color: serial,
        pad: 0,
    }
}

/// The body handle of the `serial`th entity.
#[must_use]
pub const fn body_of(serial: u32) -> Body {
    Body {
        index: serial,
        generation: 1,
    }
}

/// The radius of the `serial`th entity.
#[must_use]
pub fn radius_of(serial: u32) -> Radius {
    Radius(small(serial, 5, 3) + 1.0)
}

/// Order-independent contribution of one entity to the extract checksum.
#[must_use]
pub fn mix(transform: &Transform, sprite: &Sprite, body: &Body) -> u64 {
    let x = u64::from(transform.x.to_bits());
    let value = x
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(sprite.texture) << 32)
        .wrapping_add(u64::from(body.index));
    value ^ (value >> 29)
}
