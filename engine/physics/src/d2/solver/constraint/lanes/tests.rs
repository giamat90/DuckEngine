//! The lane path against the scalar reference: random groups of four constraints, run both ways,
//! compared bit for bit (every impulse and every velocity), including manifolds of one point,
//! static bodies, speculative and deep overlaps, and values that make the clamps act.

use super::*;
use crate::core::BodyHandle;
use crate::d2::solver::constraint::{Softness, SolverBody};
use crate::d2::{D2, Rot2};
use base::rng::Pcg32;

const GROUPS: u32 = 400;

fn unit(rng: &mut Pcg32) -> f32 {
    rng.next_f32() * 2.0 - 1.0
}

fn vec(rng: &mut Pcg32, scale: f32) -> Vec2 {
    Vec2::new(unit(rng) * scale, unit(rng) * scale)
}

fn body(rng: &mut Pcg32, fixed: bool) -> SolverBody {
    let turn = Rot2::from_cos_sin(unit(rng) + 1.5, unit(rng)).unwrap_or(Rot2::IDENTITY);
    let (inv_mass, inv_inertia) = if fixed {
        (0.0, 0.0)
    } else {
        (0.2 + rng.next_f32(), 0.1 + rng.next_f32())
    };
    SolverBody {
        velocity: if fixed { Vec2::ZERO } else { vec(rng, 4.0) },
        spin: if fixed { 0.0 } else { unit(rng) * 3.0 },
        shift: vec(rng, 0.02),
        turn,
        inv_mass,
        inv_inertia,
        ..SolverBody::AT_REST
    }
}

fn point(rng: &mut Pcg32) -> Point {
    // Separations from deep overlap to a gap the bodies can close within a sub-step.
    Point {
        anchor_a: vec(rng, 0.6),
        anchor_b: vec(rng, 0.6),
        base_separation: unit(rng) * 0.05,
        normal_mass: rng.next_f32() * 3.0,
        tangent_mass: rng.next_f32() * 3.0,
        relative_velocity: unit(rng) * 3.0,
        normal_impulse: rng.next_f32() * 0.5,
        tangent_impulse: unit(rng) * 0.3,
        max_normal_impulse: rng.next_f32() * 0.6,
        ..Point::default()
    }
}

fn constraint(rng: &mut Pcg32) -> Constraint {
    let normal = Vec2::new(unit(rng) + 1.5, unit(rng))
        .normalized()
        .unwrap_or(Vec2::X);
    let softness = Softness::new(30.0 * (1.0 + rng.next_f32()), 10.0, 1.0 / 240.0);
    Constraint {
        a: 0,
        b: 1,
        first: BodyHandle::<D2>::from_raw(0, 0),
        second: BodyHandle::<D2>::from_raw(1, 0),
        normal,
        friction: rng.next_f32(),
        restitution: rng.next_f32() * 0.5,
        softness,
        count: 1 + (rng.next_u32() % 2) as usize,
        points: [point(rng), point(rng)],
    }
}

fn staged(rng: &mut Pcg32) -> Staged {
    // One end in four is a static body.
    let a_fixed = rng.next_u32().is_multiple_of(4);
    let b_fixed = !a_fixed && rng.next_u32().is_multiple_of(4);
    Staged {
        constraint: constraint(rng),
        a: body(rng, a_fixed),
        b: body(rng, b_fixed),
    }
}

fn group(rng: &mut Pcg32) -> [Staged; LANES] {
    std::array::from_fn(|_| staged(rng))
}

/// Everything a pass may change, as bit patterns.
fn bits(staged: &Staged) -> Vec<u32> {
    let mut out = Vec::new();
    for point in &staged.constraint.points {
        out.extend([
            point.normal_impulse.to_bits(),
            point.tangent_impulse.to_bits(),
            point.max_normal_impulse.to_bits(),
        ]);
    }
    for body in [&staged.a, &staged.b] {
        out.extend([
            body.velocity.x.to_bits(),
            body.velocity.y.to_bits(),
            body.spin.to_bits(),
        ]);
    }
    out
}

/// Runs `pass` on a random group with lanes and one by one, many times; asserts equal bits.
fn check(pass: impl Fn() -> Pass<'static>) {
    let mut rng = Pcg32::new(21, 3);
    for round in 0..GROUPS {
        let mut lanes = group(&mut rng);
        let mut scalar = lanes;
        run(&mut lanes, pass());
        for staged in &mut scalar {
            staged.run(pass());
        }
        for lane in 0..LANES {
            assert_eq!(
                bits(&lanes[lane]),
                bits(&scalar[lane]),
                "round {round} lane {lane}"
            );
        }
    }
}

static SOLVE: Sweep = Sweep {
    inv_h: 240.0,
    use_bias: true,
    push_speed: 3.0,
    slop: 0.001,
    restitution_threshold: 1.0,
};

static RELAX: Sweep = Sweep {
    inv_h: 240.0,
    use_bias: false,
    push_speed: 3.0,
    slop: 0.001,
    restitution_threshold: 1.0,
};

#[test]
fn the_solve_of_four_lanes_equals_four_scalar_solves() {
    check(|| Pass::Solve(&SOLVE));
}

#[test]
fn the_relax_pass_of_four_lanes_equals_four_scalar_passes() {
    check(|| Pass::Solve(&RELAX));
}

#[test]
fn the_warm_start_of_four_lanes_equals_four_scalar_warm_starts() {
    check(|| Pass::WarmStart);
}

#[test]
fn restitution_goes_through_the_scalar_reference_in_the_lane_path() {
    check(|| Pass::Restitute(&SOLVE));
}

#[test]
fn a_pass_changes_something_so_the_comparison_is_not_empty() {
    let mut rng = Pcg32::new(5, 5);
    let before = group(&mut rng);
    let mut after = before;
    run(&mut after, Pass::Solve(&SOLVE));
    let changed = (0..LANES).any(|lane| bits(&before[lane]) != bits(&after[lane]));
    assert!(changed);
}

#[test]
fn a_manifold_of_one_point_never_writes_its_second_point() {
    let mut rng = Pcg32::new(9, 9);
    let mut lanes = group(&mut rng);
    for staged in &mut lanes {
        staged.constraint.count = 1;
    }
    let second: Vec<u32> = lanes.iter().flat_map(|s| bits(s)[3..6].to_vec()).collect();
    run(&mut lanes, Pass::Solve(&SOLVE));
    let after: Vec<u32> = lanes.iter().flat_map(|s| bits(s)[3..6].to_vec()).collect();
    assert_eq!(second, after);
}

/// A NaN has many bit patterns and the payload carries no meaning, so two NaNs are taken as equal
/// here (a loose comparison on purpose); every other value, infinities and zeros included, must
/// match to the bit.
#[test]
fn non_finite_inputs_give_the_same_bits_as_the_reference() {
    let mut rng = Pcg32::new(33, 1);
    for special in [f32::NAN, f32::INFINITY, -f32::INFINITY] {
        let mut lanes = group(&mut rng);
        if let Some(staged) = lanes.get_mut(2) {
            staged.a.velocity = Vec2::new(special, 1.0);
        }
        let mut scalar = lanes;
        run(&mut lanes, Pass::Solve(&SOLVE));
        for staged in &mut scalar {
            staged.run(Pass::Solve(&SOLVE));
        }
        for lane in 0..LANES {
            let same = bits(&lanes[lane])
                .iter()
                .zip(bits(&scalar[lane]))
                .all(|(l, s)| f32::from_bits(*l).is_nan() && f32::from_bits(s).is_nan() || *l == s);
            assert!(same, "{special} lane {lane}");
        }
    }
}

#[test]
fn signed_zeros_give_the_same_bits_as_the_reference() {
    let mut rng = Pcg32::new(44, 2);
    for zero in [0.0_f32, -0.0] {
        let lanes = zeroed_group(&mut rng, zero);
        assert_same_as_scalar(&lanes, Pass::WarmStart, "warm start");
        assert_same_as_scalar(&lanes, Pass::Solve(&SOLVE), "solve");
        assert_same_as_scalar(&lanes, Pass::Solve(&RELAX), "relax");
    }
}

/// A group whose every number that matters is the zero `zero`, with mixed signs in the velocities.
fn zeroed_group(rng: &mut Pcg32, zero: f32) -> [Staged; LANES] {
    let mut lanes = group(rng);
    for (lane, staged) in lanes.iter_mut().enumerate() {
        let other = if lane % 2 == 0 { zero } else { -zero };
        staged.a.velocity = Vec2::new(zero, other);
        staged.b.spin = zero;
        for point in &mut staged.constraint.points {
            point.normal_impulse = zero;
            point.tangent_impulse = zero;
            point.relative_velocity = zero;
            point.base_separation = zero;
        }
    }
    lanes
}

/// Asserts that four lanes and four scalar runs of `pass` agree to the bit.
fn assert_same_as_scalar(lanes: &[Staged; LANES], pass: Pass<'_>, what: &str) {
    let mut scalar = *lanes;
    let mut four = *lanes;
    run(&mut four, pass);
    for staged in &mut scalar {
        staged.run(pass);
    }
    for (lane, (l, s)) in four.iter().zip(&scalar).enumerate() {
        assert_eq!(bits(l), bits(s), "{what} lane {lane}");
    }
}
