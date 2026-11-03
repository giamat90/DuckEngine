//! 3D helpers used by variant A and by the 3D instance of variant B: the planar projection for
//! the extruded narrowphase, the quaternion step, the inertia product and the friction basis.

use crate::narrow2d::{Manifold, PlanarBox, collide};
use crate::scene::DEPTH_HALF;
use base::math::{Quat, Vec2, Vec3};

/// Box mass for density 1 and the depth `2 * DEPTH_HALF`, and the diagonal inverse inertia in the
/// body frame (`x` and `y` zero: the locked rotation axes).
#[inline]
#[must_use]
pub fn mass_properties(half_x: f32, half_y: f32) -> (f32, Vec3) {
    let mass = 8.0 * half_x * half_y * DEPTH_HALF;
    let izz = mass * (half_x * half_x + half_y * half_y) / 3.0;
    (1.0 / mass, Vec3::new(0.0, 0.0, 1.0 / izz))
}

/// `R * diag(inv) * R^T * t` for the unit quaternion `rot`.
#[inline]
#[must_use]
pub fn apply_inverse_inertia(rot: Quat, inv: Vec3, t: Vec3) -> Vec3 {
    let local = rot.conjugate().rotate(t);
    rot.rotate(Vec3::new(inv.x * local.x, inv.y * local.y, inv.z * local.z))
}

/// First-order step of the rotation by the angular velocity `w` over `dt`, renormalised.
#[inline]
#[must_use]
pub fn advance(rot: Quat, w: Vec3, dt: f32) -> Quat {
    let h = 0.5 * dt;
    let dq = Quat::new(w.x * h, w.y * h, w.z * h, 0.0) * rot;
    Quat::new(rot.x + dq.x, rot.y + dq.y, rot.z + dq.z, rot.w + dq.w)
        .normalized()
        .unwrap_or(rot)
}

/// The planar box of a body whose only rotation is about Z: `cos` and `sin` of the full angle
/// come from the quaternion without trigonometry.
#[inline]
#[must_use]
pub fn planar_box(pos: Vec3, rot: Quat, half: Vec3) -> PlanarBox {
    PlanarBox {
        pos: Vec2::new(pos.x, pos.y),
        cos: rot.w * rot.w - rot.z * rot.z,
        sin: 2.0 * rot.w * rot.z,
        half: Vec2::new(half.x, half.y),
    }
}

/// Collides the extruded boxes and lifts each planar point to the two ends of the depth, as the
/// face contact of two real 3D boxes would give four points. Returns the normal.
#[inline]
pub fn collide_extruded(
    a: PlanarBox,
    b: PlanarBox,
    z: f32,
    mut emit: impl FnMut(Vec3, f32),
) -> Option<Vec3> {
    let Manifold {
        normal,
        count,
        points,
    } = collide(a, b)?;
    for p in points.iter().take(count) {
        for side in [-DEPTH_HALF, DEPTH_HALF] {
            emit(
                Vec3::new(p.position.x, p.position.y, z + side),
                p.separation,
            );
        }
    }
    Some(Vec3::new(normal.x, normal.y, 0.0))
}

const TANGENT_SWITCH: f32 = 0.577_35;

/// Two unit vectors that complete `n` to an orthonormal basis.
#[inline]
#[must_use]
pub fn tangents(n: Vec3) -> [Vec3; 2] {
    let first = if n.x.abs() >= TANGENT_SWITCH {
        Vec3::new(n.y, -n.x, 0.0)
    } else {
        Vec3::new(0.0, n.z, -n.y)
    };
    let t1 = first.normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    [t1, n.cross(t1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tangents_are_orthonormal_to_the_normal() {
        let n = Vec3::new(0.0, 1.0, 0.0);
        let [t1, t2] = tangents(n);
        assert!(n.dot(t1).abs() < 1e-6 && n.dot(t2).abs() < 1e-6 && t1.dot(t2).abs() < 1e-6);
        assert!((t1.length() - 1.0).abs() < 1e-6 && (t2.length() - 1.0).abs() < 1e-6);
    }
}
