//! What the sample makes: the box the pieces fall into, and the bodies of the pieces. Pure
//! functions over a seeded generator, so a run is the same everywhere.

use crate::controls::Piece;
use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use physics::d3::{BodyDesc, BodyKind, Shape3};

/// Half the inner width and depth of the box, metres.
pub const ARENA_HALF: f32 = 6.0;
/// Half the thickness of the floor and the walls, metres.
const WALL_HALF: f32 = 0.5;
/// Half the height of a wall, metres.
const WALL_HEIGHT_HALF: f32 = 2.0;
/// Height pieces are dropped from, metres.
pub const DROP_HEIGHT: f32 = 6.0;
/// Distance a drop keeps from the walls, metres.
pub const MARGIN: f32 = 1.5;
/// Smallest half side of a box and smallest radius of a sphere, metres.
pub const MIN_SIZE: f32 = 0.3;
/// Most a size adds to [`MIN_SIZE`], metres.
pub const EXTRA_SIZE: f32 = 0.25;
/// Radius of the capsule mesh the scale is applied to, metres.
pub const CAPSULE_RADIUS: f32 = 0.5;
/// Half the cylinder length of the capsule mesh, metres.
pub const CAPSULE_HALF_HEIGHT: f32 = 0.5;
/// The random stream the sample draws from.
pub const RNG_STREAM: u64 = 811;

/// One fixed block of the box: its centre and half extents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    /// Centre, metres.
    pub position: Vec3,
    /// Half the size along each axis, metres.
    pub half: Vec3,
}

/// The floor and the four walls, floor first. The floor's top is at `y = 0`.
#[must_use]
pub fn scenery() -> [Block; 5] {
    let reach = ARENA_HALF + WALL_HALF;
    let side = |x: f32| Block {
        position: Vec3::new(x, WALL_HEIGHT_HALF, 0.0),
        half: Vec3::new(WALL_HALF, WALL_HEIGHT_HALF, reach),
    };
    let end = |z: f32| Block {
        position: Vec3::new(0.0, WALL_HEIGHT_HALF, z),
        half: Vec3::new(reach, WALL_HEIGHT_HALF, WALL_HALF),
    };
    [
        Block {
            position: Vec3::new(0.0, -WALL_HALF, 0.0),
            half: Vec3::new(reach, WALL_HALF, reach),
        },
        side(-reach),
        side(reach),
        end(-reach),
        end(reach),
    ]
}

/// A body to create and the scale of the mesh that draws it.
#[derive(Clone, Copy)]
pub struct Made {
    /// The body.
    pub desc: BodyDesc,
    /// The scale of the unit mesh of the piece (see [`crate::mesh`]).
    pub scale: Vec3,
}

/// A random size, from [`MIN_SIZE`] up to [`MIN_SIZE`] plus [`EXTRA_SIZE`].
fn size(rng: &mut Pcg32) -> f32 {
    MIN_SIZE + EXTRA_SIZE * rng.next_f32()
}

/// A random orientation: a turn about a random tilted axis.
fn tilt(rng: &mut Pcg32) -> Quat {
    let axis = Vec3::new(
        rng.next_f32() - 0.5,
        rng.next_f32() + 0.1,
        rng.next_f32() - 0.5,
    );
    let angle = rng.next_f32() * std::f32::consts::PI;
    axis.normalized()
        .and_then(|axis| Quat::from_axis_angle(axis, angle))
        .unwrap_or(Quat::IDENTITY)
}

/// A piece of the given kind at `at`, of a random size and tilt.
#[must_use]
pub fn piece(rng: &mut Pcg32, kind: Piece, at: Vec3) -> Made {
    let (shape, scale) = match kind {
        Piece::Sphere => {
            let radius = size(rng);
            (Shape3::Sphere { radius }, Vec3::splat(radius))
        }
        Piece::Box => {
            let half_extents = Vec3::new(size(rng), size(rng), size(rng));
            (Shape3::Box { half_extents }, half_extents)
        }
        Piece::Capsule => {
            let scale = size(rng) * 2.0;
            (
                Shape3::Capsule {
                    half_height: CAPSULE_HALF_HEIGHT * scale,
                    radius: CAPSULE_RADIUS * scale,
                },
                Vec3::splat(scale),
            )
        }
    };
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
    desc.rotation = tilt(rng);
    Made { desc, scale }
}

/// A random point at [`DROP_HEIGHT`] over the floor, [`MARGIN`] away from the walls.
#[must_use]
pub fn drop_point(rng: &mut Pcg32) -> Vec3 {
    let reach = ARENA_HALF - MARGIN;
    let mut along = || (rng.next_f32() * 2.0 - 1.0) * reach;
    let x = along();
    let z = along();
    Vec3::new(x, DROP_HEIGHT, z)
}

/// The piece the stream drops next: any of the three, evenly.
#[must_use]
pub fn stream_piece(rng: &mut Pcg32) -> Piece {
    match rng.next_u32() % 3 {
        0 => Piece::Sphere,
        1 => Piece::Box,
        _ => Piece::Capsule,
    }
}

#[cfg(test)]
mod tests;
