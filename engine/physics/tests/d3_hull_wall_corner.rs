//! Card 8-82: a hull cube against a wall box and in the corner of a floor and two walls. Card 8-71
//! gave a hull that touches with an edge both ends of the edge (it had one point, which hopped from
//! one end to the other from step to step); in the golden scene "mixed pile 3d" the two hull cubes
//! next to the wall boxes moved differently because of it (card 8-81). These tests pin that the new
//! manifold is the right one there: a vertical edge against a wall keeps both ends, as the same
//! cube made of a box does, and a cube dropped into the corner comes to rest inside it.

use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use physics::core::PhysicsError;
use physics::d3::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Hull,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The fixed step: 60 Hz.
const DT: f32 = 1.0 / 60.0;

/// Half the side of the cubes (the pile of the golden scene uses 0.35).
const HALF: f32 = 0.35;

/// Where the inner face of each wall is: the plane `x = FACE` and the plane `z = FACE`.
const FACE: f32 = 2.0;

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 16,
        gravity,
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DISABLED,
    })
}

fn cuboid(half_x: f32, half_y: f32, half_z: f32) -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(half_x, half_y, half_z),
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

fn turn(axis: Vec3, angle: f32) -> Result<Quat, PhysicsError> {
    Quat::from_axis_angle(axis, angle).ok_or(PhysicsError::NotFinite)
}

/// A static wall whose inner face is the plane `x = FACE`.
fn wall_x(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(FACE + 0.5, 2.0, 0.0),
        cuboid(0.5, 2.0, 2.5),
    ))
}

/// A static wall whose inner face is the plane `z = FACE`.
fn wall_z(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, 2.0, FACE + 0.5),
        cuboid(2.5, 2.0, 0.5),
    ))
}

fn floor(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        cuboid(50.0, 1.0, 50.0),
    ))
}

/// The points of the manifold between `wall` and `body` after the last step or `collide`.
fn points_between(world: &World, wall: BodyHandle, body: BodyHandle) -> Vec<Vec3> {
    world
        .manifolds()
        .as_slice()
        .iter()
        .filter(|entry| {
            (entry.first == wall && entry.second == body)
                || (entry.first == body && entry.second == wall)
        })
        .flat_map(|entry| entry.manifold.active().iter().map(|point| point.point))
        .collect()
}

/// A cube turned by 30 degrees about the vertical, so that one vertical edge faces the wall `x = FACE`
/// and just touches it (0.5 mm inside), with `shape` as its shape. No gravity: pairs and one `collide`.
fn edge_points(shape: &Shape3) -> Result<Vec<Vec3>, PhysicsError> {
    let thirty = std::f32::consts::FRAC_PI_6;
    let reach = HALF * (base::dmath::cos(thirty) + base::dmath::sin(thirty));
    let mut w = world(Vec3::ZERO)?;
    let wall = wall_x(&mut w)?;
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::new(FACE - reach + 0.0005, 1.0, 0.0),
        *shape,
    );
    desc.rotation = turn(Vec3::Y, thirty)?;
    let cube = w.create_body(&desc)?;
    w.find_pairs();
    w.collide();
    Ok(points_between(&w, wall, cube))
}

#[test]
fn a_vertical_edge_against_a_wall_keeps_both_ends() -> Result<(), PhysicsError> {
    let points = edge_points(&hull_cube()?)?;
    assert!(points.len() >= 2, "points: {points:?}");
    let low = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let high = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    // The edge is 0.7 m long: its two ends are about 0.7 m apart, not a single end.
    assert!(high - low > 0.6, "ends {low} to {high}");
    assert!(
        points.iter().all(|p| (p.x - FACE).abs() < 0.02),
        "points off the wall: {points:?}"
    );
    Ok(())
}

#[test]
fn a_hull_edge_has_the_points_of_the_same_box_edge() -> Result<(), PhysicsError> {
    let hull = edge_points(&hull_cube()?)?;
    let boxed = edge_points(&cuboid(HALF, HALF, HALF))?;
    assert_eq!(hull.len(), boxed.len(), "hull {hull:?}, box {boxed:?}");
    Ok(())
}

/// The turn of body `index` of the golden pile: its generator (seed 7, stream 21) gives one angle per
/// body, in order.
fn pile_angle(index: u8) -> f32 {
    let mut rng = Pcg32::new(7, 21);
    let mut angle = 0.0;
    for _ in 0..=index {
        angle = rng.next_f32();
    }
    angle
}

/// The cube of pile index 15 of the golden scene: dropped at a corner of the grid, turned about
/// `(1, 1, 0)`, next to two walls.
fn corner_world() -> Result<(World, BodyHandle, BodyHandle), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let wall = wall_x(&mut w)?;
    wall_z(&mut w)?;
    let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec3::new(1.65, 1.0, 1.65), hull_cube()?);
    desc.rotation = turn(Vec3::new(1.0, 1.0, 0.0), pile_angle(15))?;
    let cube = w.create_body(&desc)?;
    Ok((w, wall, cube))
}

#[test]
fn a_cube_dropped_into_a_corner_comes_to_rest_inside_it() -> Result<(), PhysicsError> {
    let (mut w, _, cube) = corner_world()?;
    let mut deepest_after_landing = 0.0_f32;
    for step in 1..=240_u32 {
        w.step(DT)?;
        if step > 90 {
            deepest_after_landing =
                deepest_after_landing.max(w.deepest_overlap().map_or(0.0, |o| o.depth));
        }
    }
    let body = w.body(cube)?;
    let at = body.position();
    let speed = body.linear_velocity().length_squared();
    // Inside the walls (the cube is 0.35 half-side; a turned cube reaches a little further), on the floor.
    assert!(at.x < FACE && at.z < FACE, "outside the corner: {at:?}");
    assert!(at.y > 0.3 && at.y < 0.7, "not on the floor: {at:?}");
    assert!(speed < 0.01, "still moving: speed squared {speed}");
    assert!(
        deepest_after_landing < 0.02,
        "sunk {deepest_after_landing} m"
    );
    Ok(())
}

/// The cube of the golden scene lands on the wall with a tilted vertical edge: before card 8-71 it had
/// one point there, which was the other end of the edge from step to step; now it keeps both ends.
#[test]
fn the_corner_cube_keeps_both_ends_of_its_wall_edge_while_it_lands() -> Result<(), PhysicsError> {
    let (mut w, wall, cube) = corner_world()?;
    for step in 1..=5_u32 {
        w.step(DT)?;
        let points = points_between(&w, wall, cube);
        let low = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let high = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(points.len() >= 2, "step {step}: points {points:?}");
        assert!(high - low > 0.5, "step {step}: ends {low} to {high}");
    }
    Ok(())
}
