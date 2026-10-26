//! Math types shared by physics, scene and rendering.
//!
//! Plain data and ordinary methods, no allocation, no `unsafe`. Conventions (f32, SI units,
//! right-handed Y-up, column-major) are in `wiki/architecture/conventions.md`.

mod matrix;
mod vector;

pub use matrix::{Mat3, Mat4};
pub use vector::{Vec2, Vec3, Vec4};

/// The scalar type of every math type. A single alias, so a later switch is mechanical.
pub type Scalar = f32;
