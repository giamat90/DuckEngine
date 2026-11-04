//! 2D shape descriptors and their mass properties.

use crate::core::{HashBits, MassProperties, PhysicsError, StateHasher};
use crate::d2::{Aabb2, D2, Rot2};
use base::math::Vec2;
use std::f32::consts::PI;

/// A 2D shape as plain data, centred on the body's centre of mass. Polygons come with the
/// narrowphase (card 4-12).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Shape2 {
    /// A disc.
    Circle {
        /// Radius in metres, above zero.
        radius: f32,
    },
    /// An axis-aligned box in the body frame.
    Box {
        /// Half the width and half the height in metres, both above zero.
        half_extents: Vec2,
    },
}

/// Tag byte written before the fields of a shape, so a circle and a box never hash alike.
const TAG_CIRCLE: u8 = 1;
const TAG_BOX: u8 = 2;

impl HashBits for Shape2 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        match self {
            Self::Circle { radius } => {
                hasher.write_u8(TAG_CIRCLE);
                hasher.write_f32(*radius);
            }
            Self::Box { half_extents } => {
                hasher.write_u8(TAG_BOX);
                hasher.write_f32(half_extents.x);
                hasher.write_f32(half_extents.y);
            }
        }
    }
}

/// Area in square metres and moment of inertia per unit mass (about the centre) of a valid shape.
fn area_and_inertia_per_mass(shape: &Shape2) -> (f32, f32) {
    match *shape {
        Shape2::Circle { radius } => (PI * radius * radius, 0.5 * radius * radius),
        Shape2::Box { half_extents } => (
            4.0 * half_extents.x * half_extents.y,
            (half_extents.x * half_extents.x + half_extents.y * half_extents.y) / 3.0,
        ),
    }
}

/// True when `size` is finite and above zero.
fn positive(size: f32) -> bool {
    size.is_finite() && size > 0.0
}

/// True when every size of the shape is finite and above zero. Sizes are checked here and not left
/// to the mass check, because two negative half extents give a positive area.
fn is_valid(shape: &Shape2) -> bool {
    match *shape {
        Shape2::Circle { radius } => positive(radius),
        Shape2::Box { half_extents } => [half_extents.x, half_extents.y].into_iter().all(positive),
    }
}

/// The tightest axis-aligned box around `shape` at `position`, turned by `rot`. A circle is as wide
/// as its diameter at any angle; a box of half extents `(hx, hy)` turned by `(c, s)` reaches
/// `|c| hx + |s| hy` along x and `|s| hx + |c| hy` along y.
pub(super) fn bounds(shape: &Shape2, position: Vec2, rot: Rot2) -> Aabb2 {
    let reach = match *shape {
        Shape2::Circle { radius } => Vec2::new(radius, radius),
        Shape2::Box { half_extents } => {
            let (cos, sin) = rot.direction();
            let (cos, sin) = (cos.abs(), sin.abs());
            Vec2::new(
                cos * half_extents.x + sin * half_extents.y,
                sin * half_extents.x + cos * half_extents.y,
            )
        }
    };
    Aabb2::new(position - reach, position + reach)
}

/// Mass and inverse inertia of `shape` at `density` kilograms per square metre.
pub(super) fn mass_properties(
    shape: &Shape2,
    density: f32,
) -> Result<MassProperties<D2>, PhysicsError> {
    if !is_valid(shape) {
        return Err(PhysicsError::InvalidShape);
    }
    let (area, inertia_per_mass) = area_and_inertia_per_mass(shape);
    let mass = density * area;
    let inv_inertia = 1.0 / (mass * inertia_per_mass);
    // For a valid shape the inertia per mass is positive, so a density that is zero, negative, NaN
    // or infinite, and a mass or inertia that underflows or overflows, all end in an inverse inertia
    // that is not a positive finite number; when it is one, the mass is positive and finite too.
    if inv_inertia.is_finite() && inv_inertia > 0.0 {
        Ok(MassProperties { mass, inv_inertia })
    } else {
        Err(PhysicsError::InvalidShape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-6 * b.abs().max(1.0)
    }

    fn props(shape: Shape2, density: f32) -> Option<(f32, f32)> {
        mass_properties(&shape, density)
            .ok()
            .map(|p| (p.mass, p.inv_inertia))
    }

    #[test]
    fn a_circle_has_the_closed_form_mass_and_inertia() {
        // m = rho * pi * r^2, I = m * r^2 / 2.
        let radius = 0.5_f32;
        let mass = 2.0 * PI * radius * radius;
        let inertia = 0.5 * mass * radius * radius;
        let Some((got_mass, got_inv)) = props(Shape2::Circle { radius }, 2.0) else {
            panic!("a valid circle must have mass properties");
        };
        assert!(close(got_mass, mass), "mass {got_mass} vs {mass}");
        assert!(close(got_inv, 1.0 / inertia), "inv {got_inv}");
    }

    #[test]
    fn a_box_has_the_closed_form_mass_and_inertia() {
        // Half extents (2, 0.5), density 3: width 4, height 1, m = 3 * 4 * 1 = 12 and
        // I = m * (w^2 + h^2) / 12 = 12 * 17 / 12 = 17.
        let shape = Shape2::Box {
            half_extents: Vec2::new(2.0, 0.5),
        };
        let Some((mass, inv)) = props(shape, 3.0) else {
            panic!("a valid box must have mass properties");
        };
        assert!(close(mass, 12.0), "mass {mass}");
        assert!(close(inv, 1.0 / 17.0), "inv {inv}");
        // Half extents (0.5, 2) give the same mass and inertia: the formula is symmetric.
        let turned = Shape2::Box {
            half_extents: Vec2::new(0.5, 2.0),
        };
        assert_eq!(props(turned, 3.0), props(shape, 3.0));
    }

    #[test]
    fn invalid_shapes_and_densities_are_refused() {
        let bad_shapes = [
            Shape2::Circle { radius: 0.0 },
            Shape2::Circle { radius: -1.0 },
            Shape2::Circle { radius: f32::NAN },
            Shape2::Circle {
                radius: f32::INFINITY,
            },
            Shape2::Box {
                half_extents: Vec2::new(0.0, 1.0),
            },
            Shape2::Box {
                half_extents: Vec2::new(1.0, 0.0),
            },
            Shape2::Box {
                half_extents: Vec2::new(f32::NAN, 1.0),
            },
            Shape2::Box {
                half_extents: Vec2::new(1.0, f32::INFINITY),
            },
            Shape2::Box {
                half_extents: Vec2::new(-1.0, -1.0),
            },
        ];
        for shape in bad_shapes {
            assert_eq!(
                mass_properties(&shape, 1.0).err(),
                Some(PhysicsError::InvalidShape),
                "{shape:?}"
            );
        }
        let unit = Shape2::Circle { radius: 1.0 };
        for density in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                mass_properties(&unit, density).err(),
                Some(PhysicsError::InvalidShape),
                "density {density}"
            );
        }
    }

    #[test]
    fn mass_that_underflows_or_overflows_is_refused() {
        let tiny = Shape2::Circle { radius: 1e-30 };
        assert_eq!(
            mass_properties(&tiny, 1.0).err(),
            Some(PhysicsError::InvalidShape)
        );
        let huge = Shape2::Box {
            half_extents: Vec2::new(1e30, 1e30),
        };
        assert_eq!(
            mass_properties(&huge, 1.0).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    #[test]
    fn a_circle_and_a_box_hash_differently_even_with_equal_numbers() {
        let mut circle = StateHasher::new();
        Shape2::Circle { radius: 1.0 }.write_bits(&mut circle);
        let mut boxed = StateHasher::new();
        Shape2::Box {
            half_extents: Vec2::new(1.0, 1.0),
        }
        .write_bits(&mut boxed);
        assert_ne!(circle.finish(), boxed.finish());
    }

    #[test]
    fn a_size_must_be_finite_and_above_zero() {
        assert!(positive(f32::MIN_POSITIVE));
        assert!(positive(1.0));
        assert!(!positive(0.0));
        assert!(!positive(-0.0));
        assert!(!positive(-1.0));
        assert!(!positive(f32::INFINITY));
        assert!(!positive(f32::NEG_INFINITY));
        assert!(!positive(f32::NAN));
    }

    #[test]
    fn a_box_needs_both_sizes_valid() {
        for (x, y) in [
            (0.0, 1.0),
            (1.0, 0.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (-1.0, -1.0),
        ] {
            let shape = Shape2::Box {
                half_extents: Vec2::new(x, y),
            };
            assert!(!is_valid(&shape), "({x}, {y})");
        }
    }
}
