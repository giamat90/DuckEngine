//! Local and world transforms of the 2D scene.

use base::math::{Mat3, Mat4, Quat, Vec2, Vec3};

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

/// Where an entity sits relative to its parent (or to the world, without a parent) in 3D: a
/// translation, a rotation and a scale per axis, applied scale first, then rotation, then
/// translation. Right-handed, Y up. The rotation must be a unit quaternion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3D {
    /// Translation, in metres.
    pub position: Vec3,
    /// Orientation, a unit quaternion.
    pub rotation: Quat,
    /// Scale along the local axes.
    pub scale: Vec3,
}

impl Transform3D {
    /// No translation, no rotation, scale one.
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Only a translation.
    #[must_use]
    pub const fn from_position(position: Vec3) -> Self {
        Self {
            position,
            ..Self::IDENTITY
        }
    }

    /// The matrix of this transform: scale, then rotation, then translation.
    #[must_use]
    pub fn to_mat4(self) -> Mat4 {
        let rotation = self.rotation.to_mat3();
        Mat4::from_cols(
            (rotation.cols[0] * self.scale.x).extend(0.0),
            (rotation.cols[1] * self.scale.y).extend(0.0),
            (rotation.cols[2] * self.scale.z).extend(0.0),
            self.position.extend(1.0),
        )
    }
}

impl Default for Transform3D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The derived transform of an entity in world space (3D), written by the update from the local
/// transforms along the hierarchy. Nothing else should write it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldTransform3D {
    /// The matrix that takes a point of the entity's local space to world space.
    pub matrix: Mat4,
}

impl WorldTransform3D {
    /// Takes a point of the entity's local space to world space.
    #[must_use]
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        self.matrix.transform_point3(point)
    }
}

impl Default for WorldTransform3D {
    fn default() -> Self {
        Self {
            matrix: Mat4::IDENTITY,
        }
    }
}
