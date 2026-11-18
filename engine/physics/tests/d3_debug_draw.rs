//! Card 8-4: the debug-draw port through the public 3D API: outlines, centres and contacts a world
//! describes, with the colour roles, in a fixed order. The 2D port is `debug_draw.rs`.

use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    ARC_SEGMENTS, BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    DebugColour, Hull, IPhysicsDebugDraw, NORMAL_LENGTH, STANDARD_GRAVITY, Shape3, SleepConfig,
    SolverConfig, World, WorldConfig, draw_debug,
};

const DT: f32 = 1.0 / 60.0;
const EPS: f32 = 1.0e-4;

enum Call {
    Line(Vec3, Vec3, DebugColour),
    Point(Vec3, DebugColour),
}

#[derive(Default)]
struct Recorder(Vec<Call>);

impl IPhysicsDebugDraw for Recorder {
    fn line(&mut self, from: Vec3, to: Vec3, colour: DebugColour) {
        self.0.push(Call::Line(from, to, colour));
    }
    fn point(&mut self, at: Vec3, colour: DebugColour) {
        self.0.push(Call::Point(at, colour));
    }
}

impl Recorder {
    fn lines(&self, colour: DebugColour) -> Vec<(Vec3, Vec3)> {
        self.0
            .iter()
            .filter_map(|call| match call {
                Call::Line(from, to, kind) if *kind == colour => Some((*from, *to)),
                _ => None,
            })
            .collect()
    }
    fn points(&self, colour: DebugColour) -> Vec<Vec3> {
        self.0
            .iter()
            .filter_map(|call| match call {
                Call::Point(at, kind) if *kind == colour => Some(*at),
                _ => None,
            })
            .collect()
    }
}

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 8,
        gravity,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })
}

fn body(kind: BodyKind, at: Vec3, shape: &Shape3) -> BodyDesc {
    BodyDesc::new(kind, at, *shape)
}

fn cuboid(x: f32, y: f32, z: f32) -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(x, y, z),
    }
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < EPS
}

fn recorded(world: &World) -> Recorder {
    let mut rec = Recorder::default();
    draw_debug(world, &mut rec);
    rec
}

fn arc_segments() -> usize {
    usize::from(ARC_SEGMENTS)
}

/// The distance of `point` from the segment `a`..`b`.
fn distance_to_segment(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    let along = b - a;
    let t = ((point - a).dot(along) / along.length_squared()).clamp(0.0, 1.0);
    (point - (a + along * t)).length()
}

#[test]
fn an_empty_world_describes_nothing() -> Result<(), PhysicsError> {
    assert!(recorded(&world(Vec3::ZERO)?).0.is_empty());
    Ok(())
}

#[test]
fn a_box_is_twelve_edges_between_its_eight_corners_and_a_centre() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let centre = Vec3::new(1.0, 2.0, 3.0);
    w.create_body(&body(BodyKind::Dynamic, centre, &cuboid(2.0, 1.5, 0.5)))?;
    let rec = recorded(&w);
    let edges = rec.lines(DebugColour::Awake);
    assert_eq!(edges.len(), 12);
    let is_corner = |p: Vec3| {
        let d = p - centre;
        (d.x.abs() - 2.0).abs() < EPS
            && (d.y.abs() - 1.5).abs() < EPS
            && (d.z.abs() - 0.5).abs() < EPS
    };
    let mut lengths = [0; 3];
    for (from, to) in &edges {
        assert!(is_corner(*from) && is_corner(*to), "{from:?} {to:?}");
        // An edge changes exactly one coordinate: by 4, 3 or 1.
        let d = *to - *from;
        let changed = [d.x.abs(), d.y.abs(), d.z.abs()];
        let long = changed.iter().filter(|c| **c > EPS).count();
        assert_eq!(long, 1, "{d:?}");
        for (slot, size) in lengths.iter_mut().zip([4.0, 3.0, 1.0]) {
            *slot += usize::from(changed.iter().any(|c| (c - size).abs() < EPS));
        }
    }
    assert_eq!(lengths, [4, 4, 4]);
    // Each corner is the end of three edges.
    for index in 0..8_u8 {
        let sign = |bit: u8| if index >> bit & 1 == 1 { 1.0 } else { -1.0 };
        let corner = centre + Vec3::new(2.0 * sign(0), 1.5 * sign(1), 0.5 * sign(2));
        let touching = edges
            .iter()
            .filter(|(a, b)| near(*a, corner) || near(*b, corner))
            .count();
        assert_eq!(touching, 3, "{corner:?}");
    }
    assert_eq!(rec.points(DebugColour::Awake), [centre]);
    Ok(())
}

#[test]
fn a_turned_box_turns_its_corners() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let mut desc = body(BodyKind::Dynamic, Vec3::ZERO, &cuboid(2.0, 1.0, 0.5));
    desc.rotation =
        Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
    w.create_body(&desc)?;
    let edges = recorded(&w).lines(DebugColour::Awake);
    // A quarter turn about z sends the corner (2, 1, .5) to (-1, 2, .5).
    let corner = Vec3::new(-1.0, 2.0, 0.5);
    assert!(
        edges
            .iter()
            .any(|(a, b)| near(*a, corner) || near(*b, corner))
    );
    assert!(
        !edges
            .iter()
            .any(|(a, _)| near(*a, Vec3::new(2.0, 1.0, 0.5)))
    );
    Ok(())
}

#[test]
fn a_static_body_has_its_own_colour() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    w.create_body(&body(BodyKind::Static, Vec3::ZERO, &cuboid(1.0, 1.0, 1.0)))?;
    let rec = recorded(&w);
    assert_eq!(rec.lines(DebugColour::Static).len(), 12);
    assert_eq!(rec.points(DebugColour::Static).len(), 1);
    assert!(rec.lines(DebugColour::Awake).is_empty());
    Ok(())
}

#[test]
fn a_sphere_is_three_great_circles_and_a_centre() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let centre = Vec3::new(3.0, -1.0, 0.5);
    w.create_body(&body(
        BodyKind::Dynamic,
        centre,
        &Shape3::Sphere { radius: 2.0 },
    ))?;
    let rec = recorded(&w);
    let lines = rec.lines(DebugColour::Awake);
    assert_eq!(lines.len(), 3 * arc_segments());
    for (from, to) in &lines {
        assert!(((*from - centre).length() - 2.0).abs() < EPS, "{from:?}");
        assert!(((*to - centre).length() - 2.0).abs() < EPS, "{to:?}");
    }
    // Each circle closes and lies in one of the three planes through the centre.
    for (circle, ring) in lines.chunks(arc_segments()).enumerate() {
        assert!(
            near(ring[ring.len() - 1].1, ring[0].0),
            "circle {circle} closes"
        );
        let flat = |axis: usize| {
            ring.iter()
                .all(|(a, _)| ((*a - centre).to_array()[axis]).abs() < EPS)
        };
        assert!(flat(circle), "circle {circle} lies across axis {circle}");
    }
    assert_eq!(rec.points(DebugColour::Awake), [centre]);
    Ok(())
}

#[test]
fn a_capsule_is_rings_sides_and_cap_arcs_all_on_its_surface() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let centre = Vec3::new(0.0, 1.0, 0.0);
    let shape = Shape3::Capsule {
        half_height: 1.5,
        radius: 0.5,
    };
    w.create_body(&body(BodyKind::Dynamic, centre, &shape))?;
    let rec = recorded(&w);
    let lines = rec.lines(DebugColour::Awake);
    let half_arcs = arc_segments() / 2;
    assert_eq!(lines.len(), 2 * arc_segments() + 4 + 4 * half_arcs);
    let (bottom, top) = (centre - Vec3::Y * 1.5, centre + Vec3::Y * 1.5);
    for (from, to) in &lines {
        for end in [from, to] {
            let d = distance_to_segment(*end, bottom, top);
            assert!((d - 0.5).abs() < EPS, "{end:?} is {d} from the axis");
        }
    }
    // The four sides run the full length of the cylinder part.
    let sides = lines
        .iter()
        .filter(|(a, b)| ((*b - *a).length() - 3.0).abs() < EPS)
        .count();
    assert_eq!(sides, 4);
    // They stand a quarter turn apart on the cylinder: at +x, +z, -x and -z.
    let mut feet: Vec<Vec3> = lines
        .iter()
        .filter(|(a, b)| ((*b - *a).length() - 3.0).abs() < EPS)
        .map(|(a, _)| Vec3::new(a.x, 0.0, a.z))
        .collect();
    for wanted in [
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 0.5),
        Vec3::new(-0.5, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -0.5),
    ] {
        let at = feet.iter().position(|foot| near(*foot, wanted));
        assert!(at.is_some(), "no side at {wanted:?}");
        if let Some(i) = at {
            feet.swap_remove(i);
        }
    }
    // The caps reach the tips of the capsule.
    let reaches = |tip: Vec3| lines.iter().any(|(a, b)| near(*a, tip) || near(*b, tip));
    assert!(reaches(centre + Vec3::Y * 2.0) && reaches(centre - Vec3::Y * 2.0));
    Ok(())
}

#[test]
fn a_turned_capsule_turns_with_the_body() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let mut desc = body(
        BodyKind::Dynamic,
        Vec3::ZERO,
        &Shape3::Capsule {
            half_height: 1.5,
            radius: 0.5,
        },
    );
    desc.rotation =
        Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
    w.create_body(&desc)?;
    let lines = recorded(&w).lines(DebugColour::Awake);
    // Its axis, local y, now points along -x: the tips are at x = -2 and x = 2.
    let reaches = |tip: Vec3| lines.iter().any(|(a, b)| near(*a, tip) || near(*b, tip));
    assert!(reaches(Vec3::new(2.0, 0.0, 0.0)) && reaches(Vec3::new(-2.0, 0.0, 0.0)));
    Ok(())
}

#[test]
fn a_resting_sphere_shows_its_contact_point_and_normal() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&body(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        &cuboid(10.0, 1.0, 10.0),
    ))?;
    w.create_body(&body(
        BodyKind::Dynamic,
        Vec3::new(0.0, 0.49, 0.0),
        &Shape3::Sphere { radius: 0.5 },
    ))?;
    w.step(DT)?;
    let expected: usize = w
        .manifolds()
        .as_slice()
        .iter()
        .map(|m| m.manifold.active().len())
        .sum();
    assert!(expected >= 1);
    let rec = recorded(&w);
    assert_eq!(rec.points(DebugColour::ContactPoint).len(), expected);
    let normals = rec.lines(DebugColour::ContactNormal);
    assert_eq!(normals.len(), expected);
    for contact in w.manifolds().as_slice() {
        for point in contact.manifold.active() {
            let end = point.point + contact.manifold.normal * NORMAL_LENGTH;
            assert!(
                normals
                    .iter()
                    .any(|(a, b)| near(*a, point.point) && near(*b, end))
            );
        }
    }
    for (from, to) in normals {
        assert!(((to - from).length() - NORMAL_LENGTH).abs() < EPS);
    }
    Ok(())
}

#[test]
fn bodies_come_in_slot_order_then_the_contacts() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&body(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        &cuboid(10.0, 1.0, 10.0),
    ))?;
    w.create_body(&body(
        BodyKind::Dynamic,
        Vec3::new(0.0, 0.49, 0.0),
        &Shape3::Sphere { radius: 0.5 },
    ))?;
    w.step(DT)?;
    let kinds: Vec<(bool, DebugColour)> = recorded(&w)
        .0
        .iter()
        .map(|call| match call {
            Call::Line(_, _, kind) => (true, *kind),
            Call::Point(_, kind) => (false, *kind),
        })
        .collect();
    let static_calls = 12 + 1;
    let sphere_calls = 3 * arc_segments() + 1;
    assert!(
        kinds[..static_calls - 1]
            .iter()
            .all(|k| *k == (true, DebugColour::Static))
    );
    assert_eq!(kinds[static_calls - 1], (false, DebugColour::Static));
    let after = static_calls + sphere_calls;
    assert!(
        kinds[static_calls..after - 1]
            .iter()
            .all(|k| *k == (true, DebugColour::Awake))
    );
    assert_eq!(kinds[after - 1], (false, DebugColour::Awake));
    assert!(kinds[after..].chunks(2).all(|pair| pair
        == [
            (false, DebugColour::ContactPoint),
            (true, DebugColour::ContactNormal)
        ]));
    Ok(())
}

#[test]
fn a_sleeping_body_is_drawn_as_asleep() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let rest = w.create_body(&body(BodyKind::Dynamic, Vec3::ZERO, &cuboid(0.5, 0.5, 0.5)))?;
    for _ in 0..240 {
        w.step(DT)?;
    }
    assert!(w.is_asleep(rest)?);
    let rec = recorded(&w);
    assert_eq!(rec.lines(DebugColour::Asleep).len(), 12);
    assert_eq!(rec.points(DebugColour::Asleep).len(), 1);
    assert!(rec.lines(DebugColour::Awake).is_empty());
    Ok(())
}

fn hull_shape(points: &[Vec3]) -> Result<Shape3, PhysicsError> {
    Ok(Shape3::Hull(Hull::new(points)?))
}

#[test]
fn a_hull_is_its_edges_once_each_between_its_corners_turned_with_the_body()
-> Result<(), PhysicsError> {
    // A tetrahedron has 6 edges, a cube 12 (the diagonals of its square faces are not edges).
    let tetrahedron = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
    let cube: Vec<Vec3> = (0..8_u8)
        .map(|i| {
            let side = |bit: u8| if i >> bit & 1 == 1 { 1.0 } else { -1.0 };
            Vec3::new(side(0), side(1), side(2))
        })
        .collect();
    let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.6, 0.8), 0.9).unwrap_or(Quat::IDENTITY);
    let at = Vec3::new(1.0, -2.0, 3.0);
    for (points, wanted) in [(&tetrahedron[..], 6), (&cube[..], 12)] {
        let shape = hull_shape(points)?;
        let Shape3::Hull(hull) = shape else {
            panic!("a hull");
        };
        let mut w = world(Vec3::ZERO)?;
        let mut desc = body(BodyKind::Dynamic, at, &shape);
        desc.rotation = turn;
        w.create_body(&desc)?;
        let rec = recorded(&w);
        let edges = rec.lines(DebugColour::Awake);
        assert_eq!(edges.len(), wanted);
        let corners: Vec<Vec3> = hull
            .vertices()
            .iter()
            .map(|v| at + turn.rotate(*v))
            .collect();
        for (from, to) in &edges {
            assert!(
                [from, to]
                    .iter()
                    .all(|end| corners.iter().any(|c| near(*c, **end))),
                "{from:?} {to:?}"
            );
            assert!(!near(*from, *to));
        }
        // No edge twice, in either direction.
        for (index, (from, to)) in edges.iter().enumerate() {
            assert!(
                !edges[index + 1..]
                    .iter()
                    .any(|(a, b)| (near(*a, *from) && near(*b, *to))
                        || (near(*a, *to) && near(*b, *from)))
            );
        }
        assert_eq!(rec.points(DebugColour::Awake), [at]);
    }
    Ok(())
}
