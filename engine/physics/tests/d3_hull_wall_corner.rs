//! Card 8-82: a hull cube against a wall box and in the corner of a floor and two walls. Card 8-71
//! gave a hull that touches with an edge both ends of the edge (it had one point, which hopped from
//! one end to the other from step to step); in the golden scene "mixed pile 3d" the two hull cubes
//! next to the wall boxes moved differently because of it (card 8-81). These tests pin that the new
//! manifold is the right one there (the two cubes of the golden scene, without the other thirty
//! bodies of the pile): a vertical edge against a wall keeps both ends, as the same
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

/// The distance along `y` between the lowest and the highest of `points`.
fn spread(points: &[Vec3]) -> f32 {
    let low = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let high = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    high - low
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
    // The edge is 0.7 m long: its two ends are about 0.7 m apart, not a single end.
    assert!(spread(&points) > 0.6, "points: {points:?}");
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
    assert_eq!(
        (hull.len(), boxed.len()),
        (2, 2),
        "hull {hull:?}, box {boxed:?}"
    );
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

/// The corner of the golden scene: a floor, two walls and the two hull cubes of pile indices 15 and
/// 31 (one above the other, at the grid corner next to the walls), each turned about `(1, 1, 0)` by
/// the angle of its index. Returns the world, the wall `x = FACE` and the two cubes.
fn corner_world() -> Result<(World, BodyHandle, [BodyHandle; 2]), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    floor(&mut w)?;
    let wall = wall_x(&mut w)?;
    wall_z(&mut w)?;
    let mut cubes = [wall; 2];
    for (slot, (index, height)) in [(15_u8, 1.0_f32), (31, 2.2)].into_iter().enumerate() {
        assert!(
            pile_angle(index) > 0.1,
            "the golden tilt of index {index} is almost none"
        );
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(1.65, height, 1.65),
            hull_cube()?,
        );
        desc.rotation = turn(Vec3::new(1.0, 1.0, 0.0), pile_angle(index))?;
        cubes[slot] = w.create_body(&desc)?;
    }
    Ok((w, wall, cubes))
}

/// A regression guard, not a pin of what card 8-71 changed (it passes with the old code too): the two
/// cubes dropped into the corner come to rest inside it, on the floor, without sinking.
#[test]
fn cubes_dropped_into_a_corner_come_to_rest_inside_it() -> Result<(), PhysicsError> {
    let (mut w, _, cubes) = corner_world()?;
    let mut deepest_after_landing = 0.0_f32;
    for step in 1..=300_u32 {
        w.step(DT)?;
        if step > 150 {
            deepest_after_landing =
                deepest_after_landing.max(w.deepest_overlap().map_or(0.0, |o| o.depth));
        }
    }
    // No cube centre can be closer to a wall than the half side of the cube.
    let limit = FACE - HALF + 0.01;
    for cube in cubes {
        let body = w.body(cube)?;
        let at = body.position();
        assert!(at.x <= limit && at.z <= limit, "inside a wall: {at:?}");
        assert!(at.y > 0.3 && at.y < 1.1, "not resting in the pile: {at:?}");
        let speed_squared = body.linear_velocity().length_squared();
        assert!(
            speed_squared < 0.01,
            "still moving: speed squared {speed_squared}"
        );
    }
    assert!(
        deepest_after_landing < 0.02,
        "sunk {deepest_after_landing} m"
    );
    Ok(())
}

/// The cubes of the golden scene land on the wall with a tilted vertical edge: before card 8-71 each
/// had one point there, the other end of the edge from step to step; now each keeps both ends, on the
/// wall plane, for the first five steps (the probe of card 8-82 shows four points from step 6 on).
#[test]
fn the_corner_cubes_keep_both_ends_of_their_wall_edge_while_they_land() -> Result<(), PhysicsError>
{
    let (mut w, wall, cubes) = corner_world()?;
    for step in 1..=5_u32 {
        w.step(DT)?;
        for cube in cubes {
            let points = points_between(&w, wall, cube);
            assert!(points.len() >= 2, "step {step}: points {points:?}");
            assert!(spread(&points) > 0.5, "step {step}: points {points:?}");
            assert!(
                points.iter().all(|p| (p.x - FACE).abs() < 0.03),
                "step {step}: off the wall: {points:?}"
            );
        }
    }
    Ok(())
}
