//! The soft constraint of a contact, shared by the contact solvers of every dimension: the spring
//! coefficients ([`Softness`]), how one sweep reads them ([`Sweep`]) and two small helpers of the
//! impulse maths. Plain `f32` operations in a fixed order: no fused multiply-add, no `sin`, `cos`.

use std::f32::consts::TAU;

/// The cap on the stiffness of a contact as a fraction of the sub-step rate: above a quarter of the
/// rate the soft constraint is no longer stable.
pub(crate) const HERTZ_LIMIT_FRACTION: f32 = 0.25;

/// A contact against a static body is this many times stiffer than one between two dynamic bodies
/// (the static body cannot give way).
pub(crate) const STATIC_STIFFNESS_FACTOR: f32 = 2.0;

/// The coefficients of a soft constraint for one sub-step length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Softness {
    /// Velocity per metre of error that the constraint asks for.
    pub bias_rate: f32,
    /// How much of the impulse that would remove the error at once is applied.
    pub mass_scale: f32,
    /// How much of the impulse accumulated so far is given back.
    pub impulse_scale: f32,
}

impl Softness {
    /// A spring of `hertz` and `damping_ratio` integrated implicitly over a step of `h` seconds.
    pub(crate) fn new(hertz: f32, damping_ratio: f32, h: f32) -> Self {
        let omega = TAU * hertz;
        let a1 = 2.0 * damping_ratio + h * omega;
        let a2 = h * omega * a1;
        let a3 = 1.0 / (1.0 + a2);
        Self {
            bias_rate: omega / a1,
            mass_scale: a2 * a3,
            impulse_scale: a3,
        }
    }
}

/// How a sweep over the constraints treats the soft constraint.
#[derive(Clone, Copy)]
pub(crate) struct Sweep {
    /// One over the sub-step length.
    pub inv_h: f32,
    /// True for the solve, false for the relax pass.
    pub use_bias: bool,
    pub push_speed: f32,
    pub slop: f32,
    pub restitution_threshold: f32,
}

impl Sweep {
    /// The velocity bias and the two scales for a point at `separation`.
    pub(crate) fn terms(&self, separation: f32, softness: &Softness) -> (f32, f32, f32) {
        if separation > 0.0 {
            // Speculative: the bodies may close the gap within one sub-step and no more.
            (separation * self.inv_h, 1.0, 0.0)
        } else if self.use_bias {
            let pull = softness.bias_rate * (separation + self.slop).min(0.0);
            (
                pull.max(-self.push_speed),
                softness.mass_scale,
                softness.impulse_scale,
            )
        } else {
            (0.0, 1.0, 0.0)
        }
    }
}

/// `1 / k`, or zero when the effective mass is not positive (two bodies that cannot move).
pub(crate) fn inverse_or_zero(k: f32) -> f32 {
    if k > 0.0 { 1.0 / k } else { 0.0 }
}

/// `value` limited to `[-bound, bound]`; zero when the bound is not a number or negative.
pub(crate) fn limited(value: f32, bound: f32) -> f32 {
    if bound >= 0.0 {
        value.clamp(-bound, bound)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPS * b.abs().max(1.0)
    }

    fn sweep(use_bias: bool) -> Sweep {
        Sweep {
            inv_h: 240.0,
            use_bias,
            push_speed: 3.0,
            slop: 0.001,
            restitution_threshold: 1.0,
        }
    }

    #[test]
    fn softness_is_the_implicit_spring_of_the_hertz_and_ratio() {
        // omega = 2 pi 30, zeta 10, h = 1/240: a1 = 20 + h omega, a2 = h omega a1, a3 = 1/(1 + a2).
        let soft = Softness::new(30.0, 10.0, 1.0 / 240.0);
        let omega = f64::from(TAU) * 30.0;
        let h = 1.0 / 240.0_f64;
        let a1 = 20.0 + h * omega;
        let a2 = h * omega * a1;
        let a3 = 1.0 / (1.0 + a2);
        assert!((f64::from(soft.bias_rate) - omega / a1).abs() < 1e-3);
        assert!((f64::from(soft.mass_scale) - a2 * a3).abs() < 1e-5);
        assert!((f64::from(soft.impulse_scale) - a3).abs() < 1e-5);
        // A stiffer spring pulls harder and gives back less.
        let stiff = Softness::new(60.0, 10.0, 1.0 / 240.0);
        assert!(stiff.bias_rate > soft.bias_rate);
        assert!(stiff.impulse_scale < soft.impulse_scale);
        assert!(stiff.mass_scale > soft.mass_scale);
    }

    #[test]
    fn a_gap_is_closed_at_most_in_one_sub_step_and_without_softness() {
        let soft = Softness::new(30.0, 10.0, 1.0 / 240.0);
        for use_bias in [true, false] {
            let (bias, mass, impulse) = sweep(use_bias).terms(0.01, &soft);
            assert!(near(bias, 2.4));
            assert_eq!((mass, impulse), (1.0, 0.0));
        }
    }

    #[test]
    fn overlap_inside_the_slop_is_left_alone() {
        let soft = Softness::new(30.0, 10.0, 1.0 / 240.0);
        let (bias, mass, impulse) = sweep(true).terms(-0.0005, &soft);
        assert!(near(bias, 0.0));
        assert_eq!((mass, impulse), (soft.mass_scale, soft.impulse_scale));
        let (bias, mass, impulse) = sweep(true).terms(0.0, &soft);
        assert!(near(bias, 0.0));
        assert_eq!((mass, impulse), (soft.mass_scale, soft.impulse_scale));
    }

    #[test]
    fn overlap_beyond_the_slop_is_pushed_out_at_the_spring_rate_up_to_the_push_speed() {
        let soft = Softness::new(30.0, 10.0, 1.0 / 240.0);
        let (bias, mass, impulse) = sweep(true).terms(-0.003, &soft);
        assert!(near(bias, soft.bias_rate * -0.002));
        assert_eq!((mass, impulse), (soft.mass_scale, soft.impulse_scale));
        let (deep, _, _) = sweep(true).terms(-10.0, &soft);
        assert!(near(deep, -3.0));
    }

    #[test]
    fn the_relax_pass_has_no_bias_and_full_mass_for_overlap() {
        let soft = Softness::new(30.0, 10.0, 1.0 / 240.0);
        assert_eq!(sweep(false).terms(-0.5, &soft), (0.0, 1.0, 0.0));
    }

    #[test]
    fn limited_clamps_to_the_bound_and_gives_zero_for_a_bad_bound() {
        assert!(near(limited(5.0, 2.0), 2.0));
        assert!(near(limited(-5.0, 2.0), -2.0));
        assert!(near(limited(1.0, 2.0), 1.0));
        assert!(near(limited(1.0, 0.0), 0.0));
        assert!(near(limited(1.0, -1.0), 0.0));
        assert!(near(limited(1.0, f32::NAN), 0.0));
        assert!(limited(f32::NAN, 1.0).is_nan());
    }

    #[test]
    fn the_inverse_of_a_mass_is_zero_when_nothing_can_move() {
        assert!(near(inverse_or_zero(4.0), 0.25));
        assert!(near(inverse_or_zero(0.0), 0.0));
        assert!(near(inverse_or_zero(-1.0), 0.0));
        assert!(near(inverse_or_zero(f32::NAN), 0.0));
    }
}
