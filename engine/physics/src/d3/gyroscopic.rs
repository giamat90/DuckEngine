//! The gyroscopic term of a 3D body (card 8-26). A torque-free rigid body keeps its angular
//! *momentum* `I w`, so when the principal moments `I` differ its angular velocity `w` changes:
//! Euler's equation `I dw/dt + w x (I w) = 0` in the body frame.
//!
//! It is solved implicitly with one Newton step per call, as in the Bullet and `Box2D` v3 solvers:
//! `f(w) = I (w - w0) + h w x (I w)` at `w = w0` is `h w0 x (I w0)`, the Jacobian is
//! `I + h (skew(w0) I - skew(I w0))`, and the new velocity is `w0 - J^-1 f`. The explicit step
//! (`w0 - h I^-1 (w0 x I w0)`) gains energy and blows up for a fast spin about the middle axis; the
//! implicit one stays bounded at the cost of a little damping: first order in `h`, about 0.9 % of
//! the angular momentum a second at 240 Hz (card 8-26). Only plain arithmetic is used (no
//! `sin`, `cos` or `sqrt`, no fused multiply-add), so the result is the same on every platform.

use crate::core::Quantity;
use base::math::{Quat, Vec3};

/// The angular velocity after `h` seconds of the gyroscopic term, for a body at orientation `rot`
/// whose inverse principal moments (body axes) are `inv_inertia` and whose angular velocity (world
/// axes) is `spin`.
///
/// Returns `spin` unchanged when a moment is not above zero or not finite (a body that does not
/// turn on that axis has no momentum to exchange), or when the Newton step cannot be solved.
pub(crate) fn gyroscopic_spin(rot: Quat, inv_inertia: Vec3, spin: Vec3, h: f32) -> Vec3 {
    let smallest = inv_inertia.x.min(inv_inertia.y).min(inv_inertia.z);
    if !(smallest > 0.0 && inv_inertia.is_finite()) {
        return spin;
    }
    // The moments in units of the largest one: the Newton step does not depend on the scale.
    let moments = Vec3::new(
        smallest / inv_inertia.x,
        smallest / inv_inertia.y,
        smallest / inv_inertia.z,
    );
    let w0 = rot.conjugate().rotate(spin);
    let momentum = Vec3::new(moments.x * w0.x, moments.y * w0.y, moments.z * w0.z);
    let f = w0.cross(momentum) * h;
    let column = |axis: Vec3, moment: f32| {
        let along = axis * moment;
        along + (w0.cross(along) - momentum.cross(axis)) * h
    };
    let c0 = column(Vec3::X, moments.x);
    let c1 = column(Vec3::Y, moments.y);
    let c2 = column(Vec3::Z, moments.z);
    let det = c0.dot(c1.cross(c2));
    // Relative to the size of the columns: a long thin body has a determinant far below one.
    let scale = c0.length() * c1.length() * c2.length();
    if det.is_nan() || det.abs() <= f32::EPSILON * scale {
        return spin;
    }
    // Cramer's rule for J d = f.
    let step = Vec3::new(
        f.dot(c1.cross(c2)),
        c0.dot(f.cross(c2)),
        c0.dot(c1.cross(f)),
    ) * (1.0 / det);
    let turned = rot.rotate(w0 - step);
    if turned.is_finite() { turned } else { spin }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f32 = 1.0e-3;

    fn near(a: Vec3, b: Vec3, tolerance: f32) -> bool {
        (a - b).length() <= tolerance
    }

    #[test]
    fn a_small_step_follows_eulers_equation() {
        // I = (1, 2, 3), w = (1, 1, 1): w x I w = (1, -2, 1), so dw = -h I^-1 (1, -2, 1)
        // = h (-1, 1, -1/3) to first order.
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        let out = gyroscopic_spin(Quat::IDENTITY, inv, Vec3::new(1.0, 1.0, 1.0), H);
        let expected = Vec3::new(1.0 - H, 1.0 + H, 1.0 - H / 3.0);
        assert!(near(out, expected, 1.0e-5), "{out:?} against {expected:?}");
    }

    #[test]
    fn a_spin_about_a_principal_axis_is_kept() {
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let spin = axis * 7.0;
            assert!(near(
                gyroscopic_spin(Quat::IDENTITY, inv, spin, 0.01),
                spin,
                1.0e-5
            ));
        }
    }

    #[test]
    fn equal_moments_keep_the_spin_at_any_orientation() {
        let rot = Quat::from_axis_angle(
            Vec3::new(1.0, 2.0, 3.0).normalized().unwrap_or(Vec3::X),
            1.1,
        )
        .unwrap_or(Quat::IDENTITY);
        let inv = Vec3::new(2.0, 2.0, 2.0);
        let spin = Vec3::new(1.0, -4.0, 0.5);
        assert!(near(gyroscopic_spin(rot, inv, spin, 0.01), spin, 1.0e-5));
    }

    #[test]
    fn the_step_is_done_in_the_body_frame() {
        // The same tumble, turned by a quarter about z, gives the same answer turned the same way.
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        let quarter =
            Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let local = Vec3::new(1.0, 2.0, 3.0);
        let plain = gyroscopic_spin(Quat::IDENTITY, inv, local, 0.01);
        let turned = gyroscopic_spin(quarter, inv, quarter.rotate(local), 0.01);
        assert!(near(turned, quarter.rotate(plain), 1.0e-5), "{turned:?}");
    }

    #[test]
    fn a_step_keeps_the_momentum_to_second_order_in_h() {
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        let spin = Vec3::new(3.0, 0.2, 0.1);
        let momentum = |w: Vec3| Vec3::new(w.x / inv.x, w.y / inv.y, w.z / inv.z).length();
        let out = gyroscopic_spin(Quat::IDENTITY, inv, spin, 1.0 / 240.0);
        assert!((momentum(out) - momentum(spin)).abs() / momentum(spin) < 1.0e-4);
    }

    #[test]
    fn a_long_thin_body_still_gets_the_term() {
        // A rod of half extents (100, 0.01, 0.01): about x it is 1e-8 of the other moments. The
        // step is tiny on purpose: at 1/240 s the `h` terms keep the determinant above the old
        // absolute limit, at 1e-5 s they do not (red against `det.abs() <= EPSILON`).
        let inv = Vec3::new(1.0e8, 1.0, 2.0);
        let spin = Vec3::new(0.1, 3.0, 2.0);
        let out = gyroscopic_spin(Quat::IDENTITY, inv, spin, 1.0e-5);
        assert!(out != spin, "the term was skipped for a thin body");
        assert!(out.is_finite());
    }

    #[test]
    fn a_huge_step_gives_a_finite_spin() {
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        let out = gyroscopic_spin(Quat::IDENTITY, inv, Vec3::new(50.0, 40.0, 30.0), 10.0);
        assert!(out.is_finite(), "{out:?}");
    }

    #[test]
    fn a_moment_that_is_not_above_zero_or_not_finite_leaves_the_spin() {
        let spin = Vec3::new(1.0, 2.0, 3.0);
        for inv in [
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(1.0, -1.0, 1.0),
            Vec3::new(1.0, 1.0, f32::NAN),
            Vec3::new(f32::INFINITY, 1.0, 2.0),
        ] {
            assert_eq!(gyroscopic_spin(Quat::IDENTITY, inv, spin, 0.01), spin);
        }
    }
}
