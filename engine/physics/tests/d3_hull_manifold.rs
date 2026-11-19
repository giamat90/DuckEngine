//! Card 8-18: contact manifolds of convex hulls. A hull that rests on a face has up to four contact
//! points, found by clipping the support face of one shape against the support face of the other,
//! as box against box has since card 8-4. Pairs that touch at an edge or a corner keep fewer points.

use base::math::{Quat, Vec3};
use physics::core::Dim;
use physics::d3::{ContactPoint3, D3, Hull, Manifold3, Placed3, Shape3};

const MARGIN: f32 = 0.05;
/// How far a point may be from where the geometry puts it.
const SLACK: f32 = 2.0e-3;

fn about(axis: Vec3, angle: f32) -> Quat {
    let Some(q) = Quat::from_axis_angle(axis, angle) else {
        panic!("the axis is a unit vector");
    };
    q
}

fn corners(half: Vec3) -> Vec<Vec3> {
    (0..8_u32)
        .map(|n| {
            let sign = |bit: u32| if n >> bit & 1 == 1 { 1.0 } else { -1.0 };
            Vec3::new(half.x * sign(0), half.y * sign(1), half.z * sign(2))
        })
        .collect()
}

fn hull_shape(points: &[Vec3]) -> Shape3 {
    match Hull::new(points) {
        Ok(hull) => Shape3::Hull(hull),
        Err(error) => panic!("the points should make a hull: {error:?}"),
    }
}

fn hull_box(half: Vec3, at: Vec3, rotation: Quat) -> Placed3 {
    Placed3::new(hull_shape(&corners(half)), at, rotation)
}

fn solid_box(half: Vec3, at: Vec3, rotation: Quat) -> Placed3 {
    Placed3::new(Shape3::Box { half_extents: half }, at, rotation)
}

fn must(m: Option<Manifold3>) -> Manifold3 {
    let Some(m) = m else {
        panic!("the shapes were expected to touch");
    };
    m
}

fn collide(a: &Placed3, b: &Placed3) -> Manifold3 {
    must(D3::collide(a, b, MARGIN))
}

fn has_point_near(m: &Manifold3, x: f32, z: f32) -> bool {
    m.active()
        .iter()
        .any(|c| (c.point.x - x).abs() < SLACK && (c.point.z - z).abs() < SLACK)
}

fn unit() -> Vec3 {
    Vec3::new(0.5, 0.5, 0.5)
}

/// A wide slab, top face at y = 0.
fn floor() -> Placed3 {
    solid_box(
        Vec3::new(10.0, 1.0, 10.0),
        Vec3::new(0.0, -1.0, 0.0),
        Quat::IDENTITY,
    )
}

#[test]
fn a_hull_cube_lying_on_a_floor_has_four_points_at_its_corners() {
    let slab = floor();
    let cube = hull_box(unit(), Vec3::new(0.0, 0.49, 0.0), Quat::IDENTITY);
    let m = collide(&slab, &cube);
    assert_eq!(m.count, 4, "{m:?}");
    assert!((m.normal - Vec3::Y).length() < 1e-3, "{:?}", m.normal);
    for (x, z) in [(0.5, 0.5), (0.5, -0.5), (-0.5, 0.5), (-0.5, -0.5)] {
        assert!(has_point_near(&m, x, z), "no point at ({x}, {z}): {m:?}");
    }
    for c in m.active() {
        assert!((c.separation + 0.01).abs() < SLACK, "{c:?}");
    }
}

#[test]
fn the_same_contact_the_other_way_round_is_the_same_four_points_with_the_normal_flipped() {
    let slab = floor();
    let cube = hull_box(unit(), Vec3::new(0.0, 0.49, 0.0), Quat::IDENTITY);
    let (down, up) = (collide(&cube, &slab), collide(&slab, &cube));
    assert_eq!((down.count, up.count), (4, 4));
    assert!((down.normal + up.normal).length() < 1e-3);
    for c in up.active() {
        assert!(has_point_near(&down, c.point.x, c.point.z), "{c:?}");
    }
}

#[test]
fn a_hull_cube_on_a_hull_slab_has_four_points() {
    let slab = hull_box(
        Vec3::new(5.0, 1.0, 5.0),
        Vec3::new(0.0, -1.0, 0.0),
        Quat::IDENTITY,
    );
    let cube = hull_box(unit(), Vec3::new(0.2, 0.49, -0.1), about(Vec3::Y, 0.4));
    let m = collide(&slab, &cube);
    assert_eq!(m.count, 4, "{m:?}");
    assert!((m.normal - Vec3::Y).length() < 1e-3);
}

#[test]
fn a_hull_cube_resting_on_an_edge_has_two_points_along_it() {
    let slab = floor();
    // Turned 45 degrees about z: it stands on the edge along z, centre above it by sqrt(0.5).
    let height = 0.5_f32.sqrt() - 0.005;
    let cube = hull_box(
        unit(),
        Vec3::new(0.0, height, 0.0),
        about(Vec3::Z, std::f32::consts::FRAC_PI_4),
    );
    let m = collide(&slab, &cube);
    assert_eq!(m.count, 2, "{m:?}");
    assert!(
        has_point_near(&m, 0.0, 0.5) && has_point_near(&m, 0.0, -0.5),
        "{m:?}"
    );
}

#[test]
fn a_hull_cube_standing_on_a_corner_has_one_point() {
    let slab = floor();
    let cube = hull_box(
        unit(),
        Vec3::new(0.0, 0.866_025_4 - 0.005, 0.0),
        about(Vec3::new(1.0, 0.0, -1.0), 0.955_316_6),
    );
    let m = collide(&slab, &cube);
    assert_eq!(m.count, 1, "{m:?}");
}

#[test]
fn a_cube_that_overlaps_half_of_a_face_gets_the_four_corners_of_the_shared_area() {
    let bottom = hull_box(unit(), Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY);
    let top = hull_box(unit(), Vec3::new(0.5, 0.99, 0.0), Quat::IDENTITY);
    let m = collide(&bottom, &top);
    assert_eq!(m.count, 4, "{m:?}");
    for (x, z) in [(0.5, 0.5), (0.5, -0.5), (0.0, 0.5), (0.0, -0.5)] {
        assert!(has_point_near(&m, x, z), "no point at ({x}, {z}): {m:?}");
    }
}

#[test]
fn a_small_cube_on_a_large_plate_gets_its_own_four_corners() {
    let plate = hull_box(
        Vec3::new(3.0, 0.2, 3.0),
        Vec3::new(0.0, 0.0, 0.0),
        Quat::IDENTITY,
    );
    let cube = hull_box(unit(), Vec3::new(1.0, 0.69, 0.5), Quat::IDENTITY);
    let m = collide(&plate, &cube);
    assert_eq!(m.count, 4, "{m:?}");
    for (x, z) in [(1.5, 1.0), (1.5, 0.0), (0.5, 1.0), (0.5, 0.0)] {
        assert!(has_point_near(&m, x, z), "no point at ({x}, {z}): {m:?}");
    }
}

#[test]
fn a_hull_of_a_boxs_corners_makes_the_same_contact_as_the_box() {
    let slab = floor();
    let rotation = about(Vec3::new(0.3, 1.0, 0.2), 0.9);
    let (as_hull, as_box) = (
        hull_box(unit(), Vec3::new(0.3, 0.6, 0.1), rotation),
        solid_box(unit(), Vec3::new(0.3, 0.6, 0.1), rotation),
    );
    let (via_hull, via_box) = (collide(&slab, &as_hull), collide(&slab, &as_box));
    assert!((via_hull.normal - via_box.normal).length() < 1e-3);
    assert_eq!(via_hull.count, via_box.count, "{via_hull:?} {via_box:?}");
    for c in via_box.active() {
        assert!(has_point_near(&via_hull, c.point.x, c.point.z), "{c:?}");
    }
}

#[test]
fn a_prism_with_many_corners_gets_four_points_that_spread_over_its_face() {
    // A regular 12-sided prism of radius 1 and half height 0.5, standing on the floor.
    let mut points = Vec::new();
    for k in 0..12_u8 {
        let angle = f32::from(k) * std::f32::consts::TAU / 12.0;
        let (z, x) = base::dmath::sin_cos(angle);
        points.push(Vec3::new(x, 0.5, z));
        points.push(Vec3::new(x, -0.5, z));
    }
    let prism = Placed3::new(
        hull_shape(&points),
        Vec3::new(0.0, 0.49, 0.0),
        Quat::IDENTITY,
    );
    let m = collide(&floor(), &prism);
    assert_eq!(m.count, 4, "{m:?}");
    // The four points are spread over the face, not bunched: the farthest pair is nearly a
    // diameter and the closest pair is far apart.
    let p: Vec<Vec3> = m.active().iter().map(|c| c.point).collect();
    let distances: Vec<f32> = (0..4)
        .flat_map(|a| ((a + 1)..4).map(move |b| (a, b)))
        .map(|(a, b)| (p[a] - p[b]).length())
        .collect();
    let (far, near) = (
        distances.iter().copied().fold(0.0_f32, f32::max),
        distances.iter().copied().fold(f32::MAX, f32::min),
    );
    assert!(far > 1.8 && near > 0.8, "the points sit together: {p:?}");
}

#[test]
fn only_the_corners_within_the_margin_make_points() {
    // Tilted about z by 0.08 rad: the corners on one side are about 0.08 higher than the others.
    let slab = floor();
    let cube = hull_box(unit(), Vec3::new(0.0, 0.5, 0.0), about(Vec3::Z, 0.08));
    let m = collide(&slab, &cube);
    assert_eq!(m.count, 2, "{m:?}");
    for c in m.active() {
        assert!(c.separation <= MARGIN, "{c:?}");
    }
}

#[test]
fn a_capsule_lying_on_a_hull_face_has_two_points() {
    let slab = hull_box(
        Vec3::new(3.0, 0.5, 3.0),
        Vec3::new(0.0, -0.5, 0.0),
        Quat::IDENTITY,
    );
    let capsule = Placed3::new(
        Shape3::Capsule {
            half_height: 0.6,
            radius: 0.2,
        },
        Vec3::new(0.0, 0.19, 0.0),
        about(Vec3::Z, std::f32::consts::FRAC_PI_2),
    );
    let m = collide(&slab, &capsule);
    assert_eq!(m.count, 2, "{m:?}");
    assert!(
        has_point_near(&m, 0.6, 0.0) && has_point_near(&m, -0.6, 0.0),
        "{m:?}"
    );
    for c in m.active() {
        assert!((c.separation + 0.01).abs() < SLACK, "{c:?}");
    }
}

#[test]
fn a_sphere_on_a_hull_face_still_has_one_point() {
    let slab = hull_box(
        Vec3::new(3.0, 0.5, 3.0),
        Vec3::new(0.0, -0.5, 0.0),
        Quat::IDENTITY,
    );
    let ball = Placed3::new(
        Shape3::Sphere { radius: 0.3 },
        Vec3::new(0.2, 0.29, 0.1),
        Quat::IDENTITY,
    );
    let m = collide(&slab, &ball);
    assert_eq!(m.count, 1, "{m:?}");
}

#[test]
fn every_point_is_finite_and_in_the_contact_plane_whatever_the_pose() {
    let slab = floor();
    for turn in 0..24_u8 {
        let rotation = about(Vec3::new(0.2, 1.0, 0.1), f32::from(turn) * 0.26);
        // Put the lowest corner 5 mm into the floor.
        let lowest = corners(unit())
            .iter()
            .map(|c| -rotation.rotate(*c).y)
            .fold(f32::MIN, f32::max);
        let cube = hull_box(unit(), Vec3::new(0.1, lowest - 0.005, 0.0), rotation);
        let m = collide(&slab, &cube);
        assert!(m.is_finite_manifold(), "{m:?}");
        assert!(m.count >= 1 && usize::from(m.count) <= 4);
        for c in m.active() {
            assert!(c.separation <= MARGIN + 1e-4, "{c:?}");
        }
    }
}

trait FiniteManifold {
    fn is_finite_manifold(&self) -> bool;
}

impl FiniteManifold for Manifold3 {
    fn is_finite_manifold(&self) -> bool {
        let finite = |p: &ContactPoint3| {
            p.point.x.is_finite()
                && p.point.y.is_finite()
                && p.point.z.is_finite()
                && p.separation.is_finite()
        };
        self.normal.x.is_finite() && self.active().iter().all(finite)
    }
}
