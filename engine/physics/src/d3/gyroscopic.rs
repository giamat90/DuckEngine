//! The gyroscopic term of a 3D body (cards 8-26 and 8-73). A torque-free rigid body keeps its
//! angular *momentum* `I w`, so when the principal moments `I` differ its angular velocity `w`
//! changes: Euler's equation `I dw/dt + w x (I w) = 0` in the body frame.
//!
//! It is solved with the implicit midpoint rule: `f(w1) = I (w1 - w0) + h wm x (I wm) = 0` with
//! `wm = (w0 + w1) / 2`, by Newton's method (two iterations from `w0`, the Jacobian is
//! `I + h/2 (skew(wm) I - skew(I wm))`). The explicit step (`w0 - h I^-1 (w0 x I w0)`) gains energy
//! and blows up for a fast spin about the middle axis. Backward Euler (the first version, card
//! 8-26) stays bounded but loses about 0.9 % of the angular momentum a second at 240 Hz. The
//! midpoint rule is second order and keeps the two quantities that are quadratic in `w` (the
//! length of the momentum and the energy) up to the error of the Newton iterations; measured to
//! rounding up to a spin times a step of 50 radians and refused (the old spin is kept) if the energy
//! would rise by more than 1 %. Only plain arithmetic is used (`+ - * /`, no `sin`, `cos` or
//! `sqrt`, no fused multiply-add), so the result is the same on every platform.

use crate::core::Quantity;
use base::math::{Quat, Vec3};

/// Newton iterations of the midpoint equation, starting from the old velocity. The error of the
/// start is of the order of `h` and each iteration squares it: two give errors below `f32` rounding
/// for the steps of a simulation (1/240 s and 1/60 s; measured, card 8-73), and a guard refuses
/// a result that gained energy.
const NEWTON_ITERATIONS: usize = 2;

/// The most the energy may rise in one step before the result is refused (1 %; the steps of a
/// simulation change it by rounding, 1e-6).
const ENERGY_GAIN_LIMIT: f32 = 1.01;

/// The angular velocity after `h` seconds of the gyroscopic term, for a body at orientation `rot`
/// whose inverse principal moments (body axes) are `inv_inertia` and whose angular velocity (world
/// axes) is `spin`.
///
/// Returns `spin` unchanged when a moment is not above zero or not finite (a body that does not
/// turn on that axis has no momentum to exchange), or when a Newton step cannot be solved.
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
    let mut w1 = w0;
    for _ in 0..NEWTON_ITERATIONS {
        let Some(step) = newton_step(moments, w0, w1, h) else {
            return spin;
        };
        w1 -= step;
    }
    // The midpoint rule keeps the energy; Newton from `w0` does not converge for a step that turns
    // the body by many radians (a spin times a step of 100 or more), and the result then may have
    // gained energy. A gain above `ENERGY_GAIN_LIMIT` keeps the old spin.
    let energy = |w: Vec3| moments.x * w.x * w.x + moments.y * w.y * w.y + moments.z * w.z * w.z;
    let turned = rot.rotate(w1);
    if turned.is_finite() && energy(w1) <= energy(w0) * ENERGY_GAIN_LIMIT {
        turned
    } else {
        spin
    }
}

/// `J^-1 f` of the midpoint equation at the guess `w1`, or `None` when `J` is singular.
fn newton_step(moments: Vec3, w0: Vec3, w1: Vec3, h: f32) -> Option<Vec3> {
    let times_moments = |w: Vec3| Vec3::new(moments.x * w.x, moments.y * w.y, moments.z * w.z);
    let wm = (w0 + w1) * 0.5;
    let momentum = times_moments(wm);
    let f = times_moments(w1 - w0) + wm.cross(momentum) * h;
    let half = h * 0.5;
    let column = |axis: Vec3, moment: f32| {
        let along = axis * moment;
        along + (wm.cross(along) - momentum.cross(axis)) * half
    };
    let c0 = column(Vec3::X, moments.x);
    let c1 = column(Vec3::Y, moments.y);
    let c2 = column(Vec3::Z, moments.z);
    let det = c0.dot(c1.cross(c2));
    // Relative to the size of the columns (compared squared, no `sqrt`): a long thin body has a
    // determinant far below one.
    let scale = c0.length_squared() * c1.length_squared() * c2.length_squared();
    let solvable = det * det > f32::EPSILON * f32::EPSILON * scale;
    if !solvable {
        return None;
    }
    // Cramer's rule for J d = f.
    Some(
        Vec3::new(
            f.dot(c1.cross(c2)),
            c0.dot(f.cross(c2)),
            c0.dot(c1.cross(f)),
        ) * (1.0 / det),
    )
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

    fn kinetic_energy(inv: Vec3, w: Vec3) -> f32 {
        0.5 * (w.x * w.x / inv.x + w.y * w.y / inv.y + w.z * w.z / inv.z)
    }

    /// Card 8-73: the length of the angular momentum and the energy are constants of the
    /// torque-free motion, so one step must not change them (backward Euler lost the first by about
    /// 1e-5 a step at 60 Hz; red against it).
    #[test]
    fn a_step_keeps_the_length_of_the_momentum_and_the_energy_to_rounding() {
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        let momentum = |w: Vec3| Vec3::new(w.x / inv.x, w.y / inv.y, w.z / inv.z).length();
        let spins = [
            Vec3::new(3.0, 0.2, 0.1),
            Vec3::new(1.0, 4.0, 0.5),
            Vec3::new(6.0, 0.05, 0.05),
            Vec3::new(-2.0, 3.0, 5.0),
        ];
        let cases = spins
            .into_iter()
            .flat_map(|spin| [1.0 / 960.0, 1.0 / 240.0, 1.0 / 60.0].map(|h| (spin, h)));
        for (spin, h) in cases {
            let out = gyroscopic_spin(Quat::IDENTITY, inv, spin, h);
            assert!(out != spin, "the term did nothing for {spin:?}");
            let length = (momentum(out) - momentum(spin)).abs() / momentum(spin);
            let energy = (kinetic_energy(inv, out) - kinetic_energy(inv, spin)).abs()
                / kinetic_energy(inv, spin);
            assert!(
                length < 2.0e-6,
                "{spin:?} at {h}: momentum changed by {length}"
            );
            assert!(
                energy < 2.0e-6,
                "{spin:?} at {h}: energy changed by {energy}"
            );
        }
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

    /// Card 8-73: fast spins and long steps. Up to a spin times a step of 50 radians the step keeps
    /// both quantities (measured to 1e-5); beyond that Newton from the old spin does not converge, and
    /// the result may lose energy (measured down to 18 % of it) but must not gain more than 1 %.
    #[test]
    fn a_fast_spin_or_a_long_step_keeps_the_quantities_or_loses_them_never_gains() {
        let inv = Vec3::new(1.0, 0.5, 1.0 / 3.0);
        let momentum = |w: Vec3| Vec3::new(w.x / inv.x, w.y / inv.y, w.z / inv.z).length();
        let spins = [
            Vec3::new(6.0, 0.05, 0.05),
            Vec3::new(50.0, 0.5, 0.5),
            Vec3::new(30.0, 40.0, 20.0),
            Vec3::new(200.0, 3.0, 2.0),
        ];
        let cases = spins
            .into_iter()
            .flat_map(|spin| [1.0 / 60.0, 1.0 / 30.0, 0.1, 1.0, 10.0].map(|h| (spin, h)));
        for (spin, h) in cases {
            let out = gyroscopic_spin(Quat::IDENTITY, inv, spin, h);
            let (e0, e1) = (kinetic_energy(inv, spin), kinetic_energy(inv, out));
            assert!(e1 <= e0 * 1.0101, "{spin:?} at {h}: energy x{}", e1 / e0);
            assert!(
                momentum(out) <= momentum(spin) * 1.0101,
                "{spin:?} at {h}: momentum x{}",
                momentum(out) / momentum(spin)
            );
            let converged = spin.length() * h < 50.0;
            assert!(
                !converged || (e1 / e0 - 1.0).abs() < 1.0e-4,
                "{spin:?} at {h}: energy x{}",
                e1 / e0
            );
        }
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
