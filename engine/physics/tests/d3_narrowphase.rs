//! Card 8-4: the 3D narrowphase for sphere, capsule and box. Analytic cases first, then every pair of
//! shapes against an independent oracle written here in `f64` from the support functions of the
//! shapes (the signed separation of two convex shapes is the largest gap over all directions), then
//! the properties a solver relies on: symmetry, finite output, refusal of invalid input.

use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use physics::core::Dim;
use physics::d3::{D3, Hull, Manifold3, Placed3, Shape3};

const MARGIN: f32 = 0.1;

fn sphere(radius: f32, x: f32, y: f32, z: f32) -> Placed3 {
    Placed3::new(
        Shape3::Sphere { radius },
        Vec3::new(x, y, z),
        Quat::IDENTITY,
    )
}

fn cuboid(half: [f32; 3], at: [f32; 3], rotation: Quat) -> Placed3 {
    let shape = Shape3::Box {
        half_extents: Vec3::new(half[0], half[1], half[2]),
    };
    Placed3::new(shape, Vec3::new(at[0], at[1], at[2]), rotation)
}

fn capsule(half_height: f32, radius: f32, at: [f32; 3], rotation: Quat) -> Placed3 {
    let shape = Shape3::Capsule {
        half_height,
        radius,
    };
    Placed3::new(shape, Vec3::new(at[0], at[1], at[2]), rotation)
}

fn about(axis: Vec3, angle: f32) -> Quat {
    let Some(q) = Quat::from_axis_angle(axis, angle) else {
        panic!("the axis is a unit vector");
    };
    q
}

fn collide(a: &Placed3, b: &Placed3) -> Option<Manifold3> {
    D3::collide(a, b, MARGIN)
}

fn must(m: Option<Manifold3>) -> Manifold3 {
    let Some(m) = m else {
        panic!("the shapes were expected to touch");
    };
    m
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn near_vec(a: Vec3, b: Vec3) -> bool {
    near(a.x, b.x) && near(a.y, b.y) && near(a.z, b.z)
}

fn min_separation(m: &Manifold3) -> f32 {
    m.active()
        .iter()
        .map(|p| p.separation)
        .fold(f32::MAX, f32::min)
}

// ---- spheres --------------------------------------------------------------------------------

#[test]
fn two_overlapping_spheres_touch_along_the_line_of_centres() {
    let m = must(collide(
        &sphere(1.0, 0.0, 0.0, 0.0),
        &sphere(1.0, 1.5, 0.0, 0.0),
    ));
    assert_eq!(m.active().len(), 1);
    assert!(near_vec(m.normal, Vec3::X));
    assert!(near(m.active()[0].separation, -0.5));
    assert!(near_vec(m.active()[0].point, Vec3::new(0.75, 0.0, 0.0)));
}

#[test]
fn spheres_within_the_margin_touch_speculatively_and_beyond_it_do_not() {
    let a = sphere(1.0, 0.0, 0.0, 0.0);
    let m = must(collide(&a, &sphere(1.0, 2.05, 0.0, 0.0)));
    assert!(near(m.active()[0].separation, 0.05));
    assert!(collide(&a, &sphere(1.0, 2.2, 0.0, 0.0)).is_none());
}

#[test]
fn spheres_with_the_same_centre_get_a_fixed_normal() {
    let m = must(collide(
        &sphere(1.0, 2.0, 3.0, 4.0),
        &sphere(0.5, 2.0, 3.0, 4.0),
    ));
    assert!(near(m.normal.length(), 1.0));
    assert!(near(m.active()[0].separation, -1.5));
}

#[test]
fn a_sphere_resting_on_a_box_face_gets_the_face_normal() {
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    let ball = sphere(0.5, 1.0, 0.48, 2.0);
    let m = must(collide(&ground, &ball));
    assert!(near_vec(m.normal, Vec3::Y), "{:?}", m.normal);
    assert!(near(m.active()[0].separation, -0.02));
    // The same pair the other way round has the opposite normal and the same contact.
    let n = must(collide(&ball, &ground));
    assert!(near_vec(n.normal, -Vec3::Y));
    assert!(near(n.active()[0].separation, -0.02));
    assert!(near_vec(n.active()[0].point, m.active()[0].point));
}

#[test]
fn a_sphere_past_a_box_edge_and_corner_gets_a_diagonal_normal() {
    let b = cuboid([1.0, 1.0, 1.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    // Beyond the +x +y edge: the gap is the distance from the edge.
    let edge = must(collide(&b, &sphere(0.5, 1.3, 1.3, 0.0)));
    let d = (0.3_f32 * 0.3 + 0.3 * 0.3).sqrt();
    assert!(near(edge.active()[0].separation, d - 0.5));
    assert!(near_vec(
        edge.normal,
        Vec3::new(
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0
        )
    ));
    // Beyond the +x +y +z corner.
    let corner = must(collide(&b, &sphere(0.5, 1.3, 1.3, 1.3)));
    let c = (3.0_f32 * 0.3 * 0.3).sqrt();
    assert!(near(corner.active()[0].separation, c - 0.5));
}

#[test]
fn a_sphere_centre_inside_a_box_is_pushed_out_through_the_nearest_face() {
    let b = cuboid([1.0, 2.0, 3.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    let m = must(collide(&b, &sphere(0.25, 0.8, 0.0, 0.0)));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert!(near(m.active()[0].separation, -(0.2 + 0.25)));
}

#[test]
fn a_sphere_and_a_capsule_use_the_nearest_point_of_the_capsule_axis() {
    let cap = capsule(1.0, 0.5, [0.0, 0.0, 0.0], Quat::IDENTITY);
    // Beside the middle of the axis.
    let side = must(collide(&sphere(0.5, 0.9, 0.3, 0.0), &cap));
    assert!(near_vec(side.normal, -Vec3::X));
    assert!(near(side.active()[0].separation, -0.1));
    // Past the top end: the end cap is a sphere.
    let top = must(collide(&sphere(0.5, 0.0, 2.05, 0.0), &cap));
    assert!(near_vec(top.normal, -Vec3::Y));
    assert!(near(top.active()[0].separation, 0.05));
}

// ---- capsules -------------------------------------------------------------------------------

#[test]
fn two_crossing_capsules_get_one_point_and_a_normal_across_both_axes() {
    let a = capsule(1.0, 0.25, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(
        1.0,
        0.25,
        [0.0, 0.0, 0.4],
        about(Vec3::Z, std::f32::consts::FRAC_PI_2),
    );
    let m = must(collide(&a, &b));
    assert_eq!(m.active().len(), 1);
    assert!(near_vec(m.normal, Vec3::Z), "{:?}", m.normal);
    assert!(near(m.active()[0].separation, 0.4 - 0.5));
}

#[test]
fn two_capsules_whose_axes_intersect_still_get_a_unit_normal() {
    let a = capsule(1.0, 0.25, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(
        1.0,
        0.25,
        [0.0, 0.0, 0.0],
        about(Vec3::X, std::f32::consts::FRAC_PI_2),
    );
    let m = must(collide(&a, &b));
    assert!(near(m.normal.length(), 1.0));
    assert!(near(m.active()[0].separation, -0.5));
}

#[test]
fn two_parallel_capsules_side_by_side_get_two_points_along_the_overlap() {
    let a = capsule(1.0, 0.25, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(1.0, 0.25, [0.45, 0.5, 0.0], Quat::IDENTITY);
    let m = must(collide(&a, &b));
    assert_eq!(m.active().len(), 2);
    let ys: Vec<f32> = m.active().iter().map(|p| p.point.y).collect();
    assert!(
        ys.iter().any(|y| near(*y, -0.5)) && ys.iter().any(|y| near(*y, 1.0)),
        "{ys:?}"
    );
    for p in m.active() {
        assert!(near(p.separation, 0.45 - 0.5), "{}", p.separation);
    }
    assert!(near_vec(m.normal, Vec3::X));
}

/// A capsule beside the upright capsule `a` (radius 0.25, half height 1, on the y axis), its axis
/// turned by `lean` about z, so that its ends are `2 sin(lean)` apart in distance from `a`; `gap` is
/// the distance between the axes at its middle.
fn leaning_beside(lean: f32, gap: f32) -> (Placed3, Placed3) {
    let a = capsule(1.0, 0.25, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(1.0, 0.25, [gap, 0.0, 0.0], about(Vec3::Z, lean));
    (a, b)
}

#[test]
fn a_capsule_leaning_by_a_few_millimetres_beside_another_keeps_both_ends() {
    for lean in [0.0, 0.0005, 0.002, 0.005, 0.02, 0.04] {
        let (a, b) = leaning_beside(lean, 0.45);
        let m = must(collide(&a, &b));
        assert_eq!(m.active().len(), 2, "lean {lean}");
        assert!(
            near_vec(m.normal, Vec3::X) || m.normal.x > 0.99,
            "{:?}",
            m.normal
        );
    }
}

#[test]
fn the_far_end_of_a_leaning_capsule_beside_another_is_kept_up_to_the_margin_and_not_beyond() {
    // The ends are 2 sin(lean) apart in distance (about 2 lean): 0.049 m against the margin 0.1
    // is within it, 0.15 m is not.
    let (a, near_end) = leaning_beside(0.0245, 0.45);
    assert_eq!(must(collide(&a, &near_end)).active().len(), 2);
    let (a, far_end) = leaning_beside(0.075, 0.45);
    assert_eq!(must(collide(&a, &far_end)).active().len(), 1);
}

/// Card 8-89: two skew capsules (the axes 0.3 rad apart, 0.45 between them at the middle, their
/// closest points) have a second point at an end of the overlap whose separation is the true distance
/// of the surfaces there, 0.0312 (the end of the overlap along `a`, y = cos 0.3, against the nearest
/// point of `b`; not the -0.05 of the closest pair), in either order.
#[test]
fn the_far_point_of_two_skew_capsules_has_the_true_distance_of_the_surfaces() {
    let a = capsule(1.0, 0.25, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(1.0, 0.25, [0.45, 0.0, 0.0], about(Vec3::X, 0.3));
    for m in [must(collide(&a, &b)), must(collide(&b, &a))] {
        let mut seps: Vec<f32> = m.active().iter().map(|p| p.separation).collect();
        seps.sort_by(f32::total_cmp);
        assert_eq!(seps.len(), 2, "{seps:?}");
        assert!(near(seps[0], 0.45 - 0.5), "{seps:?}");
        assert!(near(seps[1], 0.0312), "{seps:?}");
    }
}

/// Card 8-89: a short capsule across a long one at a wide angle (about 34 degrees) has both ends
/// within the margin of it, but is a crossing and not two capsules alongside: one point (with both,
/// the mixed pile of card 8-21 no longer sleeps within 2000 steps).
#[test]
fn a_short_capsule_across_another_at_a_wide_angle_gets_one_point() {
    let a = capsule(1.0, 0.25, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(0.05, 0.25, [0.45, 0.0, 0.0], about(Vec3::Z, 0.6));
    assert_eq!(must(collide(&a, &b)).active().len(), 1);
}

#[test]
fn capsules_end_to_end_on_one_axis_meet_at_the_end_caps() {
    let a = capsule(1.0, 0.5, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(1.0, 0.5, [0.0, 2.9, 0.0], Quat::IDENTITY);
    let m = must(collide(&a, &b));
    assert_eq!(m.active().len(), 1);
    assert!(near_vec(m.normal, Vec3::Y));
    assert!(near(m.active()[0].separation, 2.9 - 2.0 - 1.0));
}

#[test]
fn coaxial_overlapping_capsules_get_a_unit_normal_perpendicular_to_the_axis() {
    let a = capsule(1.0, 0.5, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = capsule(1.0, 0.5, [0.0, 0.5, 0.0], Quat::IDENTITY);
    let m = must(collide(&a, &b));
    assert!(near(m.normal.length(), 1.0));
    assert!(near(m.normal.dot(Vec3::Y), 0.0));
    assert!(near(m.active()[0].separation, -1.0));
}

// ---- capsule and box ------------------------------------------------------------------------

#[test]
fn a_capsule_lying_on_a_box_face_gets_two_points_and_one_standing_gets_one() {
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    let lying = capsule(
        1.0,
        0.5,
        [0.0, 0.48, 0.0],
        about(Vec3::Z, std::f32::consts::FRAC_PI_2),
    );
    let m = must(collide(&ground, &lying));
    assert_eq!(m.active().len(), 2);
    assert!(near_vec(m.normal, Vec3::Y), "{:?}", m.normal);
    for p in m.active() {
        assert!(near(p.separation, -0.02), "{}", p.separation);
    }
    let xs: Vec<f32> = m.active().iter().map(|p| p.point.x).collect();
    assert!(
        xs.iter().any(|x| *x < -0.9) && xs.iter().any(|x| *x > 0.9),
        "{xs:?}"
    );
    let standing = capsule(1.0, 0.5, [0.0, 1.48, 0.0], Quat::IDENTITY);
    let s = must(collide(&ground, &standing));
    assert_eq!(s.active().len(), 1);
    assert!(near(s.active()[0].separation, -0.02));
}

#[test]
fn a_capsule_hanging_over_the_edge_of_a_box_has_a_point_only_where_the_box_is() {
    let slab = cuboid([1.0, 0.5, 1.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    // Lying along X, centred at x = 1.5: the left end (x = 0.5) is over the slab, the right end
    // (x = 2.5) is 1.5 m past its edge.
    let cap = capsule(
        1.0,
        0.5,
        [1.5, 0.48, 0.0],
        about(Vec3::Z, std::f32::consts::FRAC_PI_2),
    );
    let m = must(collide(&slab, &cap));
    assert_eq!(m.active().len(), 1, "{:?}", m.active());
    assert!(m.active()[0].point.x < 1.1);
}

#[test]
fn a_capsule_lying_along_a_box_edge_gets_two_points_and_one_across_it_gets_one() {
    let b = cuboid([1.0, 1.0, 1.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    // Along the vertical edge at x = 1, y = 1: both ends are as near as the middle.
    let along = capsule(
        1.0,
        0.25,
        [1.2, 1.2, 0.0],
        about(Vec3::X, std::f32::consts::FRAC_PI_2),
    );
    let m = must(collide(&b, &along));
    assert_eq!(m.active().len(), 2);
    assert!(m.normal.x > 0.5 && m.normal.y > 0.5, "{:?}", m.normal);
    // Tilted so that the middle of the capsule is the part nearest the edge.
    let tilted = capsule(
        1.0,
        0.25,
        [1.2, 1.0, 0.0],
        about(Vec3::X, std::f32::consts::FRAC_PI_4),
    );
    let t = must(collide(&b, &tilted));
    assert_eq!(t.active().len(), 1);
    assert!(
        near(t.active()[0].separation, 0.2 - 0.25),
        "{}",
        t.active()[0].separation
    );
}

/// A capsule (half length 1, radius 0.5) lying on the top face (y = 0) of a box, turned from flat
/// by `lean` radians so that its two ends are `2 sin(lean)` apart in height, the lower end 0.02
/// into the box.
fn leaning_capsule(lean: f32, sink: f32) -> Placed3 {
    let turn = std::f32::consts::FRAC_PI_2 - lean;
    capsule(
        1.0,
        0.5,
        [0.0, 0.5 - sink + base::dmath::sin(lean), 0.0],
        about(Vec3::Z, turn),
    )
}

#[test]
fn a_capsule_leaning_by_a_few_millimetres_keeps_both_ends_while_both_are_within_the_margin() {
    // Card 8-21: the ends are 8 mm apart in height (lean 0.004), more than the 5 mm an end was
    // allowed from the nearest point, so the upper end lost its point and the capsule rocked on the
    // other; the margin (0.1 here) is what makes a point worth keeping.
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    for lean in [0.0, 0.002, 0.004, 0.02, 0.04] {
        let m = must(collide(&ground, &leaning_capsule(lean, 0.02)));
        assert_eq!(m.active().len(), 2, "lean {lean}: {:?}", m.active());
    }
    // Further apart than the margin (0.1): the upper end is out of reach, one point.
    let far = must(collide(&ground, &leaning_capsule(0.08, 0.02)));
    assert_eq!(far.active().len(), 1, "{:?}", far.active());
}

#[test]
fn the_end_of_a_leaning_capsule_is_kept_up_to_the_margin_and_not_beyond() {
    // Ends 0.0979 and 0.1020 apart, the margin 0.1: the band is the margin (card 8-21).
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    let inside = must(collide(&ground, &leaning_capsule(0.049, 0.02)));
    assert_eq!(inside.active().len(), 2, "{:?}", inside.active());
    let beyond = must(collide(&ground, &leaning_capsule(0.051, 0.02)));
    assert_eq!(beyond.active().len(), 1, "{:?}", beyond.active());
}

#[test]
fn a_leaning_capsule_keeps_both_ends_at_the_default_margin_and_at_a_margin_of_zero() {
    // The margin of the pile scenes is 0.02 (ends 8 mm apart lost one before); a margin of zero
    // must not lose the end of a capsule lying flat to rounding.
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    for (lean, margin) in [(0.004, 0.02), (0.0, 0.0), (0.002, 0.0), (0.0, 0.001)] {
        let m = must(D3::collide(&ground, &leaning_capsule(lean, 0.02), margin));
        assert_eq!(
            m.active().len(),
            2,
            "lean {lean} margin {margin}: {:?}",
            m.active()
        );
    }
}

#[test]
fn a_capsule_sunk_through_a_box_face_and_leaning_keeps_both_ends() {
    // The same lean, with the whole segment inside the box (card 8-21, the deep path).
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    for lean in [0.0, 0.004, 0.02] {
        let m = must(collide(&ground, &leaning_capsule(lean, 0.7)));
        assert_eq!(m.active().len(), 2, "lean {lean}: {:?}", m.active());
    }
}

// ---- boxes ----------------------------------------------------------------------------------

#[test]
fn a_box_stacked_on_a_box_gets_four_points_at_the_corners_of_the_overlap() {
    let a = cuboid([1.0, 0.5, 1.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = cuboid([0.5, 0.5, 0.5], [0.25, 0.95, 0.0], Quat::IDENTITY);
    let m = must(collide(&a, &b));
    assert_eq!(m.active().len(), 4);
    assert!(near_vec(m.normal, Vec3::Y));
    for p in m.active() {
        assert!(near(p.separation, -0.05), "{}", p.separation);
        assert!(
            near(p.point.y, 0.5 - 0.025) || near(p.point.y, 0.475),
            "{:?}",
            p.point
        );
        assert!(p.point.x > -0.26 && p.point.x < 0.76 && p.point.z.abs() < 0.51);
    }
    // The four are distinct.
    for (i, p) in m.active().iter().enumerate() {
        for q in &m.active()[i + 1..] {
            assert!((p.point - q.point).length() > 0.5);
        }
    }
}

#[test]
fn the_box_pair_gets_the_same_contact_in_either_order() {
    let a = cuboid([1.0, 0.5, 1.0], [0.0, 0.0, 0.0], about(Vec3::Y, 0.3));
    let b = cuboid(
        [0.5, 0.5, 0.5],
        [0.3, 0.9, 0.2],
        about(
            Vec3::new(1.0, 0.0, 1.0).normalized().unwrap_or(Vec3::X),
            0.2,
        ),
    );
    let (m, n) = (must(collide(&a, &b)), must(collide(&b, &a)));
    assert!(near_vec(m.normal, -n.normal));
    assert_eq!(m.active().len(), n.active().len());
    assert!(near(min_separation(&m), min_separation(&n)));
}

#[test]
fn two_boxes_crossing_edge_to_edge_get_one_point_on_the_edge_axis_normal() {
    // A cube turned 45 degrees about X, a cube turned 45 degrees about Z above it: edges cross.
    let q1 = about(Vec3::X, std::f32::consts::FRAC_PI_4);
    let q2 = about(Vec3::Z, std::f32::consts::FRAC_PI_4);
    let a = cuboid([0.5, 0.5, 0.5], [0.0, 0.0, 0.0], q1);
    let b = cuboid([0.5, 0.5, 0.5], [0.0, 1.3, 0.0], q2);
    let m = must(collide(&a, &b));
    assert_eq!(m.active().len(), 1, "{:?}", m.active());
    assert!(m.normal.y > 0.5);
}

#[test]
fn boxes_with_a_gap_beyond_the_margin_do_not_touch_and_within_it_do() {
    let a = cuboid([0.5, 0.5, 0.5], [0.0, 0.0, 0.0], Quat::IDENTITY);
    assert!(
        collide(
            &a,
            &cuboid([0.5, 0.5, 0.5], [0.0, 1.2, 0.0], Quat::IDENTITY)
        )
        .is_none()
    );
    let m = must(collide(
        &a,
        &cuboid([0.5, 0.5, 0.5], [0.0, 1.05, 0.0], Quat::IDENTITY),
    ));
    assert!(near(min_separation(&m), 0.05));
}

#[test]
fn a_small_box_sunk_into_a_big_one_is_pushed_out_the_shortest_way() {
    let big = cuboid([2.0, 2.0, 2.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    let small = cuboid([0.25, 0.25, 0.25], [1.9, 0.1, 0.0], Quat::IDENTITY);
    let m = must(collide(&big, &small));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert!(near(min_separation(&m), -(2.0 - 1.9 + 0.25)));
}

// ---- refusal --------------------------------------------------------------------------------

/// How far the depth of a cube hull sunk by `depth` into a box of its size (`half` each way, both
/// turned `yaw` about the vertical axis) is from `depth`, plus how far its normal is from the box's x
/// axis (the two shapes have the same orientation: this is the easy case for EPA, no relative tilt).
fn sunk_cube_error(half: f32, yaw: f32, depth: f32) -> Result<f32, String> {
    let corner = |bit: u32, i: u32| if i & bit == 0 { -half } else { half };
    let cube: Vec<Vec3> = (0..8)
        .map(|i| Vec3::new(corner(1, i), corner(2, i), corner(4, i)))
        .collect();
    let hull = Shape3::Hull(Hull::new(&cube).map_err(|e| format!("{e:?}"))?);
    let turn = about(Vec3::Y, yaw);
    let at = Vec3::new(5.0, 7.0, -3.0);
    let boxed = cuboid([half; 3], [at.x, at.y, at.z], turn);
    let offset = turn.rotate(Vec3::new(2.0 * half - depth, 0.3, -0.2));
    let sunk = Placed3::new(hull, at + offset, turn);
    let found = must(collide(&boxed, &sunk));
    // The normal is the box's own x axis (the shortest way out), not only the right depth.
    let along = found.normal.dot(turn.rotate(Vec3::X));
    Ok((min_separation(&found) + depth).abs() + (1.0 - along).abs())
}

/// Card 8-79: a cube hull sunk into a box of its size, 100 m and 2000 m across (at 1000 m from the
/// origin `f32` steps by about 1.2e-4 m, and the expected depth is itself computed in `f32`), both
/// turned together about the vertical axis: the depth is the shortest way out along the box's own
/// axes, to within 0.5 mm (about four steps at 2000 m: it catches a gross loss of digits, not one or
/// two units), and the normal is that axis. The cases were not tried by card 8-75; no failing pose
/// was found, this pins that. Not covered: a relative tilt of the two shapes, a depth that is a
/// large part of the shape.
#[test]
fn a_huge_hull_cube_sunk_into_a_box_of_its_size_gets_its_depth() -> Result<(), String> {
    let mut worst = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    for (half, yaw, depth) in huge_cube_cases() {
        let error = sunk_cube_error(half, yaw, depth)?;
        if error > worst.0 {
            worst = (error, half, yaw, depth);
        }
    }
    assert!(worst.0 < 5.0e-4, "(error, half, yaw, depth) {worst:?}");
    Ok(())
}

/// Sizes, turns and depths of the huge cube test.
fn huge_cube_cases() -> Vec<(f32, f32, f32)> {
    let mut cases = Vec::new();
    for half in [50.0_f32, 1000.0] {
        for yaw in [0.0_f32, 0.37, 1.1] {
            cases.extend([0.2_f32, 1.0, 10.0].map(|depth| (half, yaw, depth)));
        }
    }
    cases
}

/// How far the lowest separation of the prism sweep may be from the height of the lowest corner: 0.1 mm.
const PRISM_HEIGHT_TOLERANCE: f32 = 1.0e-4;

/// The corners of a prism of `sides` sides (radius 0.5, 1 m long along z) as the points given to
/// `Hull::new`. The hull keeps them in its principal frame (the centre of mass at the origin, the axes
/// of the inertia tensor): for a regular prism that frame is turned about z by an arbitrary angle
/// (3.6 degrees for 8 sides), so the heights of the corners are those of `Hull::vertices`, not of
/// these points (card 9-5: a probe that used these points saw a 1 mm error that was its own).
fn prism_points(sides: u16) -> Vec<Vec3> {
    let mut corners = Vec::new();
    for i in 0..sides {
        let angle = std::f32::consts::TAU * f32::from(i) / f32::from(sides);
        let rim = Vec3::new(
            0.5 * base::dmath::cos(angle),
            0.5 * base::dmath::sin(angle),
            0.0,
        );
        corners.push(rim - Vec3::Z * 0.5);
        corners.push(rim + Vec3::Z * 0.5);
    }
    corners
}

/// How far the lowest separation of a prism over a floor is from the height of its lowest corner
/// above the floor top (`None`: no manifold), for the prism turned `turn` about z and tilted `tilt`
/// about x. The corners are those of the hull itself.
fn prism_height_error(hull: &Hull, turn: f32, tilt: f32, height: f32) -> Option<f32> {
    let rotation = about(Vec3::X, tilt) * about(Vec3::Z, turn);
    let low = hull
        .vertices()
        .iter()
        .map(|v| rotation.rotate(*v).y)
        .fold(f32::MAX, f32::min);
    let floor = cuboid([50.0, 1.0, 50.0], [13.7, -1.0, -21.3], Quat::IDENTITY);
    let at = Vec3::new(16.7, height - low, -19.3);
    let prism = Placed3::new(Shape3::Hull(*hull), at, rotation);
    collide(&floor, &prism).map(|m| (min_separation(&m) - height).abs())
}

/// Card 9-5: the whole manifold (face clipping, not only GJK) of a prism on a floor gives as its lowest
/// separation the height of the lowest corner, for prisms of 8, 12 and 16 sides, turns every 3
/// degrees, two tilts, and heights from 0.5 mm above the floor to 5 cm sunk, to within 0.1 mm. The
/// card was opened for a 1 mm error that came from the probe (see [`prism_points`]); this pins that
/// there is none.
#[test]
fn the_lowest_point_of_a_prism_manifold_is_at_the_height_of_its_lowest_corner() -> Result<(), String>
{
    let mut bad = Vec::new();
    for sides in [8_u16, 12, 16] {
        let hull = Hull::new(&prism_points(sides)).map_err(|e| format!("{e:?}"))?;
        bad.extend(prism_failures(sides, &hull));
    }
    assert!(
        bad.is_empty(),
        "(sides, turn, (tilt, height), error): {bad:?}"
    );
    Ok(())
}

/// A pose that failed: sides, turn, (tilt, height), and the error (`None`: no manifold).
type Failure = (u16, f32, (f32, f32), Option<f32>);

/// The poses of the sweep of one prism that are off by 0.1 mm or more, or have no manifold.
fn prism_failures(sides: u16, hull: &Hull) -> Vec<Failure> {
    let turns = (0..60_u16).map(|k| (3.0 * f32::from(k) + 0.13) * std::f32::consts::PI / 180.0);
    let poses = [
        (0.0_f32, 0.0005_f32),
        (0.0, 0.0),
        (0.003, -0.005),
        (0.003, -0.05),
    ];
    turns
        .flat_map(|turn| poses.map(|pose| (turn, pose)))
        .map(|(turn, pose)| (turn, pose, prism_height_error(hull, turn, pose.0, pose.1)))
        .filter(|(_, _, error)| !error.is_some_and(|e| e < PRISM_HEIGHT_TOLERANCE))
        .map(|(turn, pose, error)| (sides, turn, pose, error))
        .collect()
}

#[test]
fn invalid_shapes_positions_orientations_and_margins_give_no_manifold() {
    let good = sphere(1.0, 0.0, 0.0, 0.0);
    assert!(collide(&good, &sphere(-1.0, 0.0, 0.0, 0.0)).is_none());
    assert!(collide(&good, &sphere(1.0, f32::NAN, 0.0, 0.0)).is_none());
    assert!(collide(&good, &sphere(1.0, 0.0, f32::INFINITY, 0.0)).is_none());
    let bad_rotation = Placed3::new(
        Shape3::Box {
            half_extents: Vec3::new(1.0, 1.0, 1.0),
        },
        Vec3::ZERO,
        Quat::new(0.0, 0.0, 0.0, 0.0),
    );
    assert!(collide(&good, &bad_rotation).is_none());
    assert!(D3::collide(&good, &sphere(1.0, 0.5, 0.0, 0.0), -0.1).is_none());
    assert!(D3::collide(&good, &sphere(1.0, 0.5, 0.0, 0.0), f32::NAN).is_none());
    assert!(D3::collide(&good, &sphere(1.0, 0.5, 0.0, 0.0), 0.0).is_some());
}

// ---- the oracle -----------------------------------------------------------------------------

type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn neg(a: V) -> V {
    [-a[0], -a[1], -a[2]]
}

fn wide(v: Vec3) -> V {
    [f64::from(v.x), f64::from(v.y), f64::from(v.z)]
}

/// One shape in `f64`: centre, local axes in world space, and what its support function needs.
struct Body {
    centre: V,
    axes: [V; 3],
    shape: Shape3,
}

impl Body {
    fn new(placed: &Placed3) -> Self {
        let q = placed.rotation;
        Self {
            centre: wide(placed.position),
            axes: [
                wide(q.rotate(Vec3::X)),
                wide(q.rotate(Vec3::Y)),
                wide(q.rotate(Vec3::Z)),
            ],
            shape: placed.shape,
        }
    }

    /// The largest `n . p` over the shape.
    fn support(&self, n: V) -> f64 {
        let along_centre = dot(n, self.centre);
        match self.shape {
            Shape3::Sphere { radius } => along_centre + f64::from(radius),
            Shape3::Box { half_extents: h } => {
                let half = [f64::from(h.x), f64::from(h.y), f64::from(h.z)];
                let reach: f64 = (0..3).map(|i| dot(n, self.axes[i]).abs() * half[i]).sum();
                along_centre + reach
            }
            Shape3::Capsule {
                half_height,
                radius,
            } => {
                along_centre
                    + dot(n, self.axes[1]).abs() * f64::from(half_height)
                    + f64::from(radius)
            }
            Shape3::Hull(hull) => along_centre + hull_extent(&hull, &self.axes, n),
            _ => along_centre,
        }
    }
}

/// The gap along `n`: positive while `a` and `b` are apart along it.
fn gap(a: &Body, b: &Body, n: V) -> f64 {
    -b.support(neg(n)) - a.support(n)
}

fn directions(count: usize) -> Vec<V> {
    let mut rng = Pcg32::new(0x8_4, 7);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let v = [0, 1, 2].map(|_| f64::from(rng.next_f32()) * 2.0 - 1.0);
        let l2 = dot(v, v);
        if l2 > 0.01 && l2 <= 1.0 {
            let l = l2.sqrt();
            out.push([v[0] / l, v[1] / l, v[2] / l]);
        }
    }
    out
}

/// The signed separation of two convex shapes: the largest gap over the sampled directions.
fn oracle_separation(a: &Body, b: &Body, dirs: &[V]) -> f64 {
    let (mut best, mut best_gap) = ([1.0, 0.0, 0.0], f64::MIN);
    for d in dirs {
        let g = gap(a, b, *d);
        if g > best_gap {
            (best, best_gap) = (*d, g);
        }
    }
    refine(a, b, best, best_gap)
}

/// Local search around the best sample: random nudges that shrink, kept when the gap grows.
fn refine(first: &Body, second: &Body, start: V, start_gap: f64) -> f64 {
    let mut rng = Pcg32::new(0x8_4, 99);
    let (mut best, mut best_gap) = (start, start_gap);
    let mut step = 0.02;
    for _ in 0..15_000 {
        let nudge = [0, 1, 2].map(|_| (f64::from(rng.next_f32()) * 2.0 - 1.0) * step);
        let moved = [best[0] + nudge[0], best[1] + nudge[1], best[2] + nudge[2]];
        let length = dot(moved, moved).sqrt();
        let candidate = [moved[0] / length, moved[1] / length, moved[2] / length];
        let candidate_gap = gap(first, second, candidate);
        if candidate_gap > best_gap {
            (best, best_gap) = (candidate, candidate_gap);
        } else {
            step *= 0.9995;
        }
    }
    best_gap
}

fn random_unit_quat(rng: &mut Pcg32) -> Quat {
    let mut next = || f32::from(i16::try_from(rng.below(2001).unwrap_or(0)).unwrap_or(0) - 1000);
    let q = Quat::new(next(), next(), next(), next() + 1.0);
    q.normalized().unwrap_or(Quat::IDENTITY)
}

fn random_shape(rng: &mut Pcg32, kind: usize) -> Shape3 {
    let mut size =
        || 0.25 + f32::from(u8::try_from(rng.below(128).unwrap_or(0)).unwrap_or(0)) / 256.0;
    match kind {
        0 => Shape3::Sphere { radius: size() },
        1 => Shape3::Capsule {
            half_height: size(),
            radius: size() * 0.6,
        },
        2 => Shape3::Box {
            half_extents: Vec3::new(size(), size(), size()),
        },
        _ => random_hull(rng),
    }
}

/// A hull of a handful of random points inside a cube of half side 0.6 (a box if they are flat).
fn random_hull(rng: &mut Pcg32) -> Shape3 {
    let mut coord =
        || f32::from(i16::try_from(rng.below(1201).unwrap_or(0)).unwrap_or(0) - 600) / 1000.0;
    let points: Vec<Vec3> = (0..9)
        .map(|_| Vec3::new(coord(), coord(), coord()))
        .collect();
    match Hull::new(&points) {
        Ok(hull) => Shape3::Hull(hull),
        Err(_) => Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        },
    }
}

fn random_placed(rng: &mut Pcg32, kind: usize) -> Placed3 {
    let shape = random_shape(rng, kind);
    let mut coord =
        || f32::from(i16::try_from(rng.below(1537).unwrap_or(0)).unwrap_or(0) - 768) / 1024.0;
    let position = Vec3::new(coord(), coord(), coord());
    Placed3::new(shape, position, random_unit_quat(rng))
}

fn show(p: &Placed3) -> String {
    format!("{:?} at {:?} turned {:?}", p.shape, p.position, p.rotation)
}

/// The two shapes, for a failure message.
fn context(a: &Placed3, b: &Placed3) -> String {
    format!("{} {}", show(a), show(b))
}

/// What a scene is for the oracle: how the pair stands against the margin.
enum Verdict {
    Apart,
    Touching,
    TooCloseToCall,
}

/// The answer for a pair that the oracle says touches: depth, normal, symmetry.
fn check_touching(a: &Placed3, b: &Placed3, oracle: f64, slack: f64) {
    const TOL: f64 = 1.0e-3;
    let (oa, ob) = (Body::new(a), Body::new(b));
    let Some(m) = collide(a, b) else {
        panic!(
            "separation {oracle} is within the margin but nothing came back: {}",
            context(a, b)
        );
    };
    let mine = f64::from(min_separation(&m));
    // The oracle is a lower bound of the true separation that falls a little short at the sharp
    // peak of the gap function, so the answer may be above it by a hair but never below.
    assert!(
        mine > oracle - TOL && mine < oracle + slack,
        "separation {mine} against the oracle {oracle}: {}",
        context(a, b)
    );
    let along_mine = gap(&oa, &ob, wide(m.normal));
    assert!(
        along_mine > oracle - TOL,
        "normal {:?} has gap {along_mine}, best is {oracle}: {}",
        m.normal,
        context(a, b)
    );
    // The deepest point sits on the surfaces: the gap of the shapes along the normal is its depth.
    assert!(
        (mine - along_mine).abs() < slack,
        "depth {mine} but the shapes' gap is {along_mine}: {}",
        context(a, b)
    );
    assert!((f64::from(m.normal.length()) - 1.0).abs() < 1e-4);
    assert!((1..=4).contains(&m.active().len()));
    let back = must(collide(b, a));
    assert!(
        (f64::from(min_separation(&back)) - mine).abs() < 1e-4 && near_vec(back.normal, -m.normal),
        "swapped pair differs: {}",
        context(a, b)
    );
}

/// Everything the production answer must agree with the oracle on, for one scene.
fn check_scene(a: &Placed3, b: &Placed3, dirs: &[V], slack: f64) -> Verdict {
    const TOL: f64 = 1.0e-3;
    let oracle = oracle_separation(&Body::new(a), &Body::new(b), dirs);
    let margin = f64::from(MARGIN);
    if oracle > margin + TOL {
        assert!(
            collide(a, b).is_none(),
            "apart by {oracle} yet touching: {}",
            context(a, b)
        );
        return Verdict::Apart;
    }
    if oracle > margin - TOL {
        return Verdict::TooCloseToCall;
    }
    check_touching(a, b, oracle, slack);
    Verdict::Touching
}

#[test]
fn every_pair_of_shapes_agrees_with_the_support_function_oracle() {
    let dirs = directions(20_000);
    for (kind_a, kind_b) in [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)] {
        let mut rng = Pcg32::new(0x8_4, (kind_a * 3 + kind_b) as u64);
        let (mut touching, mut apart) = (0, 0);
        // Face clipping can drop the corner nearest the other box, so a box pair's speculative
        // separation may be above the true one by a little; the other pairs stay within 4e-3 (the oracle's own step).
        let slack = if kind_b == 2 && kind_a == 2 {
            1.0e-2
        } else {
            4.0e-3
        };
        for _ in 0..150 {
            let (a, b) = (
                random_placed(&mut rng, kind_a),
                random_placed(&mut rng, kind_b),
            );
            let verdict = check_scene(&a, &b, &dirs, slack);
            touching += usize::from(matches!(verdict, Verdict::Touching));
            apart += usize::from(matches!(verdict, Verdict::Apart));
        }
        assert!(
            touching > 30 && apart > 5,
            "pair ({kind_a},{kind_b}): {touching} touching, {apart} apart"
        );
    }
}

// ---- in a world ------------------------------------------------------------------------------

fn small_world() -> Result<physics::d3::World, physics::core::PhysicsError> {
    use physics::d3::{
        DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, SleepConfig, SolverConfig, World,
        WorldConfig,
    };
    World::new(WorldConfig {
        max_bodies: 8,
        gravity: Vec3::ZERO,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })
}

#[test]
fn a_world_step_makes_the_manifold_of_each_touching_pair() -> Result<(), physics::core::PhysicsError>
{
    use physics::d3::{BodyDesc, BodyKind};
    let mut world = small_world()?;
    let ground = BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        Shape3::Box {
            half_extents: Vec3::new(5.0, 0.5, 5.0),
        },
    );
    let ball = BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::new(0.0, 0.45, 0.0),
        Shape3::Sphere { radius: 0.5 },
    );
    let far = BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::new(0.0, 4.0, 0.0),
        Shape3::Sphere { radius: 0.5 },
    );
    let (ground, ball) = (world.create_body(&ground)?, world.create_body(&ball)?);
    world.create_body(&far)?;
    world.step(1.0e-3)?;
    let found = world.manifolds().as_slice();
    assert_eq!(found.len(), 1, "only the ball touches the ground");
    let contact = &found[0];
    let pair = [contact.first, contact.second];
    assert!(pair.contains(&ground) && pair.contains(&ball));
    assert_eq!(contact.manifold.active().len(), 1);
    assert!(contact.manifold.active()[0].separation < 0.0);
    // The same scene again gives the same manifolds, bit for bit.
    let mut again = small_world()?;
    for shape in [
        (BodyKind::Static, -0.5, 5.0),
        (BodyKind::Dynamic, 0.45, 0.5),
    ] {
        let half = if shape.0 == BodyKind::Static {
            Shape3::Box {
                half_extents: Vec3::new(5.0, 0.5, 5.0),
            }
        } else {
            Shape3::Sphere { radius: shape.2 }
        };
        again.create_body(&BodyDesc::new(shape.0, Vec3::new(0.0, shape.1, 0.0), half))?;
    }
    again.step(1.0e-3)?;
    assert_eq!(again.manifolds().as_slice().len(), 1);
    Ok(())
}

// ---- where the points are ---------------------------------------------------------------------

fn has_point(m: &Manifold3, at: Vec3) -> bool {
    m.active().iter().any(|p| (p.point - at).length() < 2.0e-3)
}

fn ridge_pair(turn_a: f32, turn_b: f32, shift: Vec3) -> (Placed3, Placed3) {
    let a = cuboid([0.5, 0.5, 0.5], [0.0, 0.0, 0.0], about(Vec3::X, turn_a));
    let b = cuboid(
        [0.5, 0.5, 0.5],
        [shift.x, 1.3 + shift.y, shift.z],
        about(Vec3::Z, turn_b),
    );
    (a, b)
}

#[test]
fn an_edge_contact_is_halfway_between_the_nearest_points_of_the_two_edges() {
    use std::f32::consts::FRAC_PI_4;
    let ridge = 0.5 * std::f32::consts::SQRT_2;
    for (turn_a, turn_b) in [
        (FRAC_PI_4, FRAC_PI_4),
        (-FRAC_PI_4, FRAC_PI_4),
        (FRAC_PI_4, -FRAC_PI_4),
    ] {
        // The ridge of the first runs along x at z = 0, the one of the second along z at x = 0.2.
        let (a, b) = ridge_pair(turn_a, turn_b, Vec3::new(0.2, 0.0, -0.1));
        let depth = ridge - (1.3 - ridge);
        let at = Vec3::new(0.2, (ridge + 1.3 - ridge) / 2.0, 0.0);
        for (first, second, flip) in [(&a, &b, 1.0), (&b, &a, -1.0)] {
            let m = must(collide(first, second));
            assert_eq!(m.active().len(), 1);
            assert!(
                near(m.active()[0].separation, -depth),
                "{}",
                m.active()[0].separation
            );
            assert!(
                has_point(&m, at),
                "{:?} not near {at:?}",
                m.active()[0].point
            );
            assert!(near(m.normal.y * flip, 1.0), "{:?}", m.normal);
        }
    }
}

#[test]
fn a_face_contact_is_clipped_to_the_reference_face() {
    let a = cuboid([1.0, 0.5, 1.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = cuboid([0.5, 0.5, 0.5], [0.9, 0.95, 0.2], Quat::IDENTITY);
    for (first, second) in [(&a, &b), (&b, &a)] {
        let m = must(collide(first, second));
        assert_eq!(m.active().len(), 4);
        // The overlap in x is 0.4 to 1.0 and in z -0.3 to 0.7, halfway between the two faces.
        for (x, z) in [(0.4, -0.3), (1.0, -0.3), (1.0, 0.7), (0.4, 0.7)] {
            assert!(
                has_point(&m, Vec3::new(x, 0.475, z)),
                "no point at ({x}, {z})"
            );
        }
    }
}

#[test]
fn eight_clipped_corners_are_cut_down_to_four_spread_over_the_face() {
    let a = cuboid([1.0, 0.5, 1.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    let b = cuboid(
        [0.9, 0.5, 0.9],
        [0.0, 0.95, 0.0],
        about(Vec3::Y, std::f32::consts::FRAC_PI_4),
    );
    let m = must(collide(&a, &b));
    assert_eq!(m.active().len(), 4);
    let points: Vec<Vec3> = m.active().iter().map(|p| p.point).collect();
    let centroid = points.iter().fold(Vec3::ZERO, |sum, p| sum + *p) * 0.25;
    assert!(
        (centroid - Vec3::new(0.0, 0.475, 0.0)).length() < 1.0e-3,
        "{centroid:?}"
    );
    for (i, p) in points.iter().enumerate() {
        assert!(
            near(p.y, 0.475) && p.x.abs() <= 1.001 && p.z.abs() <= 1.001,
            "{p:?}"
        );
        for q in &points[i + 1..] {
            assert!((*p - *q).length() > 0.9, "points too close: {p:?} {q:?}");
        }
    }
    for p in m.active() {
        assert!(near(p.separation, -0.05));
    }
}

#[test]
fn contact_points_sit_halfway_between_the_surfaces() {
    // A ball sunk 0.02 into the ground: the ground's surface is y = 0, the ball's y = -0.02.
    let ground = cuboid([5.0, 0.5, 5.0], [0.0, -0.5, 0.0], Quat::IDENTITY);
    let m = must(collide(&ground, &sphere(0.5, 1.0, 0.48, 2.0)));
    assert!(
        has_point(&m, Vec3::new(1.0, -0.01, 2.0)),
        "{:?}",
        m.active()[0].point
    );
    // A capsule lying on it has both points at that height, at the two ends.
    let lying = capsule(
        1.0,
        0.5,
        [0.0, 0.48, 0.0],
        about(Vec3::Z, std::f32::consts::FRAC_PI_2),
    );
    let l = must(collide(&ground, &lying));
    assert!(
        has_point(&l, Vec3::new(-1.0, -0.01, 0.0)) && has_point(&l, Vec3::new(1.0, -0.01, 0.0))
    );
}

#[test]
fn a_segment_through_a_box_gets_points_at_its_deepest_ends() {
    let cube = cuboid([1.0, 1.0, 1.0], [0.0, 0.0, 0.0], Quat::IDENTITY);
    // Straight up through the box 0.5 from its x face: both ends are as deep.
    let upright = capsule(1.0, 0.25, [0.5, 0.2, 0.0], Quat::IDENTITY);
    for (first, second, flip) in [(&cube, &upright, 1.0), (&upright, &cube, -1.0)] {
        let m = must(collide(first, second));
        assert_eq!(m.active().len(), 2);
        assert!(near(m.normal.x * flip, 1.0), "{:?}", m.normal);
        for y in [-0.8, 1.2] {
            assert!(
                has_point(&m, Vec3::new(0.625, y, 0.0)),
                "no point at y = {y}"
            );
        }
        assert!(near(min_separation(&m), -0.75));
    }
    // Tilted: one end is deeper by more than the tolerance and is the only point.
    let tilted = capsule(0.4, 0.25, [0.6, 0.0, 0.0], about(Vec3::Z, 0.5));
    let t = must(collide(&cube, &tilted));
    assert_eq!(t.active().len(), 1);
    let (sin, cos) = (base::dmath::sin(0.5), base::dmath::cos(0.5));
    assert!(
        has_point(&t, Vec3::new(0.6 - 0.4 * sin + 0.171, 0.4 * cos, 0.0)),
        "{:?}",
        t.active()[0].point
    );
    // A sphere gets one point, halfway between the box face and its own surface.
    let s = must(collide(&cube, &sphere(0.25, 0.8, 0.0, 0.0)));
    assert_eq!(s.active().len(), 1);
    assert!(
        has_point(&s, Vec3::new(0.775, 0.0, 0.0)),
        "{:?}",
        s.active()[0].point
    );
}

#[test]
fn an_edge_contact_of_two_slanted_edges_is_where_the_lines_are_nearest() {
    use std::f32::consts::FRAC_PI_4;
    // The first box's top ridge runs along x at height 0.7071, z = 0. The second box's lowest ridge
    // runs along (sin t, 0, cos t) at height 1.3 - 0.7071 through the point (cx, cz): the angle
    // between the two lines is not a right angle.
    let ridge = 0.5 * std::f32::consts::SQRT_2;
    let turn = 0.5_f32;
    let (cx, cz) = (0.3_f32, 0.2_f32);
    let a = cuboid([0.5, 0.5, 0.5], [0.0, 0.0, 0.0], about(Vec3::X, FRAC_PI_4));
    let b = cuboid(
        [0.5, 0.5, 0.5],
        [cx, 1.3, cz],
        about(Vec3::Y, turn) * about(Vec3::Z, FRAC_PI_4),
    );
    // Where the second line crosses z = 0 (seen from above).
    let crossing = Vec3::new(
        cx - cz * base::dmath::sin(turn) / base::dmath::cos(turn),
        0.0,
        0.0,
    );
    let at = Vec3::new(crossing.x, (ridge + 1.3 - ridge) / 2.0, 0.0);
    for (first, second, flip) in [(&a, &b, 1.0), (&b, &a, -1.0)] {
        let m = must(collide(first, second));
        assert_eq!(m.active().len(), 1);
        assert!(
            has_point(&m, at),
            "{:?} not near {at:?}",
            m.active()[0].point
        );
        assert!(near(m.active()[0].separation, -(2.0 * ridge - 1.3)));
        assert!(near(m.normal.y * flip, 1.0), "{:?}", m.normal);
    }
}

/// The points of the two lines `start + along * dir` that are nearest each other.
fn nearest_on_lines(first: (Vec3, Vec3), second: (Vec3, Vec3)) -> (Vec3, Vec3) {
    let ((start_a, dir_a), (start_b, dir_b)) = (first, second);
    let offset = start_a - start_b;
    let (aa, ab, bb) = (dir_a.dot(dir_a), dir_a.dot(dir_b), dir_b.dot(dir_b));
    let (on_a, on_b) = (dir_a.dot(offset), dir_b.dot(offset));
    let denominator = aa * bb - ab * ab;
    let along_a = (ab * on_b - bb * on_a) / denominator;
    let along_b = (aa * on_b - ab * on_a) / denominator;
    (start_a + dir_a * along_a, start_b + dir_b * along_b)
}

#[test]
fn an_edge_contact_between_boxes_of_different_sizes_is_between_their_own_ridges() {
    use std::f32::consts::FRAC_PI_4;
    let (half_a, half_b) = (Vec3::new(0.5, 0.8, 0.3), Vec3::new(0.5, 0.5, 0.45));
    let turn_a = about(Vec3::X, FRAC_PI_4);
    let turn_b = about(Vec3::Y, 0.5) * about(Vec3::Z, FRAC_PI_4);
    // The first box's top ridge (local y high, z low) and the second's lowest (x and y low).
    let ridge_a = turn_a.rotate(Vec3::new(0.0, half_a.y, -half_a.z));
    let ridge_b = turn_b.rotate(Vec3::new(-half_b.x, -half_b.y, 0.0));
    let centre_b = Vec3::new(0.3, 1.385, 0.2);
    let a = cuboid([half_a.x, half_a.y, half_a.z], [0.0, 0.0, 0.0], turn_a);
    let b = cuboid(
        [half_b.x, half_b.y, half_b.z],
        [centre_b.x, centre_b.y, centre_b.z],
        turn_b,
    );
    let (on_a, on_b) = nearest_on_lines(
        (ridge_a, turn_a.rotate(Vec3::X)),
        (centre_b + ridge_b, turn_b.rotate(Vec3::Z)),
    );
    let at = (on_a + on_b) * 0.5;
    for (first, second) in [(&a, &b), (&b, &a)] {
        let m = must(collide(first, second));
        assert_eq!(m.active().len(), 1);
        assert!(
            has_point(&m, at),
            "{:?} not near {at:?}",
            m.active()[0].point
        );
        assert!(
            near(m.active()[0].separation, -(on_a.y - on_b.y)),
            "{}",
            m.active()[0].separation
        );
    }
}

// ---- convex hulls --------------------------------------------------------------------------

/// The largest dot of `n` with a corner of the hull, the corners turned by `axes`.
fn hull_extent(hull: &Hull, axes: &[V; 3], n: V) -> f64 {
    let along = |corner: &Vec3| {
        let local = wide(*corner);
        (0..3).map(|k| dot(n, axes[k]) * local[k]).sum::<f64>()
    };
    hull.vertices().iter().map(along).fold(f64::MIN, f64::max)
}

fn hull_of(points: &[Vec3]) -> Shape3 {
    match Hull::new(points) {
        Ok(hull) => Shape3::Hull(hull),
        Err(error) => panic!("the points should make a hull: {error:?}"),
    }
}

fn cube_corners(half: f32) -> Vec<Vec3> {
    (0..8)
        .map(|i| {
            let side = |bit: usize| if (i >> bit) & 1 == 1 { half } else { -half };
            Vec3::new(side(0), side(1), side(2))
        })
        .collect()
}

fn hulled(points: &[Vec3], at: [f32; 3], rotation: Quat) -> Placed3 {
    Placed3::new(hull_of(points), Vec3::new(at[0], at[1], at[2]), rotation)
}

#[test]
fn a_sphere_pressed_into_a_hull_face_gets_the_face_normal_and_depth() {
    let cube = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let ball = sphere(0.5, 1.4, 0.2, -0.3);
    let m = must(collide(&cube, &ball));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert_eq!(m.active().len(), 1);
    assert!(
        near(m.active()[0].separation, -0.1),
        "{}",
        m.active()[0].separation
    );
    assert!(
        has_point(&m, Vec3::new(0.95, 0.2, -0.3)),
        "{:?}",
        m.active()[0].point
    );
    let back = must(collide(&ball, &cube));
    assert!(near_vec(back.normal, -Vec3::X) && near(back.active()[0].separation, -0.1));
}

#[test]
fn two_hull_cubes_overlapping_along_x_get_the_x_axis_and_the_overlap() {
    let first = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let second = hulled(&cube_corners(1.0), [1.7, 0.3, 0.2], Quat::IDENTITY);
    let m = must(collide(&first, &second));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert!(near(min_separation(&m), -0.3), "{}", min_separation(&m));
    let back = must(collide(&second, &first));
    assert!(near_vec(back.normal, -Vec3::X) && near(min_separation(&back), -0.3));
}

#[test]
fn a_hull_of_a_boxs_corners_touches_a_box_as_the_box_does() {
    let turn = about(Vec3::new(0.0, 0.6, 0.8), 0.4);
    let as_hull = hulled(&cube_corners(0.5), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let as_box = cuboid([0.5, 0.5, 0.5], [0.0, 0.0, 0.0], Quat::IDENTITY);
    let other = cuboid([0.4, 0.3, 0.6], [0.7, 0.1, 0.2], turn);
    let (via_hull, via_box) = (
        must(collide(&as_hull, &other)),
        must(collide(&as_box, &other)),
    );
    assert!(
        near_vec(via_hull.normal, via_box.normal),
        "{:?} {:?}",
        via_hull.normal,
        via_box.normal
    );
    assert!(
        near(min_separation(&via_hull), min_separation(&via_box)),
        "{} {}",
        min_separation(&via_hull),
        min_separation(&via_box)
    );
}

#[test]
fn a_hull_against_a_capsule_uses_the_capsules_axis_and_radius() {
    let cube = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    // Standing beside the cube, 0.2 into it, tall enough to pass its top.
    let post = capsule(2.0, 0.5, [1.3, 0.0, 0.0], Quat::IDENTITY);
    let m = must(collide(&cube, &post));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert!(near(min_separation(&m), -0.2), "{}", min_separation(&m));
}

#[test]
fn a_hull_within_the_margin_touches_speculatively_and_beyond_it_does_not() {
    let cube = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let near_ball = sphere(0.5, 1.55, 0.0, 0.0);
    let far_ball = sphere(0.5, 1.7, 0.0, 0.0);
    let m = must(collide(&cube, &near_ball));
    assert!(near(min_separation(&m), 0.05), "{}", min_separation(&m));
    assert!(collide(&cube, &far_ball).is_none());
}

#[test]
fn a_hull_with_the_same_centre_as_another_shape_gets_a_unit_normal() {
    let cube = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let inside = sphere(0.25, 0.0, 0.0, 0.0);
    let m = must(collide(&cube, &inside));
    assert!(near(m.normal.length(), 1.0), "{:?}", m.normal);
    assert!(near(min_separation(&m), -1.25), "{}", min_separation(&m));
}

#[test]
fn the_oracle_agrees_for_random_hulls_against_every_shape() {
    let dirs = directions(20_000);
    for kind_other in [0, 1, 2, 3] {
        let mut rng = Pcg32::new(0x8_5, kind_other as u64);
        let (mut touching, mut apart) = (0, 0);
        for _ in 0..120 {
            let (a, b) = (
                random_placed(&mut rng, 3),
                random_placed(&mut rng, kind_other),
            );
            let verdict = check_scene(&a, &b, &dirs, 4.0e-3);
            touching += usize::from(matches!(verdict, Verdict::Touching));
            apart += usize::from(matches!(verdict, Verdict::Apart));
        }
        assert!(
            touching > 20 && apart > 5,
            "hull against kind {kind_other}: {touching} touching, {apart} apart"
        );
    }
}

/// Checks a hull's answer against the box routines' answer for the same shape; `true` when both
/// found a contact, `false` when neither did (or only the box did, by its early diagonal gap).
fn compare_hull_and_box(kind: usize, hull: Option<Manifold3>, boxed: Option<Manifold3>) -> bool {
    match (hull, boxed) {
        (Some(via_hull), Some(via_box)) => {
            let (h, b) = (min_separation(&via_hull), min_separation(&via_box));
            assert!((h - b).abs() < 6.0e-3, "kind {kind}: hull {h}, box {b}");
            assert!(near(via_hull.normal.length(), 1.0));
            true
        }
        (None, None) => false,
        // The box routine reports a diagonal gap early (a documented limit of card 8-4).
        (None, Some(boxed)) if min_separation(&boxed) > MARGIN / 2.0 => false,
        (hull, boxed) => panic!(
            "kind {kind}: hull {:?}, box {:?}",
            hull.map(|m| min_separation(&m)),
            boxed.map(|m| min_separation(&m))
        ),
    }
}

#[test]
fn a_hull_of_a_boxs_corners_agrees_with_the_box_routines_in_deep_and_shallow_overlaps() {
    let half = [0.5, 0.35, 0.6];
    let corners: Vec<Vec3> = (0..8)
        .map(|i| {
            let side = |bit: usize, size: f32| if (i >> bit) & 1 == 1 { size } else { -size };
            Vec3::new(side(0, half[0]), side(1, half[1]), side(2, half[2]))
        })
        .collect();
    for kind in 0..3 {
        let mut rng = Pcg32::new(0x8_5_5, kind as u64);
        let (mut both, mut neither) = (0, 0);
        for _ in 0..300 {
            let other = random_placed(&mut rng, kind);
            let turn = random_unit_quat(&mut rng);
            let at = other.position * -0.5;
            let as_box = cuboid(half, [at.x, at.y, at.z], turn);
            let as_hull = hulled(&corners, [at.x, at.y, at.z], turn);
            let found =
                compare_hull_and_box(kind, collide(&as_hull, &other), collide(&as_box, &other));
            both += usize::from(found);
            neither += usize::from(!found);
        }
        assert!(both > 60, "kind {kind}: {both} overlaps, {neither} apart");
    }
}

#[test]
fn hulls_that_touch_exactly_by_a_face_an_edge_or_a_corner_get_no_depth_and_a_unit_normal() {
    let cube = cube_corners(1.0);
    let first = hulled(&cube, [0.0, 0.0, 0.0], Quat::IDENTITY);
    for (offset, name) in [
        ([2.0, 0.0, 0.0], "face"),
        ([2.0, 0.5, -0.25], "face, shifted"),
        ([2.0, 2.0, 0.0], "edge"),
        ([2.0, 2.0, 2.0], "corner"),
        ([0.0, -2.0, 0.0], "face, below"),
    ] {
        let second = hulled(&cube, offset, Quat::IDENTITY);
        let m = must(collide(&first, &second));
        assert!(near(m.normal.length(), 1.0), "{name}: {:?}", m.normal);
        assert!(
            min_separation(&m).abs() < 1.0e-3,
            "{name}: {}",
            min_separation(&m)
        );
        // The normal points from the first cube towards the second.
        let towards = Vec3::new(offset[0], offset[1], offset[2]);
        assert!(m.normal.dot(towards) > 0.0, "{name}: {:?}", m.normal);
        let back = must(collide(&second, &first));
        assert!(
            back.normal.dot(towards) < 0.0,
            "{name} reversed: {:?}",
            back.normal
        );
    }
}

#[test]
fn a_face_touch_has_the_face_normal_and_a_capsule_or_box_can_touch_a_hull_too() {
    let cube = cube_corners(1.0);
    let first = hulled(&cube, [0.0, 0.0, 0.0], Quat::IDENTITY);
    let touching_box = cuboid([1.0, 1.0, 1.0], [2.0, 0.0, 0.0], Quat::IDENTITY);
    let m = must(collide(&first, &touching_box));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert!(min_separation(&m).abs() < 1.0e-3);
    // A capsule axis ends on the face of the cube: its round end then touches the face.
    let post = capsule(0.5, 0.25, [0.0, 1.75, 0.0], Quat::IDENTITY);
    let m = must(collide(&first, &post));
    assert!(near_vec(m.normal, Vec3::Y), "{:?}", m.normal);
    assert!(min_separation(&m).abs() < 1.0e-3, "{}", min_separation(&m));
}

#[test]
fn a_hull_exactly_at_the_margin_still_touches_and_a_hair_beyond_does_not() {
    // All values are exact in binary: the gap is 1.625 - 1.0 - 0.5 = 0.125, the margin 0.125.
    let cube = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let at_margin = sphere(0.5, 1.625, 0.0, 0.0);
    let beyond = sphere(0.5, 1.6251, 0.0, 0.0);
    let Some(m) = D3::collide(&cube, &at_margin, 0.125) else {
        panic!("a gap equal to the margin is within it");
    };
    assert!(near(min_separation(&m), 0.125), "{}", min_separation(&m));
    assert!(D3::collide(&cube, &beyond, 0.125).is_none());
}

#[test]
fn a_sphere_centre_inside_a_hull_has_its_point_halfway_between_the_two_surfaces() {
    let cube = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    // The ball's far side is at x = 0.25 inside the cube, the cube's face at x = 1: halfway is 0.625.
    let ball = sphere(0.25, 0.5, 0.0, 0.0);
    let m = must(collide(&cube, &ball));
    assert!(near_vec(m.normal, Vec3::X), "{:?}", m.normal);
    assert!(near(min_separation(&m), -0.75), "{}", min_separation(&m));
    assert!(
        has_point(&m, Vec3::new(0.625, 0.0, 0.0)),
        "{:?}",
        m.active()[0].point
    );
    let back = must(collide(&ball, &cube));
    assert!(
        has_point(&back, Vec3::new(0.625, 0.0, 0.0)),
        "{:?}",
        back.active()[0].point
    );
}

#[test]
fn overlapping_hull_cubes_have_their_point_between_the_faces_inside_the_overlap() {
    let first = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    let second = hulled(&cube_corners(1.0), [1.7, 0.3, 0.2], Quat::IDENTITY);
    let m = must(collide(&first, &second));
    let point = m.active()[0].point;
    // The faces are at x = 1 and x = 0.7; the overlap in y is [-0.7, 1], in z [-0.8, 1].
    assert!(near(point.x, 0.85), "{point:?}");
    assert!(
        (-0.7..=1.0).contains(&point.y) && (-0.8..=1.0).contains(&point.z),
        "{point:?}"
    );
}

#[test]
fn a_hull_contact_point_stays_inside_a_small_overlap_and_halfway_between_the_faces() {
    let first = hulled(&cube_corners(1.0), [0.0, 0.0, 0.0], Quat::IDENTITY);
    // The overlap is 0.2 in x, 0.1 in y and 0.05 in z: the shallowest axis is z.
    let second = hulled(&cube_corners(1.0), [1.8, 1.9, 1.95], Quat::IDENTITY);
    let m = must(collide(&first, &second));
    assert!(near_vec(m.normal, Vec3::Z), "{:?}", m.normal);
    assert!(near(min_separation(&m), -0.05), "{}", min_separation(&m));
    // Card 8-18: the face contact has a point at each corner of the overlap, so a point can lie on
    // the edge of the range (to within rounding).
    let slack = 1.0e-5;
    for contact in m.active() {
        let point = contact.point;
        assert!(near(point.z, 0.975), "{point:?}");
        assert!(
            (0.8 - slack..=1.0 + slack).contains(&point.x)
                && (0.9 - slack..=1.0 + slack).contains(&point.y),
            "{point:?}"
        );
    }
}

#[test]
fn a_sphere_centre_inside_a_tetrahedron_gets_the_nearest_face_and_the_exact_point() {
    let corners = [
        Vec3::ZERO,
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 3.0, 0.0),
        Vec3::new(0.0, 0.0, 6.0),
    ];
    let Shape3::Hull(hull) = hull_of(&corners) else {
        panic!("a hull");
    };
    // The body sits where the points were given: its origin and orientation put the hull back.
    let solid = Placed3::new(Shape3::Hull(hull), hull.origin(), hull.orientation());
    let sqrt14 = 14.0_f32.sqrt();
    for (centre, normal, depth) in [
        (Vec3::new(0.4, 0.5, 0.8), -Vec3::X, 0.4),
        (
            Vec3::new(1.0, 0.8, 1.2),
            Vec3::new(3.0, 2.0, 1.0) * (1.0 / sqrt14),
            0.2 / sqrt14,
        ),
    ] {
        let radius = 0.01;
        let ball = sphere(radius, centre.x, centre.y, centre.z);
        let m = must(collide(&solid, &ball));
        assert!(
            near_vec(m.normal, normal),
            "{:?} wanted {normal:?}",
            m.normal
        );
        assert!(
            near(min_separation(&m), -(depth + radius)),
            "{}",
            min_separation(&m)
        );
        let wanted = centre + normal * ((depth - radius) * 0.5);
        assert!(
            has_point(&m, wanted),
            "{:?} wanted {wanted:?}",
            m.active()[0].point
        );
    }
}

#[test]
fn hulls_with_crossing_ridges_that_are_apart_have_their_point_between_the_nearest_ridge_points() {
    use std::f32::consts::FRAC_PI_4;
    let ridge = 0.5 * std::f32::consts::SQRT_2;
    let corners = cube_corners(0.5);
    for (turn_a, turn_b) in [
        (FRAC_PI_4, FRAC_PI_4),
        (-FRAC_PI_4, FRAC_PI_4),
        (FRAC_PI_4, -FRAC_PI_4),
    ] {
        // The ridge of the first runs along x at z = 0, the one of the second along z at x = 0.2.
        let a = hulled(&corners, [0.0, 0.0, 0.0], about(Vec3::X, turn_a));
        let b = hulled(&corners, [0.2, 1.45, -0.1], about(Vec3::Z, turn_b));
        let gap = 1.45 - 2.0 * ridge;
        let at = Vec3::new(0.2, ridge + gap / 2.0, 0.0);
        for (first, second, flip) in [(&a, &b, 1.0), (&b, &a, -1.0)] {
            let m = must(collide(first, second));
            assert!(near(min_separation(&m), gap), "{}", min_separation(&m));
            assert!(
                has_point(&m, at),
                "{:?} not near {at:?}",
                m.active()[0].point
            );
            assert!(near(m.normal.y * flip, 1.0), "{:?}", m.normal);
        }
    }
}
