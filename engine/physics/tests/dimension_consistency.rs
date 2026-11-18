//! Card 8-8: the 2D instance and the 3D instance agree. Each scene runs in a `d2` world and in a
//! `d3` world whose motion starts in the plane z = 0 (start positions, velocities and orientations
//! in the plane; the box depth and the density are chosen so that mass and the inertia about z
//! equal the 2D body's). The positions and the angle are compared after every step, and so is how
//! far the 3D body drifts out of its plane (its z plus the tilt of its z axis); the worst
//! difference over the run must stay inside the tolerance stated at the scene. The scenes are short
//! and not chaotic. The tolerances are the measured worst difference with a margin of two to three
//! (a margin of about 40 where the measured difference is zero, a floor and not a measurement),
//! and are not to be widened to get green. Only the ratio of inertia to mass shows in these scenes
//! (each has gravity, a static floor or two equal balls), so the mass match is not itself tested.
//!
//! A rolling ball is not a scene here: a circle has the inertia `m r^2 / 2` and a sphere
//! `2 m r^2 / 5`, so they roll differently by design. The scenes below do not depend on that.

use base::dmath::atan2;
use base::math::{Quat, Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

/// The fixed step of every scene: 60 Hz.
const DT: f32 = 1.0 / 60.0;

/// The pose of a body: x, y, the angle about z, and how far it is out of the plane (zero in 2D).
type Pose = [f32; 4];

fn world2(gravity: Vec2, joints: u32) -> Result<d2::World, PhysicsError> {
    d2::World::new(d2::WorldConfig {
        max_bodies: 16,
        gravity,
        max_pairs: 256,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: joints,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })
}

fn world3(gravity: Vec3, joints: u32) -> Result<d3::World, PhysicsError> {
    d3::World::new(d3::WorldConfig {
        max_bodies: 16,
        gravity,
        max_pairs: 256,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        max_joints: joints,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })
}

fn pose2(world: &d2::World, body: d2::BodyHandle) -> Result<Pose, PhysicsError> {
    let b = world.body(body)?;
    Ok([b.position().x, b.position().y, b.rotation().angle(), 0.0])
}

fn pose3(world: &d3::World, body: d3::BodyHandle) -> Result<Pose, PhysicsError> {
    let b = world.body(body)?;
    let x_axis = b.rotation().rotate(Vec3::X);
    let tilt = b.rotation().rotate(Vec3::Z).cross(Vec3::Z).length();
    Ok([
        b.position().x,
        b.position().y,
        atan2(x_axis.y, x_axis.x),
        b.position().z.abs() + tilt,
    ])
}

fn about_z(angle: f32) -> Quat {
    Quat::from_axis_angle(Vec3::Z, angle).unwrap_or(Quat::IDENTITY)
}

/// The largest difference per channel between two runs (the last channel is the drift of the 3D
/// body out of its plane).
fn worst(a: &[Pose], b: &[Pose]) -> Pose {
    let mut out = [0.0_f32; 4];
    for (p, q) in a.iter().zip(b) {
        for k in 0..4 {
            out[k] = out[k].max((p[k] - q[k]).abs());
        }
    }
    out
}

/// Fails when a channel of `diff` is above its tolerance.
fn assert_within(diff: &Pose, tolerance: Pose) {
    let names = ["x", "y", "angle", "out of plane"];
    for k in 0..4 {
        assert!(
            diff[k] <= tolerance[k],
            "{} differs by {} (tolerance {})",
            names[k],
            diff[k],
            tolerance[k]
        );
    }
}

/// Runs `steps` steps of a 2D world and records the pose of `body` after each.
fn trace2(
    world: &mut d2::World,
    body: d2::BodyHandle,
    steps: u32,
) -> Result<Vec<Pose>, PhysicsError> {
    let mut out = Vec::new();
    for _ in 0..steps {
        world.step(DT)?;
        out.push(pose2(world, body)?);
    }
    Ok(out)
}

fn trace3(
    world: &mut d3::World,
    body: d3::BodyHandle,
    steps: u32,
) -> Result<Vec<Pose>, PhysicsError> {
    let mut out = Vec::new();
    for _ in 0..steps {
        world.step(DT)?;
        out.push(pose3(world, body)?);
    }
    Ok(out)
}

fn floor2(world: &mut d2::World) -> Result<(), PhysicsError> {
    world.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::new(0.0, -1.0),
        d2::Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    ))?;
    Ok(())
}

fn floor3(world: &mut d3::World) -> Result<(), PhysicsError> {
    world.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(50.0, 1.0, 50.0),
        },
    ))?;
    Ok(())
}

/// A box of half sizes `(hx, hy)` and depth `2 hz`: 2D density 1 and 3D density `1 / (2 hz)` give
/// the same mass and the same inertia about z.
struct Slab {
    hx: f32,
    hy: f32,
    hz: f32,
}

struct Start {
    position: Vec2,
    angle: f32,
    velocity: Vec2,
    spin: f32,
    friction: f32,
}

fn slab2(world: &mut d2::World, slab: &Slab, s: &Start) -> Result<d2::BodyHandle, PhysicsError> {
    let mut desc = d2::BodyDesc::new(
        d2::BodyKind::Dynamic,
        s.position,
        d2::Shape2::Box {
            half_extents: Vec2::new(slab.hx, slab.hy),
        },
    );
    desc.rotation = d2::Rot2::from_angle(s.angle);
    desc.linear_velocity = s.velocity;
    desc.angular_velocity = s.spin;
    desc.friction = s.friction;
    world.create_body(&desc)
}

fn slab3(world: &mut d3::World, slab: &Slab, s: &Start) -> Result<d3::BodyHandle, PhysicsError> {
    let mut desc = d3::BodyDesc::new(
        d3::BodyKind::Dynamic,
        Vec3::new(s.position.x, s.position.y, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(slab.hx, slab.hy, slab.hz),
        },
    );
    desc.density = 1.0 / (2.0 * slab.hz);
    desc.rotation = about_z(s.angle);
    desc.linear_velocity = Vec3::new(s.velocity.x, s.velocity.y, 0.0);
    desc.angular_velocity = Vec3::new(0.0, 0.0, s.spin);
    desc.friction = s.friction;
    world.create_body(&desc)
}

/// Runs a slab scene in both dimensions on a floor and returns the worst differences.
/// Measured with the scenes below; the tolerances are two to three times the measured worst difference.
fn slab_scene(start: &Start, slab: &Slab, steps: u32) -> Result<(Pose, Vec<Pose>), PhysicsError> {
    let mut w2 = world2(d2::STANDARD_GRAVITY, 0)?;
    floor2(&mut w2)?;
    let b2 = slab2(&mut w2, slab, start)?;
    let mut w3 = world3(d3::STANDARD_GRAVITY, 0)?;
    floor3(&mut w3)?;
    let b3 = slab3(&mut w3, slab, start)?;
    let flat = trace2(&mut w2, b2, steps)?;
    let solid = trace3(&mut w3, b3, steps)?;
    Ok((worst(&flat, &solid), flat))
}

#[test]
fn a_spinning_box_dropped_on_the_floor_lands_the_same_in_both_dimensions()
-> Result<(), PhysicsError> {
    let start = Start {
        position: Vec2::new(0.0, 1.5),
        angle: 0.3,
        velocity: Vec2::new(1.0, 0.0),
        spin: 1.0,
        friction: 0.6,
    };
    let slab = Slab {
        hx: 0.5,
        hy: 0.25,
        hz: 0.5,
    };
    let (diff, flat) = slab_scene(&start, &slab, 120)?;
    // The box lands flat after the tumble and has slid on: this is a scene with something in it.
    let end = flat.last().copied().unwrap_or_default();
    assert!(
        end[0] > 0.5 && (end[1] - 0.25).abs() < 0.01 && end[2].abs() < 0.01,
        "{end:?}"
    );
    // Measured worst differences: x 0.025, y 0.011, angle 0.027, out of plane 0.044.
    assert_within(&diff, [0.05, 0.025, 0.055, 0.09]);
    Ok(())
}

#[test]
fn a_sliding_box_stops_at_the_same_place_in_both_dimensions() -> Result<(), PhysicsError> {
    let start = Start {
        position: Vec2::new(0.0, 0.25),
        angle: 0.0,
        velocity: Vec2::new(3.0, 0.0),
        spin: 0.0,
        friction: 0.5,
    };
    let slab = Slab {
        hx: 0.5,
        hy: 0.25,
        hz: 0.5,
    };
    let (diff, flat) = slab_scene(&start, &slab, 90)?;
    // Friction 0.5 against 0.6 combines to about 0.548: a stop after 3^2 / (2 * 0.548 * g) = 0.84 m.
    let stop = flat.last().copied().unwrap_or_default();
    assert!(
        (stop[0] - 0.84).abs() < 0.03,
        "the 2D box stops at {}",
        stop[0]
    );
    // Measured worst differences: x 0.029, y 0.0003, angle 0.0008, out of plane 0.0069.
    assert_within(&diff, [0.06, 0.001, 0.002, 0.015]);
    Ok(())
}

/// A circle of radius 0.5 in 2D, the sphere of the same radius in 3D, with no friction and full
/// restitution: a frictionless contact gives no torque, so the different inertia does not matter.
fn ball2(world: &mut d2::World, at: Vec2, velocity: Vec2) -> Result<d2::BodyHandle, PhysicsError> {
    let mut desc = d2::BodyDesc::new(
        d2::BodyKind::Dynamic,
        at,
        d2::Shape2::Circle { radius: 0.5 },
    );
    desc.linear_velocity = velocity;
    desc.friction = 0.0;
    desc.restitution = 1.0;
    world.create_body(&desc)
}

fn ball3(world: &mut d3::World, at: Vec2, velocity: Vec2) -> Result<d3::BodyHandle, PhysicsError> {
    let mut desc = d3::BodyDesc::new(
        d3::BodyKind::Dynamic,
        Vec3::new(at.x, at.y, 0.0),
        d3::Shape3::Sphere { radius: 0.5 },
    );
    desc.linear_velocity = Vec3::new(velocity.x, velocity.y, 0.0);
    desc.friction = 0.0;
    desc.restitution = 1.0;
    world.create_body(&desc)
}

/// The poses of two bodies of a 2D world after every step.
fn two2(
    w: &mut d2::World,
    a: d2::BodyHandle,
    b: d2::BodyHandle,
    steps: u32,
) -> Result<[Vec<Pose>; 2], PhysicsError> {
    let (mut first, mut second) = (Vec::new(), Vec::new());
    for _ in 0..steps {
        w.step(DT)?;
        first.push(pose2(w, a)?);
        second.push(pose2(w, b)?);
    }
    Ok([first, second])
}

fn two3(
    w: &mut d3::World,
    a: d3::BodyHandle,
    b: d3::BodyHandle,
    steps: u32,
) -> Result<[Vec<Pose>; 2], PhysicsError> {
    let (mut first, mut second) = (Vec::new(), Vec::new());
    for _ in 0..steps {
        w.step(DT)?;
        first.push(pose3(w, a)?);
        second.push(pose3(w, b)?);
    }
    Ok([first, second])
}

#[test]
fn an_elastic_glancing_collision_sends_both_balls_the_same_way_in_both_dimensions()
-> Result<(), PhysicsError> {
    let (from, at) = (Vec2::new(-3.0, 0.0), Vec2::new(0.0, 0.4));
    let mut w2 = world2(Vec2::ZERO, 0)?;
    let (hit2, target2) = (
        ball2(&mut w2, from, Vec2::new(4.0, 0.0))?,
        ball2(&mut w2, at, Vec2::ZERO)?,
    );
    let mut w3 = world3(Vec3::ZERO, 0)?;
    let (hit3, target3) = (
        ball3(&mut w3, from, Vec2::new(4.0, 0.0))?,
        ball3(&mut w3, at, Vec2::ZERO)?,
    );
    let flat = two2(&mut w2, hit2, target2, 90)?;
    let solid = two3(&mut w3, hit3, target3, 90)?;
    let diff = [worst(&flat[0], &solid[0]), worst(&flat[1], &solid[1])];
    // The target is knocked up and away and the hitter deflected down: the collision happened.
    let (hit_end, target_end) = (flat[0][89], flat[1][89]);
    assert!(
        target_end[0] > 2.0 && target_end[1] > 1.0 && hit_end[1] < -1.0,
        "{hit_end:?} {target_end:?}"
    );
    // Measured: no difference in x and y, 2.6e-7 in the angle (a circle and a sphere meet alike).
    for d in diff {
        assert_within(&d, [1.0e-5, 1.0e-5, 1.0e-5, 1.0e-5]);
    }
    Ok(())
}

#[test]
fn a_box_swinging_on_a_hinge_follows_the_same_arc_in_both_dimensions() -> Result<(), PhysicsError> {
    let half = 0.1;
    let mut w2 = world2(d2::STANDARD_GRAVITY, 1)?;
    let pin2 = w2.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::ZERO,
        d2::Shape2::Circle { radius: 0.05 },
    ))?;
    let mut desc2 = d2::BodyDesc::new(
        d2::BodyKind::Dynamic,
        Vec2::new(1.0, 0.0),
        d2::Shape2::Box {
            half_extents: Vec2::new(half, half),
        },
    );
    desc2.friction = 0.0;
    let bob2 = w2.create_body(&desc2)?;
    let hinge2 = d2::RevoluteJoint2::new(Vec2::ZERO, Vec2::new(-1.0, 0.0));
    w2.create_joint(&d2::JointDesc::new(
        pin2,
        bob2,
        d2::Joint2::Revolute(hinge2),
    ))?;

    let mut w3 = world3(d3::STANDARD_GRAVITY, 1)?;
    let pin3 = w3.create_body(&d3::BodyDesc::new(
        d3::BodyKind::Static,
        Vec3::ZERO,
        d3::Shape3::Sphere { radius: 0.05 },
    ))?;
    let mut desc3 = d3::BodyDesc::new(
        d3::BodyKind::Dynamic,
        Vec3::new(1.0, 0.0, 0.0),
        d3::Shape3::Box {
            half_extents: Vec3::new(half, half, half),
        },
    );
    desc3.density = 1.0 / (2.0 * half);
    desc3.friction = 0.0;
    let bob3 = w3.create_body(&desc3)?;
    let hinge3 = d3::HingeJoint3::new(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0), Vec3::Z, Vec3::Z);
    w3.create_joint(&d3::JointDesc::new(pin3, bob3, d3::Joint3::Hinge(hinge3)))?;

    let flat = trace2(&mut w2, bob2, 180)?;
    let solid = trace3(&mut w3, bob3, 180)?;
    // The arm has swung well past the horizontal, so the scene has motion in it.
    let end = flat.last().copied().unwrap_or_default();
    assert!(end[1] < -0.9 && end[2].abs() > 1.0, "{end:?}");
    // Measured worst differences: x 1.0e-4, y 0.8e-4, angle 1.1e-4, out of plane 0.
    assert_within(&worst(&flat, &solid), [2.0e-4, 2.0e-4, 2.2e-4, 1.0e-4]);
    Ok(())
}
