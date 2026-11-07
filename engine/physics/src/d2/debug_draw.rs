//! The debug-draw port (card 4-24): the world describes what it holds as lines and points, and an
//! application draws them however it likes. Physics knows no renderer; the sample implements
//! [`IPhysicsDebugDraw`] with the sprite batcher.

use crate::d2::{Body, BodyKind, Rot2, Shape2, World};
use base::math::Vec2;

/// What a drawn line or point stands for. The application picks the colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugColour {
    /// A dynamic body that is awake.
    Awake,
    /// A dynamic body that sleeps.
    Asleep,
    /// A static body.
    Static,
    /// A contact point of a manifold.
    ContactPoint,
    /// The normal of a manifold, drawn from each of its points.
    ContactNormal,
}

/// Segments of a circle's outline.
pub const CIRCLE_SEGMENTS: u16 = 16;
/// Length in metres of the line drawn for a contact normal.
pub const NORMAL_LENGTH: f32 = 0.25;

/// Where [`draw_debug`] sends what it describes. Calls come in a fixed order: for each body in
/// slot order its outline lines then its centre point, then for each manifold in list order each
/// contact point and its normal.
pub trait IPhysicsDebugDraw {
    /// A line from `a` to `b`, world coordinates.
    fn line(&mut self, a: Vec2, b: Vec2, colour: DebugColour);
    /// A point, world coordinates.
    fn point(&mut self, p: Vec2, colour: DebugColour);
}

/// The role of a body's lines: static, asleep or awake.
fn colour_of(body: &Body) -> DebugColour {
    if body.kind() == BodyKind::Static {
        DebugColour::Static
    } else if body.is_asleep() {
        DebugColour::Asleep
    } else {
        DebugColour::Awake
    }
}

/// The four edges of a box with half sides `half`, counter-clockwise from the corner (-x, -y).
fn draw_box(
    sink: &mut dyn IPhysicsDebugDraw,
    centre: Vec2,
    rotation: Rot2,
    half: Vec2,
    colour: DebugColour,
) {
    let corners = [
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(-half.x, half.y),
    ]
    .map(|corner| centre + rotation.rotate(corner));
    for (index, from) in corners.iter().enumerate() {
        let to = corners[(index + 1) % corners.len()];
        sink.line(*from, to, colour);
    }
}

/// A ring of [`CIRCLE_SEGMENTS`] edges starting on the body's x axis, then a spoke from the centre
/// to the rim along that axis, which shows how the disc turns.
fn draw_circle(
    sink: &mut dyn IPhysicsDebugDraw,
    centre: Vec2,
    rotation: Rot2,
    radius: f32,
    colour: DebugColour,
) {
    let turn = Rot2::from_angle(std::f32::consts::TAU / f32::from(CIRCLE_SEGMENTS));
    let mut direction = rotation;
    let rim = |direction: Rot2| centre + direction.rotate(Vec2::new(radius, 0.0));
    for _ in 0..CIRCLE_SEGMENTS {
        let next = direction.turned_by(turn);
        sink.line(rim(direction), rim(next), colour);
        direction = next;
    }
    sink.line(centre, rim(rotation), colour);
}

fn draw_body(body: &Body, sink: &mut dyn IPhysicsDebugDraw) {
    let (centre, rotation, colour) = (body.position(), body.rotation(), colour_of(body));
    match body.shape() {
        Shape2::Box { half_extents } => draw_box(sink, centre, rotation, half_extents, colour),
        Shape2::Circle { radius } => draw_circle(sink, centre, rotation, radius, colour),
    }
    sink.point(centre, colour);
}

fn draw_contacts(world: &World, sink: &mut dyn IPhysicsDebugDraw) {
    for contact in world.manifolds().as_slice() {
        let normal = contact.manifold.normal * NORMAL_LENGTH;
        for point in contact.manifold.points() {
            sink.point(point.point, DebugColour::ContactPoint);
            sink.line(
                point.point,
                point.point + normal,
                DebugColour::ContactNormal,
            );
        }
    }
}

/// Describes the bodies and the contacts of `world` to `sink`. Allocates nothing.
pub fn draw_debug(world: &World, sink: &mut dyn IPhysicsDebugDraw) {
    for (_, body) in world.iter() {
        draw_body(body, sink);
    }
    draw_contacts(world, sink);
}
