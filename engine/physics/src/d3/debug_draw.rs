//! The 3D debug-draw port (card 8-4): the world describes what it holds as lines and points, and
//! an application draws them however it likes, as the 2D port does with `Vec2`. Physics knows no
//! renderer.
//!
//! A sphere is three great circles, a box its twelve edges, a capsule a ring at each end of its
//! cylinder, four lines along it and two half circles over each cap.

pub use crate::core::{DebugColour, NORMAL_LENGTH};
use crate::d3::{Body, BodyKind, Shape3, World};
use base::dmath::{cos, sin};
use base::math::{Quat, Vec3};

/// Segments of a full circle in an outline; a half circle has half as many.
pub const ARC_SEGMENTS: u16 = 16;

/// Where [`draw_debug`] sends what it describes. Calls come in a fixed order: for each body in
/// slot order its outline lines then its centre point, then for each manifold in list order each
/// contact point and its normal.
pub trait IPhysicsDebugDraw {
    /// A line from `from` to `to`, world coordinates.
    fn line(&mut self, from: Vec3, to: Vec3, colour: DebugColour);
    /// A point, world coordinates.
    fn point(&mut self, at: Vec3, colour: DebugColour);
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

/// How a body sits in the world, to turn points of its own frame into world points.
struct Frame {
    centre: Vec3,
    rotation: Quat,
}

impl Frame {
    fn world(&self, local: Vec3) -> Vec3 {
        self.centre + self.rotation.rotate(local)
    }
}

/// A circle in the body's frame: the point at angle `a` is `origin + radius (u cos a + v sin a)`.
struct Ring {
    origin: Vec3,
    u: Vec3,
    v: Vec3,
    radius: f32,
}

impl Ring {
    /// The point `step` segments round the circle.
    fn at(&self, step: u16) -> Vec3 {
        let angle = std::f32::consts::TAU * f32::from(step) / f32::from(ARC_SEGMENTS);
        self.origin + (self.u * cos(angle) + self.v * sin(angle)) * self.radius
    }
}

/// The segments `steps.0..steps.1` of `ring`, each one a line.
fn draw_arc(
    sink: &mut dyn IPhysicsDebugDraw,
    frame: &Frame,
    ring: &Ring,
    steps: (u16, u16),
    colour: DebugColour,
) {
    for step in steps.0..steps.1 {
        sink.line(
            frame.world(ring.at(step)),
            frame.world(ring.at(step + 1)),
            colour,
        );
    }
}

fn basis(axis: usize) -> Vec3 {
    [Vec3::X, Vec3::Y, Vec3::Z][axis % 3]
}

/// The corner of a box with half sides `half` that the three low bits of `index` choose: a set bit
/// is the positive side of that axis.
fn corner(half: Vec3, index: usize) -> Vec3 {
    let side = |bit: usize| if (index >> bit) & 1 == 1 { 1.0 } else { -1.0 };
    Vec3::new(side(0) * half.x, side(1) * half.y, side(2) * half.z)
}

/// The twelve edges of a box: each corner to the corner across each of its negative axes.
fn draw_box(sink: &mut dyn IPhysicsDebugDraw, frame: &Frame, half: Vec3, colour: DebugColour) {
    for index in 0..8_usize {
        for bit in (0..3).filter(|bit| (index >> bit) & 1 == 0) {
            let across = index | (1 << bit);
            sink.line(
                frame.world(corner(half, index)),
                frame.world(corner(half, across)),
                colour,
            );
        }
    }
}

/// Three circles of the sphere's radius, each across one axis of the body.
fn draw_sphere(sink: &mut dyn IPhysicsDebugDraw, frame: &Frame, radius: f32, colour: DebugColour) {
    for axis in 0..3 {
        let ring = Ring {
            origin: Vec3::ZERO,
            u: basis(axis + 1),
            v: basis(axis + 2),
            radius,
        };
        draw_arc(sink, frame, &ring, (0, ARC_SEGMENTS), colour);
    }
}

/// The cylinder part (a ring at each end and a line at each quarter turn) and a half circle in
/// each of two planes over each end.
fn draw_capsule(
    sink: &mut dyn IPhysicsDebugDraw,
    frame: &Frame,
    size: (f32, f32),
    colour: DebugColour,
) {
    let (half_height, radius) = size;
    let ends = [(half_height, 1.0), (-half_height, -1.0)];
    let ring = |y: f32| Ring {
        origin: Vec3::new(0.0, y, 0.0),
        u: Vec3::X,
        v: Vec3::Z,
        radius,
    };
    let (top, bottom) = (ring(half_height), ring(-half_height));
    for ring in [&top, &bottom] {
        draw_arc(sink, frame, ring, (0, ARC_SEGMENTS), colour);
    }
    for quarter in 0..4 {
        let step = quarter * ARC_SEGMENTS / 4;
        sink.line(
            frame.world(top.at(step)),
            frame.world(bottom.at(step)),
            colour,
        );
    }
    for (y, up) in ends {
        for across in [Vec3::X, Vec3::Z] {
            let cap = Ring {
                origin: Vec3::new(0.0, y, 0.0),
                u: across,
                v: Vec3::Y * up,
                radius,
            };
            draw_arc(sink, frame, &cap, (0, ARC_SEGMENTS / 2), colour);
        }
    }
}

fn draw_body(body: &Body, sink: &mut dyn IPhysicsDebugDraw) {
    let frame = Frame {
        centre: body.position(),
        rotation: body.rotation(),
    };
    let colour = colour_of(body);
    match body.shape() {
        Shape3::Sphere { radius } => draw_sphere(sink, &frame, radius, colour),
        Shape3::Box { half_extents } => draw_box(sink, &frame, half_extents, colour),
        Shape3::Capsule {
            half_height,
            radius,
        } => draw_capsule(sink, &frame, (half_height, radius), colour),
    }
    sink.point(frame.centre, colour);
}

fn draw_contacts(world: &World, sink: &mut dyn IPhysicsDebugDraw) {
    for contact in world.manifolds().as_slice() {
        let normal = contact.manifold.normal * NORMAL_LENGTH;
        for point in contact.manifold.active() {
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
