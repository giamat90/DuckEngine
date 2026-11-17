//! 3D shape descriptors: validity, bounds and mass properties.

use crate::core::{HashBits, MassProperties, PhysicsError, StateHasher};
use crate::d3::{Aabb3, D3};
use base::math::{Quat, Vec3};
use std::f32::consts::PI;

/// A convex shape in its body's local frame, centred on the body's origin.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Shape3 {
    /// A ball.
    Sphere {
        /// Radius in metres, positive and finite.
        radius: f32,
    },
    /// A box aligned with the local axes.
    Box {
        /// Half the size along each local axis, positive and finite.
        half_extents: Vec3,
    },
    /// A cylinder along the local Y axis with a hemisphere on each end.
    Capsule {
        /// Half the length of the cylinder part, positive and finite; the capsule is
        /// `half_height + radius` long on each side of the centre.
        half_height: f32,
        /// Radius of the cylinder and of the end caps, positive and finite.
        radius: f32,
    },
}

const TAG_SPHERE: u8 = 1;
const TAG_BOX: u8 = 2;
const TAG_CAPSULE: u8 = 3;

impl HashBits for Shape3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        match self {
            Self::Sphere { radius } => {
                hasher.write_u8(TAG_SPHERE);
                hasher.write_f32(*radius);
            }
            Self::Box { half_extents } => {
                hasher.write_u8(TAG_BOX);
                half_extents.write_bits(hasher);
            }
            Self::Capsule {
                half_height,
                radius,
            } => {
                hasher.write_u8(TAG_CAPSULE);
                hasher.write_f32(*half_height);
                hasher.write_f32(*radius);
            }
        }
    }
}

fn positive(size: f32) -> bool {
    size.is_finite() && size > 0.0
}

pub(super) fn is_valid(shape: &Shape3) -> bool {
    match *shape {
        Shape3::Sphere { radius } => positive(radius),
        Shape3::Box { half_extents } => {
            positive(half_extents.x) && positive(half_extents.y) && positive(half_extents.z)
        }
        Shape3::Capsule {
            half_height,
            radius,
        } => positive(half_height) && positive(radius),
    }
}

/// The reach of the shape from its centre along each world axis.
fn reach(shape: &Shape3, rot: Quat) -> Vec3 {
    match *shape {
        Shape3::Sphere { radius } => Vec3::new(radius, radius, radius),
        Shape3::Box { half_extents: h } => {
            let (x, y, z) = (
                rot.rotate(Vec3::X),
                rot.rotate(Vec3::Y),
                rot.rotate(Vec3::Z),
            );
            Vec3::new(
                x.x.abs() * h.x + y.x.abs() * h.y + z.x.abs() * h.z,
                x.y.abs() * h.x + y.y.abs() * h.y + z.y.abs() * h.z,
                x.z.abs() * h.x + y.z.abs() * h.y + z.z.abs() * h.z,
            )
        }
        Shape3::Capsule {
            half_height,
            radius,
        } => {
            let axis = rot.rotate(Vec3::Y).abs();
            axis * half_height + Vec3::new(radius, radius, radius)
        }
    }
}

pub(super) fn bounds(shape: &Shape3, position: Vec3, rot: Quat) -> Aabb3 {
    let r = reach(shape, rot);
    Aabb3::new(position - r, position + r)
}

/// Mass and the three principal moments of inertia (about the local axes) per unit density.
fn mass_and_moments_per_density(shape: &Shape3) -> (f32, Vec3) {
    match *shape {
        Shape3::Sphere { radius: r } => {
            let mass = 4.0 / 3.0 * PI * r * r * r;
            let moment = 0.4 * mass * r * r;
            (mass, Vec3::new(moment, moment, moment))
        }
        Shape3::Box { half_extents: h } => {
            let mass = 8.0 * h.x * h.y * h.z;
            let k = mass / 3.0;
            (
                mass,
                Vec3::new(
                    k * (h.y * h.y + h.z * h.z),
                    k * (h.x * h.x + h.z * h.z),
                    k * (h.x * h.x + h.y * h.y),
                ),
            )
        }
        Shape3::Capsule {
            half_height: h,
            radius: r,
        } => {
            let cylinder = PI * r * r * 2.0 * h;
            let ball = 4.0 / 3.0 * PI * r * r * r;
            let along = cylinder * r * r / 2.0 + ball * 0.4 * r * r;
            // The two hemispheres, together a sphere, sit at the ends: parallel-axis shift of each.
            let across = cylinder * (3.0 * r * r + 4.0 * h * h) / 12.0
                + ball * (0.4 * r * r + h * h + 0.75 * h * r);
            (cylinder + ball, Vec3::new(across, along, across))
        }
    }
}

/// Mass and inverse principal inertia of `shape` at `density` kilograms per cubic metre.
pub(super) fn mass_properties(
    shape: &Shape3,
    density: f32,
) -> Result<MassProperties<D3>, PhysicsError> {
    if !is_valid(shape) {
        return Err(PhysicsError::InvalidShape);
    }
    let (volume, moments) = mass_and_moments_per_density(shape);
    let (mass, scale) = (density * volume, density);
    let inv = Vec3::new(
        1.0 / (scale * moments.x),
        1.0 / (scale * moments.y),
        1.0 / (scale * moments.z),
    );
    // A density that is zero, negative, NaN or infinite, and a mass or moment that underflows or
    // overflows, all end in an inverse moment that is not a positive finite number; when all three
    // are, the mass is positive and finite too.
    if [inv.x, inv.y, inv.z].into_iter().all(positive) && positive(mass) {
        Ok(MassProperties {
            mass,
            inv_inertia: inv,
        })
    } else {
        Err(PhysicsError::InvalidShape)
    }
}
