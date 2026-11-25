//! Card 8-34: the contact of a hull against the contact of a box, and of round hulls. Card 8-6 left
//! four hull limits open (one contact point per pair: a hull overlapped more, a stacked hull sank,
//! the points hopped); card 8-18 gave hulls up to four points. These tests measure what is left:
//! a hull cube is the same as a box cube (through another code path: `face_clip` for the hull, `box_box` for the box) in every number below, so what remains of the first two
//! limits is the solver's, not the hull's; the switch from four points to two at a tilt of 1.5
//! degrees is the box's too (an edge contact is two points); a prism at rest has fixed points, and
//! a rolling prism keeps both points of the edge it touches (card 8-71).

use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Hull,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The fixed step of every scene: 60 Hz.
const DT: f32 = 1.0 / 60.0;

/// Half the side of the cubes.
const HALF: f32 = 0.5;

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 64,
        gravity,
        max_pairs: 1024,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DISABLED,
    })
}

fn box_cube() -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(HALF, HALF, HALF),
    }
}

fn hull_cube() -> Result<Shape3, PhysicsError> {
    let corners: Vec<Vec3> = (0..8_u8)
        .map(|n| {
            let side = |bit: u8| if n & bit == 0 { -HALF } else { HALF };
            Vec3::new(side(1), side(2), side(4))
        })
        .collect();
    Ok(Shape3::Hull(Hull::new(&corners)?))
}

/// A prism of `sides` sides (radius 0.5, 1 m long along z).
fn prism(sides: u8) -> Result<Shape3, PhysicsError> {
    let mut points = Vec::new();
    for i in 0..sides {
        let angle = std::f32::consts::TAU * f32::from(i) / f32::from(sides);
        let rim = Vec3::new(
            HALF * base::dmath::cos(angle),
            HALF * base::dmath::sin(angle),
            0.0,
        );
        points.push(rim - Vec3::new(0.0, 0.0, HALF));
        points.push(rim + Vec3::new(0.0, 0.0, HALF));
    }
    Ok(Shape3::Hull(Hull::new(&points)?))
}

/// A rotation of `angle` radians about `axis`.
fn turn(axis: Vec3, angle: f32) -> Result<Quat, PhysicsError> {
    Quat::from_axis_angle(axis, angle).ok_or(PhysicsError::NotFinite)
}

fn add_floor(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        Shape3::Box {
            half_extents: Vec3::new(50.0, 1.0, 50.0),
        },
    ))
}

fn add_body(
    world: &mut World,
    at: Vec3,
    shape: &Shape3,
    configure: impl FnOnce(&mut BodyDesc),
) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, *shape);
    configure(&mut desc);
    world.create_body(&desc)
}

/// The deepest overlap of any contact point, in metres.
fn deepest(world: &World) -> f32 {
    world
        .manifolds()
        .as_slice()
        .iter()
        .flat_map(|entry| entry.manifold.active())
        .map(|point| -point.separation)
        .fold(0.0, f32::max)
}

/// The most points any manifold of the step holds.
fn most_points(world: &World) -> u8 {
    world
        .manifolds()
        .as_slice()
        .iter()
        .map(|entry| entry.manifold.count)
        .max()
        .unwrap_or(0)
}

/// What a column of `rows` cubes of `shape` ends as after `steps` steps (the last 60 sampled for
/// the overlap): the height of the top one and the deepest overlap.
fn column(shape: &Shape3, rows: u8, steps: u32) -> Result<(f32, f32), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    add_floor(&mut w)?;
    let mut top = None;
    for level in 0..rows {
        let height = HALF + f32::from(level) + 0.001;
        top = Some(add_body(
            &mut w,
            Vec3::new(0.0, height, 0.0),
            shape,
            |_| {},
        )?);
    }
    let mut overlap = 0.0_f32;
    for step in 0..steps {
        w.step(DT)?;
        if step + 60 >= steps {
            overlap = overlap.max(deepest(&w));
        }
    }
    let top = top.ok_or(PhysicsError::InvalidShape)?;
    Ok((w.body(top)?.position().y, overlap))
}

/// The most a column of 5 or 10 cubes may sink below its ideal height, in metres (0.0109 and 0.0355
/// measured, for boxes and hulls alike).
const SINK_BOUND: f32 = 0.05;

/// Bound on the difference between a hull cube and a box cube in position and overlap, in metres:
/// the two are the same to within the rounding of the hull's own frame (0.000 measured).
const SAME_AS_BOX: f32 = 1.0e-4;

#[test]
fn a_hull_cube_rests_on_the_floor_as_deep_as_a_box_cube() -> Result<(), PhysicsError> {
    let (box_y, box_overlap) = column(&box_cube(), 1, 300)?;
    let (hull_y, hull_overlap) = column(&hull_cube()?, 1, 300)?;
    assert!(
        (hull_y - box_y).abs() < SAME_AS_BOX,
        "{hull_y} against {box_y}"
    );
    assert!(
        (hull_overlap - box_overlap).abs() < SAME_AS_BOX,
        "{hull_overlap} against {box_overlap}"
    );
    // The overlap is the slop's, about a millimetre (0.00107 measured): not the 13 to 16 mm that the
    // one-point hull of card 8-6 had.
    assert!(hull_overlap < 0.002, "{hull_overlap}");
    Ok(())
}

#[test]
fn a_column_of_hull_cubes_sinks_as_much_as_a_column_of_box_cubes_and_no_more()
-> Result<(), PhysicsError> {
    for rows in [5_u8, 10] {
        let (box_top, box_overlap) = column(&box_cube(), rows, 600)?;
        let (hull_top, hull_overlap) = column(&hull_cube()?, rows, 600)?;
        assert!(
            (hull_top - box_top).abs() < SAME_AS_BOX,
            "{rows} rows: {hull_top} against {box_top}"
        );
        assert!(
            (hull_overlap - box_overlap).abs() < SAME_AS_BOX,
            "{rows} rows: {hull_overlap} against {box_overlap}"
        );
        // The sink is the same for a column of boxes: the parity above is the pin, this is the envelope.
        let sink = f32::from(rows) - HALF - hull_top;
        assert!(sink < SINK_BOUND, "{rows} rows sink {sink} m");
    }
    Ok(())
}

/// How many points the contact of a cube tilted by `degrees` about x has with the floor, one step
/// after it was put a half millimetre above it, with no gravity.
fn points_when_tilted(shape: &Shape3, degrees: f32) -> Result<u8, PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    add_floor(&mut w)?;
    let angle = degrees.to_radians();
    let tilt = turn(Vec3::X, angle)?;
    let half_height = HALF * (base::dmath::cos(angle) + base::dmath::sin(angle));
    add_body(
        &mut w,
        Vec3::new(0.0, half_height + 0.0005, 0.0),
        shape,
        |d| {
            d.rotation = tilt;
        },
    )?;
    w.step(DT)?;
    Ok(most_points(&w))
}

/// The tilts, in tenths of a degree, from 0 to 10 degrees in steps of half a degree.
const TILTS_TENTHS: [u8; 21] = [
    0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100,
];

#[test]
fn a_hull_cube_goes_from_four_contact_points_to_two_at_the_tilt_where_a_box_does()
-> Result<(), PhysicsError> {
    for tenths in TILTS_TENTHS {
        let degrees = f32::from(tenths) / 10.0;
        let on_box = points_when_tilted(&box_cube(), degrees)?;
        let on_hull = points_when_tilted(&hull_cube()?, degrees)?;
        assert_eq!(on_hull, on_box, "tilt {degrees} degrees");
        // Four points flat on the face up to 1 degree, the two points of an edge from 1.5 degrees
        // on: the edge is what touches, which is what a tilted cube does.
        assert_eq!(
            on_hull,
            if tenths <= 10 { 4 } else { 2 },
            "tilt {degrees} degrees"
        );
    }
    Ok(())
}

/// The sides of the prisms (the most a hull holds is 32 corners: 16 sides).
const PRISM_SIDES: [u8; 3] = [8, 12, 16];

/// A prism leaning 0.3 rad onto the floor: the largest move of a contact point between two steps in
/// the steps 300 to 400, how many points it ends with, and how many times their number changed.
fn rest_jump(sides: u8) -> Result<(f32, usize, u32), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    add_floor(&mut w)?;
    let lean = turn(Vec3::Z, 0.3)?;
    add_body(&mut w, Vec3::new(0.0, 0.6, 0.0), &prism(sides)?, |d| {
        d.rotation = lean;
    })?;
    let (mut last, mut jump, mut changes) = (Vec::new(), 0.0_f32, 0_u32);
    for step in 0..400 {
        w.step(DT)?;
        let points = contact_points(&w);
        let moved = largest_move(&last, &points);
        jump = jump.max(if step >= 300 { moved } else { 0.0 });
        changes += u32::from(step >= 300 && points.len() != last.len());
        last = points;
    }
    Ok((jump, last.len(), changes))
}

#[test]
fn the_contact_points_of_a_prism_at_rest_stay_where_they_are() -> Result<(), PhysicsError> {
    for sides in PRISM_SIDES {
        let (jump, points, changes) = rest_jump(sides)?;
        assert_eq!(changes, 0, "{sides} sides: the number of points changed");
        assert!(jump < 1.0e-4, "{sides} sides: a point moved {jump} m");
        assert!(points > 0, "{sides} sides: no contact at rest");
    }
    Ok(())
}

/// The contact points of the step, ordered by position.
fn contact_points(world: &World) -> Vec<Vec3> {
    let mut points: Vec<Vec3> = world
        .manifolds()
        .as_slice()
        .iter()
        .flat_map(|entry| entry.manifold.active().iter().map(|p| p.point))
        .collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
    points
}

/// The largest distance a point moved between two steps (zero when their counts differ: a change of
/// count is a different thing, counted apart).
fn largest_move(before: &[Vec3], after: &[Vec3]) -> f32 {
    if before.len() != after.len() {
        return 0.0;
    }
    before
        .iter()
        .zip(after)
        .map(|(a, b)| (*a - *b).length())
        .fold(0.0, f32::max)
}

/// What a prism that is let roll along x shows over 240 steps: the steps with one point and with
/// none, its highest centre, the farthest it moved sideways, and its peak spin about x and y.
struct Roll {
    one_point_steps: u32,
    no_contact_steps: u32,
    highest: f32,
    sideways: f32,
    cross_spin: f32,
}

fn roll(sides: u8, speed: f32) -> Result<Roll, PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    add_floor(&mut w)?;
    let prism = prism(sides)?;
    let body = add_body(&mut w, Vec3::new(0.0, HALF, 0.0), &prism, |d| {
        d.linear_velocity = Vec3::new(speed, 0.0, 0.0);
        d.angular_velocity = Vec3::new(0.0, 0.0, -2.0 * speed);
    })?;
    let mut found = Roll {
        one_point_steps: 0,
        no_contact_steps: 0,
        highest: 0.0,
        sideways: 0.0,
        cross_spin: 0.0,
    };
    for _ in 0..ROLL_STEPS {
        w.step(DT)?;
        match most_points(&w) {
            0 => found.no_contact_steps += 1,
            1 => found.one_point_steps += 1,
            _ => {}
        }
        let at = w.body(body)?;
        found.highest = found.highest.max(at.position().y);
        found.sideways = found.sideways.max(at.position().z.abs());
        let spin = at.angular_velocity();
        found.cross_spin = found.cross_spin.max(spin.x.abs().max(spin.y.abs()));
    }
    Ok(found)
}

const ROLL_STEPS: u32 = 240;

/// Speeds of the roll in m/s (the spin is twice the speed, so the prism rolls without slipping).
/// Above about 2.2 m/s the prism hops by 3 to 4 mm, before and after card 8-71, so they are left out.
const ROLL_SPEEDS: [f32; 4] = [1.0, 1.4, 1.8, 2.0];

/// Card 8-71. A rolling prism had one point in a quarter of the steps where an edge (two points)
/// touches: 52, 65 and 58 of 240 steps for 8, 12 and 16 sides at 2 m/s, 31 to 70 over the speeds
/// below. The cause was twofold: the normal of EPA at a corner leans past the 1.8 degrees a fitted
/// face allows, so neither shape had a face and the single point stood; and a tilted edge kept only
/// its lower end. Now at most 9 of 240 steps were measured over these 12 scenes; the bound sits between that and
/// the 31 of the old code, because a rolling prism is a chaotic scene.
const ONE_POINT_STEPS: u32 = 20;

/// Steps with no contact at all while the centre stays at the height of the prism. GJK used to miss
/// a touching prism on 0 to 5 of 240 steps (a sliver of its simplex lost its digits over the wide
/// floor, card 8-75); none is missed over these 12 scenes (0 measured, 1 allowed: the scene is
/// chaotic and the machine without FMA was not run).
const NO_CONTACT_STEPS: u32 = 1;

/// The centre never rises more than 2 mm above the radius (0.5 mm measured), sideways drift (11 mm
/// measured) and the peak spin about the other axes (0.23 rad/s measured).
const RISE: f32 = 0.002;
const SIDEWAYS: f32 = 0.03;
const CROSS_SPIN: f32 = 0.6;

#[test]
fn a_rolling_prism_keeps_both_points_of_the_edge_it_touches_and_rolls_straight()
-> Result<(), PhysicsError> {
    for sides in PRISM_SIDES {
        for speed in ROLL_SPEEDS {
            let found = roll(sides, speed)?;
            let scene = format!("{sides} sides at {speed} m/s");
            assert!(
                found.one_point_steps <= ONE_POINT_STEPS,
                "{scene}: {} steps with one point",
                found.one_point_steps
            );
            assert!(
                found.no_contact_steps <= NO_CONTACT_STEPS,
                "{scene}: {} steps with no contact",
                found.no_contact_steps
            );
            assert!(found.highest < HALF + RISE, "{scene}: {} m", found.highest);
            assert!(found.sideways < SIDEWAYS, "{scene}: {}", found.sideways);
            assert!(
                found.cross_spin < CROSS_SPIN,
                "{scene}: {}",
                found.cross_spin
            );
        }
    }
    Ok(())
}

/// Card 8-76. A body that rolls over a corner leaves the ground when it is faster than
/// sqrt(g R): the centre then follows a parabola above the arc of radius R about the corner (here
/// sqrt(9.81 x 0.5) = 2.2 m/s). A rigid-polygon model of the roll (inelastic corner impacts, no
/// sliding) hops the same way: nothing for 8, 12 and 16 sides below 2.2 m/s, and for 3.0 m/s 7.2,
/// 9.0 and 6.3 mm against 8.4, 5.7 and 3.9 mm here. So the hop is the physics of a polygon, not a
/// contact fault. This pins both sides of the threshold (measured: at most 0.54 mm at 2.0 m/s, at
/// least 3.9 mm at 3.0 m/s).
const HOP_FREE_SPEED: f32 = 2.0;
const HOP_SPEED: f32 = 3.0;
const HOP_FREE_RISE: f32 = 0.001;
const HOP_RISE: f32 = 0.002;

#[test]
fn a_prism_rolls_without_a_hop_below_the_speed_a_corner_holds_it_and_hops_above()
-> Result<(), PhysicsError> {
    let critical = (STANDARD_GRAVITY.length() * HALF).sqrt();
    assert!(HOP_FREE_SPEED < critical && critical < HOP_SPEED);
    for sides in PRISM_SIDES {
        let slow = roll(sides, HOP_FREE_SPEED)?;
        assert!(
            slow.highest < HALF + HOP_FREE_RISE,
            "{sides} sides at {HOP_FREE_SPEED} m/s: {} m",
            slow.highest
        );
        let fast = roll(sides, HOP_SPEED)?;
        assert!(
            fast.highest > HALF + HOP_RISE,
            "{sides} sides at {HOP_SPEED} m/s: {} m",
            fast.highest
        );
    }
    Ok(())
}
