//! The chain, bridge and ground scenes shared by the chain and determinism tests (card 4-16).

use super::common::{hinge_at, hinge_error, pin, plank};
use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyHandle, BodyKind, JointHandle, PhysicsError, Rot2, STANDARD_GRAVITY, Shape2,
    World,
};

/// Half extents of a chain link: 0.5 m long, 0.1 m thick.
pub const LINK_HALF: Vec2 = Vec2::new(0.25, 0.05);

/// Bodies and joints of a chain, first link first.
pub struct Chain {
    pub links: Vec<BodyHandle>,
    pub joints: Vec<JointHandle>,
}

/// A horizontal chain of `count` links from `anchor` (a body at `start`), the last link of
/// `last_density` (the others have density 1, 0.05 kg each); one hinge between the anchor and the
/// first link and one between every two links.
pub fn chain(
    world: &mut World,
    anchor: BodyHandle,
    start: Vec2,
    count: u8,
    last_density: f32,
) -> Result<Chain, PhysicsError> {
    let mut links: Vec<BodyHandle> = Vec::new();
    let mut joints = Vec::new();
    let mut previous = anchor;
    for i in 0..count {
        let density = if i + 1 == count { last_density } else { 1.0 };
        let x = start.x + 0.5 * f32::from(i) + LINK_HALF.x;
        let link = plank(world, Vec2::new(x, start.y), LINK_HALF, density)?;
        let at = Vec2::new(x - LINK_HALF.x, start.y);
        joints.push(hinge_at(world, previous, link, at)?);
        links.push(link);
        previous = link;
    }
    Ok(Chain { links, joints })
}

/// A chain of `count` links hanging straight down from `anchor` (a body at `start`): at rest.
pub fn hanging_chain(
    world: &mut World,
    anchor: BodyHandle,
    start: Vec2,
    count: u8,
) -> Result<Chain, PhysicsError> {
    let mut links: Vec<BodyHandle> = Vec::new();
    let mut joints = Vec::new();
    let mut previous = anchor;
    for i in 0..count {
        let top = start.y - 0.5 * f32::from(i);
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(start.x, top - LINK_HALF.x),
            Shape2::Box {
                half_extents: LINK_HALF,
            },
        );
        desc.rotation = Rot2::from_angle(std::f32::consts::FRAC_PI_2);
        let link = world.create_body(&desc)?;
        joints.push(hinge_at(world, previous, link, Vec2::new(start.x, top))?);
        links.push(link);
        previous = link;
    }
    Ok(Chain { links, joints })
}

/// A chain of `count` links lying `height` above the ground (`y = 0`), left end at `x = start`,
/// attached to nothing.
pub fn chain_on_ground(
    world: &mut World,
    start: f32,
    count: u8,
    height: f32,
) -> Result<Chain, PhysicsError> {
    let mut links: Vec<BodyHandle> = Vec::new();
    let mut joints = Vec::new();
    for i in 0..count {
        let x = start + 0.5 * f32::from(i) + LINK_HALF.x;
        let y = LINK_HALF.y + height;
        let link = plank(world, Vec2::new(x, y), LINK_HALF, 1.0)?;
        if let Some(&previous) = links.last() {
            joints.push(hinge_at(
                world,
                previous,
                link,
                Vec2::new(x - LINK_HALF.x, y),
            )?);
        }
        links.push(link);
    }
    Ok(Chain { links, joints })
}

/// A bridge of `planks` planks between two pins `span` metres apart at height `height`, laid
/// along a parabola that sags `sag` metres in the middle; a hinge at every corner of the line.
pub fn bridge(
    world: &mut World,
    span: f32,
    height: f32,
    planks: u8,
    sag: f32,
) -> Result<Chain, PhysicsError> {
    let left = pin(world, 0.0, height)?;
    let right = pin(world, span, height)?;
    let corner = |i: u8| {
        let t = f32::from(i) / f32::from(planks);
        Vec2::new(span * t, height - sag * 4.0 * t * (1.0 - t))
    };
    let mut links: Vec<BodyHandle> = Vec::new();
    let mut joints = Vec::new();
    let mut previous = left;
    for i in 0..planks {
        let (from, to) = (corner(i), corner(i + 1));
        let along = to - from;
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            (from + to) * 0.5,
            Shape2::Box {
                half_extents: Vec2::new(along.length() * 0.5, LINK_HALF.y),
            },
        );
        desc.rotation = Rot2::from_angle(base::dmath::atan2(along.y, along.x));
        let body = world.create_body(&desc)?;
        joints.push(hinge_at(world, previous, body, from)?);
        links.push(body);
        previous = body;
    }
    joints.push(hinge_at(world, previous, right, corner(planks))?);
    Ok(Chain { links, joints })
}

/// The mechanical energy of `bodies` in joules: translation, rotation and height above `y = 0`.
pub fn energy(world: &World, bodies: &[BodyHandle]) -> Result<f32, PhysicsError> {
    let mut total = 0.0;
    for &handle in bodies {
        let body = world.body(handle)?;
        let spin = body.angular_velocity();
        let inertia = 1.0 / body.inverse_inertia();
        let speed = body.linear_velocity().length();
        total += 0.5 * body.mass() * speed * speed
            + 0.5 * inertia * spin * spin
            + body.mass() * -STANDARD_GRAVITY.y * body.position().y;
    }
    Ok(total)
}

/// The largest anchor separation among `joints`.
pub fn worst_hinge_error(world: &World, joints: &[JointHandle]) -> Result<f32, PhysicsError> {
    let mut worst = 0.0_f32;
    for &joint in joints {
        worst = worst.max(hinge_error(world, joint)?);
    }
    Ok(worst)
}
