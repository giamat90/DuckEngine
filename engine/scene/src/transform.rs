//! Local and world transforms of the 2D scene.

use base::math::{Mat3, Vec2};

/// Where an entity sits relative to its parent (or to the world, without a parent): a translation,
/// a counter-clockwise rotation in radians and a scale per axis, applied scale first, then
/// rotation, then translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    /// Translation along x and y, in metres.
    pub position: Vec2,
    /// Counter-clockwise rotation in radians.
    pub rotation: f32,
    /// Scale along the local x and y axes.
    pub scale: Vec2,
}

impl Transform2D {
    /// No translation, no rotation, scale one.
    pub const IDENTITY: Self = Self {
        position: Vec2::ZERO,
        rotation: 0.0,
        scale: Vec2::ONE,
    };

    /// Only a translation.
    #[must_use]
    pub const fn from_position(position: Vec2) -> Self {
        Self {
            position,
            ..Self::IDENTITY
        }
    }

    /// The matrix of this transform: scale, then rotation, then translation.
    #[must_use]
    pub fn to_mat3(self) -> Mat3 {
        let rotation = Mat3::from_rotation_z(self.rotation);
        Mat3::from_cols(
            rotation.cols[0] * self.scale.x,
            rotation.cols[1] * self.scale.y,
            self.position.extend(1.0),
        )
    }
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The derived transform of an entity in world space, written by the update from the local
/// transforms along the hierarchy. Nothing else should write it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldTransform2D {
    /// The matrix that takes a point of the entity's local space to world space.
    pub matrix: Mat3,
}

impl WorldTransform2D {
    /// Takes a point of the entity's local space to world space.
    #[must_use]
    pub fn transform_point(&self, point: Vec2) -> Vec2 {
        (self.matrix * point.extend(1.0)).truncate()
    }
}

impl Default for WorldTransform2D {
    fn default() -> Self {
        Self {
            matrix: Mat3::IDENTITY,
        }
    }
}
