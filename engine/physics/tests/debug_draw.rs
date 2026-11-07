//! The debug-draw port through the public 2D API (card 4-24): the outlines, centres and contacts a
//! world describes, with the colour roles, in a fixed order.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyKind, CIRCLE_SEGMENTS, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    DebugColour, IPhysicsDebugDraw, NORMAL_LENGTH, PhysicsError, Rot2, STANDARD_GRAVITY, Shape2,
    SolverConfig, World, WorldConfig, draw_debug,
};

const DT: f32 = 1.0 / 60.0;
const EPS: f32 = 1.0e-4;

enum Call {
    Line(Vec2, Vec2, DebugColour),
    Point(Vec2, DebugColour),
}

#[derive(Default)]
struct Recorder(Vec<Call>);

impl IPhysicsDebugDraw for Recorder {
    fn line(&mut self, a: Vec2, b: Vec2, colour: DebugColour) {
        self.0.push(Call::Line(a, b, colour));
    }
    fn point(&mut self, p: Vec2, colour: DebugColour) {
        self.0.push(Call::Point(p, colour));
    }
}

impl Recorder {
    fn lines(&self, colour: DebugColour) -> Vec<(Vec2, Vec2)> {
        self.0
            .iter()
            .filter_map(|c| match c {
                Call::Line(a, b, k) if *k == colour => Some((*a, *b)),
                _ => None,
            })
            .collect()
    }
    fn points(&self, colour: DebugColour) -> Vec<Vec2> {
        self.0
            .iter()
            .filter_map(|c| match c {
                Call::Point(p, k) if *k == colour => Some(*p),
                _ => None,
            })
            .collect()
    }
}

fn world(gravity: Vec2) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 8,
        gravity,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DEFAULT,
    })
}

fn boxed(kind: BodyKind, x: f32, y: f32, hx: f32, hy: f32) -> BodyDesc {
    BodyDesc::new(
        kind,
        Vec2::new(x, y),
        Shape2::Box {
            half_extents: Vec2::new(hx, hy),
        },
    )
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS
}

fn recorded(world: &World) -> Recorder {
    let mut rec = Recorder::default();
    draw_debug(world, &mut rec);
    rec
}

#[test]
fn an_empty_world_describes_nothing() -> Result<(), PhysicsError> {
    assert!(recorded(&world(Vec2::ZERO)?).0.is_empty());
    Ok(())
}

#[test]
fn a_box_is_four_edges_through_its_corners_and_a_centre() -> Result<(), PhysicsError> {
    let mut w = world(Vec2::ZERO)?;
    w.create_body(&boxed(BodyKind::Dynamic, 1.0, 2.0, 2.0, 1.0))?;
    let rec = recorded(&w);
    let edges = rec.lines(DebugColour::Awake);
    assert_eq!(edges.len(), 4);
    let corners = [
        Vec2::new(-1.0, 1.0),
        Vec2::new(3.0, 1.0),
        Vec2::new(3.0, 3.0),
        Vec2::new(-1.0, 3.0),
    ];
    for (i, (a, b)) in edges.iter().enumerate() {
        assert!(near(*a, corners[i]), "edge {i} starts at {a:?}");
        assert!(near(*b, corners[(i + 1) % 4]), "edge {i} ends at {b:?}");
    }
    assert_eq!(rec.points(DebugColour::Awake), [Vec2::new(1.0, 2.0)]);
    Ok(())
}

#[test]
fn a_turned_box_turns_its_corners() -> Result<(), PhysicsError> {
    let mut w = world(Vec2::ZERO)?;
    let mut desc = boxed(BodyKind::Dynamic, 0.0, 0.0, 2.0, 1.0);
    desc.rotation = Rot2::from_angle(std::f32::consts::FRAC_PI_2);
    w.create_body(&desc)?;
    let edges = recorded(&w).lines(DebugColour::Awake);
    // (-2,-1) turned a quarter counter-clockwise is (1,-2).
    assert!(near(edges[0].0, Vec2::new(1.0, -2.0)), "{:?}", edges[0]);
    Ok(())
}

#[test]
fn a_static_body_has_its_own_colour() -> Result<(), PhysicsError> {
    let mut w = world(Vec2::ZERO)?;
    w.create_body(&boxed(BodyKind::Static, 0.0, 0.0, 1.0, 1.0))?;
    let rec = recorded(&w);
    assert_eq!(rec.lines(DebugColour::Static).len(), 4);
    assert_eq!(rec.points(DebugColour::Static).len(), 1);
    assert!(rec.lines(DebugColour::Awake).is_empty());
    Ok(())
}

#[test]
fn a_circle_is_a_ring_of_segments_and_a_spoke() -> Result<(), PhysicsError> {
    let mut w = world(Vec2::ZERO)?;
    let centre = Vec2::new(3.0, -1.0);
    w.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        centre,
        Shape2::Circle { radius: 2.0 },
    ))?;
    let lines = recorded(&w).lines(DebugColour::Awake);
    let ring = usize::from(CIRCLE_SEGMENTS);
    assert_eq!(lines.len(), ring + 1);
    for (a, b) in &lines[..ring] {
        assert!((a.distance(centre) - 2.0).abs() < EPS, "{a:?}");
        assert!((b.distance(centre) - 2.0).abs() < EPS, "{b:?}");
    }
    // The ring closes, and the spoke runs from the centre to the rim along the body's x axis.
    assert!(near(lines[ring - 1].1, lines[0].0));
    assert!(near(lines[ring].0, centre));
    assert!(near(lines[ring].1, Vec2::new(5.0, -1.0)));
    Ok(())
}

#[test]
fn a_resting_box_shows_its_contact_points_and_normals() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&boxed(BodyKind::Static, 0.0, -1.0, 10.0, 1.0))?;
    w.create_body(&boxed(BodyKind::Dynamic, 0.0, 0.49, 0.5, 0.5))?;
    w.step(DT)?;
    let expected: usize = w
        .manifolds()
        .as_slice()
        .iter()
        .map(|m| m.manifold.points().len())
        .sum();
    assert!(expected >= 1);
    let rec = recorded(&w);
    assert_eq!(rec.points(DebugColour::ContactPoint).len(), expected);
    let normals = rec.lines(DebugColour::ContactNormal);
    assert_eq!(normals.len(), expected);
    let mut drawn = normals.iter();
    for contact in w.manifolds().as_slice() {
        for point in contact.manifold.points() {
            let expected_end = point.point + contact.manifold.normal * NORMAL_LENGTH;
            assert!(drawn.any(|&(from, to)| near(from, point.point) && near(to, expected_end)));
        }
    }
    for (from, to) in normals {
        assert!((from.distance(to) - NORMAL_LENGTH).abs() < EPS);
    }
    Ok(())
}

#[test]
fn a_sleeping_body_is_drawn_as_asleep() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&boxed(BodyKind::Static, 0.0, -1.0, 10.0, 1.0))?;
    let rest = w.create_body(&boxed(BodyKind::Dynamic, 0.0, 0.5, 0.5, 0.5))?;
    for _ in 0..240 {
        w.step(DT)?;
    }
    assert!(w.is_asleep(rest)?);
    let rec = recorded(&w);
    assert_eq!(rec.lines(DebugColour::Asleep).len(), 4);
    assert!(rec.lines(DebugColour::Awake).is_empty());
    Ok(())
}

#[test]
fn a_turned_circle_turns_its_spoke_and_the_ring_steps_evenly() -> Result<(), PhysicsError> {
    let mut w = world(Vec2::ZERO)?;
    let mut desc = BodyDesc::new(BodyKind::Static, Vec2::ZERO, Shape2::Circle { radius: 2.0 });
    desc.rotation = Rot2::from_angle(std::f32::consts::FRAC_PI_2);
    w.create_body(&desc)?;
    let rec = recorded(&w);
    let lines = rec.lines(DebugColour::Static);
    let ring = usize::from(CIRCLE_SEGMENTS);
    assert!(
        near(lines[ring].1, Vec2::new(0.0, 2.0)),
        "{:?}",
        lines[ring]
    );
    // One segment spans a sixteenth of a turn: its chord is 2 r sin(pi / 16) = 4 * 0.195090.
    let chord = 0.780_361_3;
    assert!((lines[0].0.distance(lines[0].1) - chord).abs() < 1.0e-3);
    assert_eq!(rec.points(DebugColour::Static).len(), 1);
    Ok(())
}

#[test]
fn bodies_come_in_slot_order_then_the_contacts() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&boxed(BodyKind::Static, 0.0, -1.0, 10.0, 1.0))?;
    w.create_body(&boxed(BodyKind::Dynamic, 0.0, 0.49, 0.5, 0.5))?;
    w.step(DT)?;
    let rec = recorded(&w);
    let kinds: Vec<(bool, DebugColour)> = rec
        .0
        .iter()
        .map(|c| match c {
            Call::Line(_, _, k) => (true, *k),
            Call::Point(_, k) => (false, *k),
        })
        .collect();
    let mut expected = vec![(true, DebugColour::Static); 4];
    expected.push((false, DebugColour::Static));
    expected.extend(vec![(true, DebugColour::Awake); 4]);
    expected.push((false, DebugColour::Awake));
    assert_eq!(kinds[..10], expected[..]);
    assert!(kinds[10..].chunks(2).all(|pair| pair
        == [
            (false, DebugColour::ContactPoint),
            (true, DebugColour::ContactNormal)
        ]));
    Ok(())
}

#[test]
fn a_sleeping_body_centre_is_asleep_too() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&boxed(BodyKind::Static, 0.0, -1.0, 10.0, 1.0))?;
    let rest = w.create_body(&boxed(BodyKind::Dynamic, 0.0, 0.5, 0.5, 0.5))?;
    for _ in 0..240 {
        w.step(DT)?;
    }
    assert!(w.is_asleep(rest)?);
    assert_eq!(recorded(&w).points(DebugColour::Asleep).len(), 1);
    Ok(())
}
