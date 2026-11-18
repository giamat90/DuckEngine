//! Card 8-5, part 1: a convex hull is a shape like the others: it is built from points, it has a
//! mass and inertia, bounds, a hash and a validity check. Contacts come in the second part.

use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use physics::core::{Dim, HashBits, PhysicsError, StateHasher};
use physics::d3::{D3, Hull, MAX_HULL_VERTICES, Shape3};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0e-4 * b.abs().max(1.0)
}

fn near_vec(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1.0e-4
}

fn about(axis: Vec3, angle: f32) -> Quat {
    let Some(q) = Quat::from_axis_angle(axis, angle) else {
        panic!("the axis is a unit vector");
    };
    q
}

/// The eight corners of a box with half sizes `half`, centred at `centre`, turned by `turn`.
fn box_corners(half: Vec3, centre: Vec3, turn: Quat) -> Vec<Vec3> {
    (0..8)
        .map(|i| {
            let side = |bit: usize| if (i >> bit) & 1 == 1 { 1.0 } else { -1.0 };
            centre
                + turn.rotate(Vec3::new(
                    side(0) * half.x,
                    side(1) * half.y,
                    side(2) * half.z,
                ))
        })
        .collect()
}

fn built(points: &[Vec3]) -> Hull {
    match Hull::new(points) {
        Ok(hull) => hull,
        Err(error) => panic!("the points should make a hull: {error:?}"),
    }
}

fn hash_of(shape: &Shape3) -> u64 {
    let mut hasher = StateHasher::new();
    shape.write_bits(&mut hasher);
    hasher.finish()
}

/// A small index as a float.
fn index(i: usize) -> f32 {
    f32::from(u8::try_from(i).unwrap_or(u8::MAX))
}

fn sorted(v: Vec3) -> [f32; 3] {
    let mut a = [v.x, v.y, v.z];
    a.sort_by(f32::total_cmp);
    a
}

#[test]
fn a_hull_of_a_boxs_corners_has_the_mass_and_inertia_of_the_box() {
    let half = Vec3::new(0.5, 1.0, 1.5);
    let hull = Shape3::Hull(built(&box_corners(half, Vec3::ZERO, Quat::IDENTITY)));
    let reference = Shape3::Box { half_extents: half };
    for density in [1.0, 250.0] {
        let (Ok(a), Ok(b)) = (
            D3::mass_properties(&hull, density),
            D3::mass_properties(&reference, density),
        ) else {
            panic!("both shapes have a mass");
        };
        assert!(near(a.mass, b.mass), "{} {}", a.mass, b.mass);
        let (ours, theirs) = (sorted(a.inv_inertia), sorted(b.inv_inertia));
        for (x, y) in ours.iter().zip(theirs) {
            assert!(near(*x, y), "{ours:?} {theirs:?}");
        }
    }
}

#[test]
fn a_hull_keeps_only_its_corners() {
    let half = Vec3::new(1.0, 1.0, 1.0);
    let mut points = box_corners(half, Vec3::ZERO, Quat::IDENTITY);
    // Inside, on a face, on an edge: none of them is a corner.
    points.extend([
        Vec3::ZERO,
        Vec3::new(0.2, -0.3, 0.1),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.5),
        Vec3::new(1.0, 1.0, 0.0),
    ]);
    let hull = built(&points);
    assert_eq!(hull.vertices().len(), 8);
    for corner in box_corners(half, Vec3::ZERO, Quat::IDENTITY) {
        assert!(
            hull.vertices()
                .iter()
                .any(|v| near_vec(hull.origin() + hull.orientation().rotate(*v), corner)),
            "corner {corner:?} was lost"
        );
    }
}

#[test]
fn a_tetrahedron_has_its_volume_and_its_centroid_as_origin() {
    let points = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 3.0, 0.0),
        Vec3::new(0.0, 0.0, 6.0),
    ];
    let hull = built(&points);
    assert!(
        near_vec(hull.origin(), Vec3::new(0.5, 0.75, 1.5)),
        "{:?}",
        hull.origin()
    );
    let Ok(mass) = D3::mass_properties(&Shape3::Hull(hull), 1.0) else {
        panic!("a tetrahedron has a mass");
    };
    // Volume: 2 * 3 * 6 / 6 = 6.
    assert!(near(mass.mass, 6.0), "{}", mass.mass);
}

#[test]
fn a_hull_is_centred_on_its_centre_of_mass_and_turned_to_its_principal_axes() {
    let half = Vec3::new(0.4, 0.9, 1.6);
    let centre = Vec3::new(3.0, -2.0, 5.0);
    let turn = about(Vec3::new(0.6, 0.0, 0.8), 0.9);
    let points = box_corners(half, centre, turn);
    let hull = built(&points);
    assert!(near_vec(hull.origin(), centre), "{:?}", hull.origin());
    // Its own points, put back where they were, are the points given.
    let back: Vec<Vec3> = hull
        .vertices()
        .iter()
        .map(|v| hull.origin() + hull.orientation().rotate(*v))
        .collect();
    for point in &points {
        assert!(
            back.iter().any(|b| near_vec(*b, *point)),
            "{point:?} not found"
        );
    }
    // The vertices sit on the principal axes of a box: each local coordinate is plus or minus a half size.
    let mut sizes = [0.0_f32; 3];
    for v in hull.vertices() {
        for (size, c) in sizes.iter_mut().zip([v.x, v.y, v.z]) {
            *size = size.max(c.abs());
        }
    }
    let wanted = sorted(half);
    let got = sorted(Vec3::new(sizes[0], sizes[1], sizes[2]));
    for (g, w) in got.iter().zip(wanted) {
        assert!(near(*g, w), "{got:?} {wanted:?}");
    }
}

#[test]
fn points_that_do_not_make_a_solid_are_refused() {
    let flat = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ];
    let line = [Vec3::ZERO, Vec3::X, Vec3::X * 2.0, Vec3::X * 3.0];
    let few = [Vec3::ZERO, Vec3::X, Vec3::Y];
    let same = [Vec3::ONE; 6];
    for (name, points) in [
        ("flat", &flat[..]),
        ("line", &line),
        ("few", &few),
        ("same", &same),
        ("none", &[]),
    ] {
        assert_eq!(
            Hull::new(points).err(),
            Some(PhysicsError::InvalidShape),
            "{name}"
        );
    }
}

#[test]
fn points_that_are_not_finite_are_refused() {
    let mut points = box_corners(Vec3::ONE, Vec3::ZERO, Quat::IDENTITY);
    points[3] = Vec3::new(f32::NAN, 0.0, 0.0);
    assert_eq!(Hull::new(&points).err(), Some(PhysicsError::NotFinite));
    points[3] = Vec3::new(0.0, f32::INFINITY, 0.0);
    assert_eq!(Hull::new(&points).err(), Some(PhysicsError::NotFinite));
}

#[test]
fn a_hull_has_at_most_the_vertex_limit() {
    // Points on a sphere are all corners of their hull.
    let ring = |count: usize| -> Vec<Vec3> {
        let golden = 2.399_963_2_f32;
        (0..count)
            .map(|i| {
                let y = 1.0 - 2.0 * (index(i) + 0.5) / index(count);
                let radius = (1.0 - y * y).sqrt();
                let angle = golden * index(i);
                Vec3::new(
                    radius * base::dmath::cos(angle),
                    y,
                    radius * base::dmath::sin(angle),
                )
            })
            .collect()
    };
    assert_eq!(MAX_HULL_VERTICES, 32);
    assert_eq!(
        built(&ring(MAX_HULL_VERTICES)).vertices().len(),
        MAX_HULL_VERTICES
    );
    assert_eq!(
        Hull::new(&ring(MAX_HULL_VERTICES + 1)).err(),
        Some(PhysicsError::InvalidShape)
    );
}

#[test]
fn the_bounds_of_a_hull_reach_its_extreme_points() {
    let points = box_corners(Vec3::new(0.5, 1.0, 1.5), Vec3::ZERO, Quat::IDENTITY);
    let hull = built(&points);
    let shape = Shape3::Hull(hull);
    let turn = about(Vec3::new(0.0, 0.6, 0.8), 0.7);
    let at = Vec3::new(4.0, 5.0, 6.0);
    let aabb = D3::shape_bounds(&shape, at, turn);
    let mut lo = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for v in hull.vertices() {
        let w = at + turn.rotate(*v);
        lo = Vec3::new(lo.x.min(w.x), lo.y.min(w.y), lo.z.min(w.z));
        hi = Vec3::new(hi.x.max(w.x), hi.y.max(w.y), hi.z.max(w.z));
    }
    assert!(
        near_vec(aabb.min, lo) && near_vec(aabb.max, hi),
        "{aabb:?} {lo:?} {hi:?}"
    );
}

#[test]
fn a_hull_hashes_by_its_points_and_apart_from_other_shapes() {
    let a = Shape3::Hull(built(&box_corners(Vec3::ONE, Vec3::ZERO, Quat::IDENTITY)));
    let same = Shape3::Hull(built(&box_corners(Vec3::ONE, Vec3::ZERO, Quat::IDENTITY)));
    let wider = Shape3::Hull(built(&box_corners(
        Vec3::new(1.0, 1.0, 1.001),
        Vec3::ZERO,
        Quat::IDENTITY,
    )));
    let cube = Shape3::Box {
        half_extents: Vec3::ONE,
    };
    assert_eq!(hash_of(&a), hash_of(&same));
    assert_ne!(hash_of(&a), hash_of(&wider));
    assert_ne!(hash_of(&a), hash_of(&cube));
}

#[test]
fn a_hull_that_is_a_valid_shape_can_be_a_body() {
    use physics::d3::{
        BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, SleepConfig,
        SolverConfig, World, WorldConfig,
    };
    let config = WorldConfig {
        max_bodies: 4,
        gravity: Vec3::ZERO,
        max_pairs: 16,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    };
    let Ok(mut world) = World::new(config) else {
        panic!("a world");
    };
    let shape = Shape3::Hull(built(&box_corners(Vec3::ONE, Vec3::ZERO, Quat::IDENTITY)));
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, shape);
    desc.density = 2.0;
    assert!(world.create_body(&desc).is_ok());
}

/// Every corner of `hull`, put back where the input was, is one of `points`.
fn put_back_matches(hull: &Hull, points: &[Vec3]) -> bool {
    hull.vertices().iter().all(|v| {
        let back = hull.origin() + hull.orientation().rotate(*v);
        points.iter().any(|p| near_vec(back, *p))
    })
}

#[test]
fn a_hull_of_a_box_turned_any_way_puts_its_corners_back_where_they_were() {
    let half = Vec3::new(0.4, 0.9, 1.6);
    let centre = Vec3::new(-1.0, 2.0, 0.5);
    let mut turns = vec![
        Quat::IDENTITY,
        about(Vec3::X, std::f32::consts::PI),
        about(Vec3::Y, std::f32::consts::PI),
        about(Vec3::Z, std::f32::consts::PI),
        about(Vec3::new(0.6, 0.0, 0.8), 2.5),
        about(Vec3::new(0.0, 0.6, 0.8), 3.0),
        about(Vec3::new(0.8, 0.6, 0.0), 2.2),
    ];
    let mut rng = Pcg32::new(0x8_5_6, 1);
    for _ in 0..200 {
        let mut next =
            || f32::from(i16::try_from(rng.below(2001).unwrap_or(0)).unwrap_or(0) - 1000);
        let random = Quat::new(next(), next(), next(), next()).normalized();
        turns.push(random.unwrap_or(Quat::IDENTITY));
    }
    for turn in turns {
        let points = box_corners(half, centre, turn);
        let hull = built(&points);
        assert!(
            near(hull.orientation().length(), 1.0),
            "{:?}",
            hull.orientation()
        );
        assert!(near_vec(hull.origin(), centre), "{:?}", hull.origin());
        assert!(put_back_matches(&hull, &points), "{turn:?}");
        assert_eq!(hull.vertices().len(), 8);
    }
}

#[test]
fn a_hull_of_scattered_points_keeps_only_input_points_and_puts_them_back() {
    let mut rng = Pcg32::new(0x8_5_7, 2);
    for _ in 0..60 {
        let mut coord =
            || f32::from(i16::try_from(rng.below(2001).unwrap_or(0)).unwrap_or(0) - 1000) / 700.0;
        let points: Vec<Vec3> = (0..12)
            .map(|_| Vec3::new(coord(), coord(), coord()))
            .collect();
        let hull = built(&points);
        assert!(put_back_matches(&hull, &points));
        assert!(hull.vertices().len() >= 4);
    }
}

/// The 24 ways to put the indices 0 to 3 in a row.
fn orderings() -> Vec<[usize; 4]> {
    let mut all = Vec::new();
    for a in 0..4 {
        for b in (0..4).filter(|b| *b != a) {
            all.extend(
                (0..4)
                    .filter(|c| *c != a && *c != b)
                    .map(|c| [a, b, c, 6 - a - b - c]),
            );
        }
    }
    all
}

#[test]
fn a_tetrahedron_does_not_depend_on_the_order_of_its_points() {
    let corners = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(0.0, 3.0, 0.0),
        Vec3::new(0.0, 0.0, 6.0),
    ];
    let mut checked = 0;
    for [a, b, c, d] in orderings() {
        let hull = built(&[corners[a], corners[b], corners[c], corners[d]]);
        assert!(
            near_vec(hull.origin(), Vec3::new(0.5, 0.75, 1.5)),
            "{a}{b}{c}{d}"
        );
        assert_eq!(hull.vertices().len(), 4);
        let Ok(mass) = D3::mass_properties(&Shape3::Hull(hull), 1.0) else {
            panic!("a tetrahedron has a mass");
        };
        assert!(near(mass.mass, 6.0), "{a}{b}{c}{d}: {}", mass.mass);
        checked += 1;
    }
    assert_eq!(checked, 24);
}

#[test]
fn a_square_pyramid_has_its_centre_of_mass_a_quarter_up_and_the_exact_inertia() {
    // Base 2 x 2 at z = 0, apex at z = 2: the mean of the corners is at z = 0.4, the centre of
    // mass at z = 0.5. Volume 8/3; per unit density Ixx = Iyy = 14/15, Izz = 16/15.
    let points = [
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 2.0),
    ];
    let hull = built(&points);
    assert!(
        near_vec(hull.origin(), Vec3::new(0.0, 0.0, 0.5)),
        "{:?}",
        hull.origin()
    );
    let Ok(mass) = D3::mass_properties(&Shape3::Hull(hull), 1.0) else {
        panic!("a pyramid has a mass");
    };
    assert!(near(mass.mass, 8.0 / 3.0), "{}", mass.mass);
    let inverse = sorted(mass.inv_inertia);
    let wanted = [15.0 / 16.0, 15.0 / 14.0, 15.0 / 14.0];
    for (got, want) in inverse.iter().zip(wanted) {
        assert!((got - want).abs() < 1.0e-3, "{inverse:?} wanted {wanted:?}");
    }
}

#[test]
fn a_prism_with_a_seven_sided_face_has_its_exact_volume_whatever_the_order_of_the_points() {
    let sides = 7_u8;
    let angle = |i: u8| 2.0 * std::f32::consts::PI * f32::from(i) / f32::from(sides);
    let mut ring: Vec<Vec3> = Vec::new();
    for i in 0..sides {
        let (x, y) = (base::dmath::cos(angle(i)), base::dmath::sin(angle(i)));
        ring.push(Vec3::new(x, y, -1.0));
        ring.push(Vec3::new(x, y, 1.0));
    }
    // Area of the regular heptagon of radius 1, times the height 2.
    let volume = 7.0 * 0.5 * base::dmath::sin(angle(1)) * 2.0;
    let mut reversed = ring.clone();
    reversed.reverse();
    let mut interleaved: Vec<Vec3> = ring
        .iter()
        .step_by(2)
        .chain(ring.iter().skip(1).step_by(2))
        .copied()
        .collect();
    interleaved.rotate_left(3);
    for points in [ring, reversed, interleaved] {
        let hull = built(&points);
        assert_eq!(hull.vertices().len(), 14);
        let Ok(mass) = D3::mass_properties(&Shape3::Hull(hull), 1.0) else {
            panic!("a prism has a mass");
        };
        assert!(
            (mass.mass - volume).abs() < 1.0e-3,
            "{} wanted {volume}",
            mass.mass
        );
    }
}

#[test]
fn a_hull_is_the_same_solid_at_any_size_and_far_from_the_origin() {
    for (scale, centre) in [
        (0.00001, Vec3::ZERO),
        (0.001, Vec3::ZERO),
        (1.0, Vec3::ZERO),
        (1000.0, Vec3::ZERO),
        (1.0, Vec3::new(1000.0, -2000.0, 3000.0)),
        (0.01, Vec3::new(5.0, 5.0, 5.0)),
    ] {
        let points = box_corners(Vec3::ONE * scale, centre, Quat::IDENTITY);
        let hull = built(&points);
        assert_eq!(hull.vertices().len(), 8, "scale {scale} at {centre:?}");
        let Ok(mass) = D3::mass_properties(&Shape3::Hull(hull), 1.0) else {
            panic!("a cube has a mass");
        };
        let wanted = 8.0 * scale * scale * scale;
        assert!(
            (mass.mass - wanted).abs() <= 2.0e-3 * wanted,
            "scale {scale} at {centre:?}: {} wanted {wanted}",
            mass.mass
        );
    }
}

/// The lowest and highest coordinate on each axis.
fn extremes(points: &[Vec3]) -> (Vec3, Vec3) {
    let low = points
        .iter()
        .fold(Vec3::new(f32::MAX, f32::MAX, f32::MAX), |m, p| {
            Vec3::new(m.x.min(p.x), m.y.min(p.y), m.z.min(p.z))
        });
    let high = points
        .iter()
        .fold(Vec3::new(f32::MIN, f32::MIN, f32::MIN), |m, p| {
            Vec3::new(m.x.max(p.x), m.y.max(p.y), m.z.max(p.z))
        });
    (low, high)
}

/// Twenty random points in a cube of side `scale`, its centre `offset` along each axis.
fn scattered(rng: &mut Pcg32, scale: f32, offset: f32) -> Vec<Vec3> {
    let mut coord = || {
        let step = f32::from(i16::try_from(rng.below(2001).unwrap_or(0)).unwrap_or(0) - 1000);
        step / 2000.0 * scale + offset
    };
    (0..20)
        .map(|_| Vec3::new(coord(), coord(), coord()))
        .collect()
}

/// Twenty random points always have a hull: it must be built, and reach their extremes.
fn reaches_the_extremes(points: &[Vec3], slack: f32) -> bool {
    let Ok(hull) = Hull::new(points) else {
        return false;
    };
    let world: Vec<Vec3> = hull
        .vertices()
        .iter()
        .map(|v| hull.origin() + hull.orientation().rotate(*v))
        .collect();
    let ((low, high), (hull_low, hull_high)) = (extremes(points), extremes(&world));
    near_within(low, hull_low, slack) && near_within(high, hull_high, slack)
}

#[test]
fn a_hull_far_from_the_origin_is_built_and_still_reaches_every_extreme_point() {
    let mut rng = Pcg32::new(0x0858, 3);
    for (scale, offset) in [
        (1.0_f32, 0.0_f32),
        (1.0, 100.0),
        (0.01, 10.0),
        (10.0, 10_000.0),
        (0.1, 1.0),
    ] {
        let slack = 2.0e-3 * scale + 1.0e-6 * offset;
        for _ in 0..60 {
            let points = scattered(&mut rng, scale, offset);
            assert!(
                reaches_the_extremes(&points, slack),
                "scale {scale} offset {offset}"
            );
        }
    }
}

fn near_within(a: Vec3, b: Vec3, slack: f32) -> bool {
    let d = (a - b).abs();
    d.x.max(d.y).max(d.z) <= slack
}

#[test]
fn a_thin_plate_keeps_all_its_corners() {
    for thickness in [0.001, 0.01, 0.1] {
        let points = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, thickness),
            Vec3::new(10.0, 10.0, thickness),
        ];
        let hull = built(&points);
        assert_eq!(hull.vertices().len(), 5, "thickness {thickness}");
    }
}

/// The twice-signed area under the turn from `a` to `b` round the origin.
fn cross_2d(a: (f32, f32), b: (f32, f32)) -> f32 {
    a.0 * b.1 - b.0 * a.1
}

/// The next point of the outline after `current`: the one with every other point on its left.
fn next_on_outline(points: &[(f32, f32)], current: usize) -> usize {
    let mut next = (current + 1) % points.len();
    for (candidate, p) in points.iter().enumerate() {
        let (a, b) = (points[current], points[next]);
        let turn = cross_2d((b.0 - a.0, b.1 - a.1), (p.0 - a.0, p.1 - a.1));
        let reach = |from: (f32, f32), to: (f32, f32)| {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            dx * dx + dy * dy
        };
        let farther = reach(a, *p) > reach(a, b);
        next = if turn < 0.0 || (turn == 0.0 && farther) {
            candidate
        } else {
            next
        };
    }
    next
}

/// The area of the convex hull of points in the plane (gift wrapping, counter-clockwise).
fn area_of_hull_2d(points: &[(f32, f32)]) -> f32 {
    let start = points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1)))
        .map_or(0, |(i, _)| i);
    let mut ring = vec![start];
    for _ in 0..points.len() {
        let next = next_on_outline(points, ring[ring.len() - 1]);
        if next == start {
            break;
        }
        ring.push(next);
    }
    let doubled: f32 = (0..ring.len())
        .map(|i| cross_2d(points[ring[i]], points[ring[(i + 1) % ring.len()]]))
        .sum();
    doubled.abs() * 0.5
}

/// Twelve random points of a square of side 2 and, for each, the point above it at `thickness`.
fn outline_and_prism(rng: &mut Pcg32, thickness: f32) -> (Vec<(f32, f32)>, Vec<Vec3>) {
    let mut coord =
        || f32::from(i16::try_from(rng.below(2001).unwrap_or(0)).unwrap_or(0) - 1000) / 1000.0;
    let outline: Vec<(f32, f32)> = (0..12).map(|_| (coord(), coord())).collect();
    let points = outline
        .iter()
        .flat_map(|(x, y)| [Vec3::new(*x, *y, 0.0), Vec3::new(*x, *y, thickness)])
        .collect();
    (outline, points)
}

/// Whether the hull of `points` is built with a volume within `1e-3` of `wanted`.
fn prism_is_right(points: &[Vec3], wanted: f32) -> Option<bool> {
    let volume = volume_of(points)?;
    Some((volume - wanted).abs() <= 2.0e-3 * wanted)
}

#[test]
fn a_thin_prism_has_the_volume_of_its_outline_times_its_thickness() {
    let mut rng = Pcg32::new(0x0859, 4);
    for thickness in [1.0e-2_f32, 1.0e-3, 1.0e-4] {
        let (mut right, mut wrong, mut refused) = (0, 0, 0);
        for _ in 0..80 {
            let (outline, points) = outline_and_prism(&mut rng, thickness);
            match prism_is_right(&points, area_of_hull_2d(&outline) * thickness) {
                Some(true) => right += 1,
                Some(false) => wrong += 1,
                None => refused += 1,
            }
        }
        assert_eq!(
            wrong, 0,
            "thickness {thickness}: {wrong} wrong, {right} right, {refused} refused"
        );
        assert!(
            right >= 72,
            "thickness {thickness}: {right} right, {refused} refused"
        );
    }
}

/// The volume of the hull of `points`, or `None` when they are refused.
fn volume_of(points: &[Vec3]) -> Option<f32> {
    let hull = Hull::new(points).ok()?;
    D3::mass_properties(&Shape3::Hull(hull), 1.0)
        .ok()
        .map(|m| m.mass)
}

/// Sixteen random points over a square of side 2, their height random within `thickness`.
fn plate(rng: &mut Pcg32, thickness: f32) -> Vec<Vec3> {
    let mut unit =
        || f32::from(i16::try_from(rng.below(2001).unwrap_or(0)).unwrap_or(0) - 1000) / 1000.0;
    (0..16)
        .map(|_| Vec3::new(unit(), unit(), (unit() + 1.0) * 0.5 * thickness))
        .collect()
}

/// How the volume of the hull of `points` compares in three orders of them: `None` when any of
/// them is refused, else whether the largest difference is within `allowed` of the first volume.
fn agrees_in_any_order(points: &[Vec3], allowed: f32) -> Option<bool> {
    let mut reversed = points.to_vec();
    reversed.reverse();
    let mut turned = points.to_vec();
    turned.rotate_left(5);
    let (first, second, third) = (
        volume_of(points)?,
        volume_of(&reversed)?,
        volume_of(&turned)?,
    );
    let spread = (first - second).abs().max((first - third).abs());
    Some(spread <= allowed * first)
}

#[test]
fn a_thin_plate_has_the_same_volume_in_any_order_of_its_points() {
    let mut rng = Pcg32::new(0x085A, 5);
    // The planes are found to within 1e-5 of the size, so the volume of a plate of thickness t is
    // good to about 2e-5 * size / t: 0.1% at 0.1, 0.3% at 0.01, a few percent at 0.001.
    for (thickness, allowed) in [(1.0e-1_f32, 1.0e-3_f32), (1.0e-2, 3.0e-3), (1.0e-3, 8.0e-2)] {
        let (mut agreed, mut disagreed, mut refused) = (0, 0, 0);
        for _ in 0..120 {
            match agrees_in_any_order(&plate(&mut rng, thickness), allowed) {
                Some(true) => agreed += 1,
                Some(false) => disagreed += 1,
                None => refused += 1,
            }
        }
        assert_eq!(
            disagreed, 0,
            "thickness {thickness}: {disagreed} disagree, {agreed} agree, {refused} refused"
        );
        assert!(refused <= 60, "thickness {thickness}: {refused} refused");
    }
}
