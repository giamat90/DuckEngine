//! Scene-building helpers shared by the joint tests. Everything returns a `Result`, because the
//! lints deny `unwrap` and `expect`.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    DistanceJoint2, Joint2, JointDesc, JointHandle, PhysicsError, RevoluteJoint2, STANDARD_GRAVITY,
    Shape2, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The fixed step of every joint scene: 60 Hz.
pub const DT: f32 = 1.0 / 60.0;

/// The configuration the joint scenes use: room for every pair, manifold and joint they make.
pub fn config(max_bodies: u32, max_joints: u32, gravity: Vec2, sleep: SleepConfig) -> WorldConfig {
    WorldConfig {
        max_bodies,
        gravity,
        max_pairs: 4096,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 4096,
        max_joints,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    }
}

/// A world with 64 bodies and 64 joints, standard gravity, sleeping off.
pub fn world() -> Result<World, PhysicsError> {
    World::new(config(64, 64, STANDARD_GRAVITY, SleepConfig::DISABLED))
}

/// As [`world`] with another gravity.
pub fn world_with_gravity(gravity: Vec2) -> Result<World, PhysicsError> {
    World::new(config(64, 64, gravity, SleepConfig::DISABLED))
}

/// A static circle of radius 0.1 at `(x, y)`: a point to hang things from.
pub fn pin(world: &mut World, x: f32, y: f32) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(x, y),
        Shape2::Circle { radius: 0.1 },
    ))
}

/// A static box whose top face is the line `y = 0`, 100 m wide.
pub fn ground(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    ))
}

/// A dynamic box of the given half extents and density at `(x, y)`.
pub fn plank(
    world: &mut World,
    at: Vec2,
    half: Vec2,
    density: f32,
) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(BodyKind::Dynamic, at, Shape2::Box { half_extents: half });
    desc.density = density;
    world.create_body(&desc)
}

/// A dynamic circle of density 1 at `(x, y)`.
pub fn ball(world: &mut World, radius: f32, at: Vec2) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        at,
        Shape2::Circle { radius },
    ))
}

/// A hinge between two anchors given in the frame of each body.
pub fn hinge(
    world: &mut World,
    first: BodyHandle,
    second: BodyHandle,
    anchor_a: Vec2,
    anchor_b: Vec2,
) -> Result<JointHandle, PhysicsError> {
    world.create_joint(&JointDesc::new(
        first,
        second,
        Joint2::Revolute(RevoluteJoint2::new(anchor_a, anchor_b)),
    ))
}

/// `point` of the world as a point of the frame of `body`, relative to its centre.
pub fn local_point(world: &World, body: BodyHandle, point: Vec2) -> Result<Vec2, PhysicsError> {
    let body = world.body(body)?;
    let (cos, sin) = body.rotation().direction();
    let offset = point - body.position();
    Ok(Vec2::new(
        cos * offset.x + sin * offset.y,
        cos * offset.y - sin * offset.x,
    ))
}

/// A hinge at the world point `point`, whichever way the two bodies are placed.
pub fn hinge_at(
    world: &mut World,
    first: BodyHandle,
    second: BodyHandle,
    point: Vec2,
) -> Result<JointHandle, PhysicsError> {
    let anchor_a = local_point(world, first, point)?;
    let anchor_b = local_point(world, second, point)?;
    hinge(world, first, second, anchor_a, anchor_b)
}

/// A hinge at the midpoint of the centres of the two bodies.
pub fn hinge_between(
    world: &mut World,
    first: BodyHandle,
    second: BodyHandle,
) -> Result<JointHandle, PhysicsError> {
    let middle = (world.body(first)?.position() + world.body(second)?.position()) * 0.5;
    hinge_at(world, first, second, middle)
}

/// A rigid rod of `length` between two anchors.
pub fn rod(
    world: &mut World,
    first: BodyHandle,
    second: BodyHandle,
    anchors: (Vec2, Vec2),
    length: f32,
) -> Result<JointHandle, PhysicsError> {
    world.create_joint(&JointDesc::new(
        first,
        second,
        Joint2::Distance(DistanceJoint2::new(anchors.0, anchors.1, length)),
    ))
}

/// Where the point `local` of `body` is in the world.
pub fn world_point(world: &World, body: BodyHandle, local: Vec2) -> Result<Vec2, PhysicsError> {
    let body = world.body(body)?;
    Ok(body.position() + body.rotation().rotate(local))
}

/// How far apart the two anchors of a revolute joint are: zero when the hinge holds.
pub fn hinge_error(world: &World, joint: JointHandle) -> Result<f32, PhysicsError> {
    let entry = *world.joint(joint)?;
    let Joint2::Revolute(shape) = entry.shape else {
        return Err(PhysicsError::InvalidJoint);
    };
    let a = world_point(world, entry.first, shape.anchor_a)?;
    let b = world_point(world, entry.second, shape.anchor_b)?;
    Ok((b - a).length())
}

/// How far the distance between the two anchors of a rod is from its rest length.
pub fn rod_error(world: &World, joint: JointHandle) -> Result<f32, PhysicsError> {
    let entry = *world.joint(joint)?;
    let Joint2::Distance(shape) = entry.shape else {
        return Err(PhysicsError::InvalidJoint);
    };
    let a = world_point(world, entry.first, shape.anchor_a)?;
    let b = world_point(world, entry.second, shape.anchor_b)?;
    Ok(((b - a).length() - shape.length).abs())
}

/// The angle of a body in radians, in `(-pi, pi]`.
pub fn angle(world: &World, body: BodyHandle) -> Result<f32, PhysicsError> {
    Ok(world.body(body)?.rotation().angle())
}

pub fn step_n(world: &mut World, steps: u32) -> Result<(), PhysicsError> {
    for _ in 0..steps {
        world.step(DT)?;
    }
    Ok(())
}

/// The time of the first crossing of zero by `samples` (one per step, from time 0) in the downward
/// direction at or after the index `from`, by linear interpolation; and the index after it.
pub fn downward_crossing(samples: &[f32], from: usize) -> Option<(f32, usize)> {
    samples
        .windows(2)
        .enumerate()
        .skip(from)
        .find(|(_, pair)| pair[0] > 0.0 && pair[1] <= 0.0)
        .map(|(index, pair)| {
            let fraction = pair[0] / (pair[0] - pair[1]);
            (
                (f32::from(u16::try_from(index).unwrap_or(u16::MAX)) + fraction) * DT,
                index + 1,
            )
        })
}
