//! Card 8-25: a joint scene that diverges (a body tiny for its arm, see review 8-7) used to end
//! with finite but enormous speeds while `World::step` kept returning `Ok`. A step that would give
//! a body a speed beyond the bound is now refused as a whole, so the caller can see it.

use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    AngleLimits, BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN,
    DEFAULT_SPECULATIVE_MARGIN, HingeJoint3, Joint3, JointDesc, MAX_GIVEN_SPEED, MAX_GIVEN_SPIN,
    MAX_SPEED, MAX_SPIN, STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
const PIVOT_HEIGHT: f32 = 10.0;
/// Steps a scene runs: enough for every scene below to blow up (the first speed above the bound
/// comes within a few dozen steps).
const STEPS: u32 = 600;

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 8,
        gravity,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 4,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DISABLED,
    })
}

/// A cube of half side `half` on a hinge `arm` long, started `turn` rad round, limits +-0.5 rad.
fn tiny_on_a_long_arm(
    gravity: Vec3,
    arm: f32,
    half: f32,
    turn: f32,
) -> Result<(World, BodyHandle), PhysicsError> {
    let mut w = world(gravity)?;
    let pivot = Vec3::new(0.0, PIVOT_HEIGHT, 0.0);
    let pin = w.create_body(&BodyDesc::new(
        BodyKind::Static,
        pivot,
        Shape3::Sphere { radius: 0.1 },
    ))?;
    let rotation = Quat::from_axis_angle(Vec3::Z, turn).unwrap_or(Quat::IDENTITY);
    let door = w.create_body(&BodyDesc {
        rotation,
        ..BodyDesc::new(
            BodyKind::Dynamic,
            pivot + rotation.rotate(Vec3::new(arm, 0.0, 0.0)),
            Shape3::Box {
                half_extents: Vec3::new(half, half, half),
            },
        )
    })?;
    let hinge = HingeJoint3 {
        limits: Some(AngleLimits {
            lower: -0.5,
            upper: 0.5,
        }),
        ..HingeJoint3::new(Vec3::ZERO, Vec3::new(-arm, 0.0, 0.0), Vec3::Z, Vec3::Z)
    };
    w.create_joint(&JointDesc::new(pin, door, Joint3::Hinge(hinge)))?;
    Ok((w, door))
}

/// The largest component of a vector, by magnitude (a length would overflow first).
fn largest(v: Vec3) -> f32 {
    v.x.abs().max(v.y.abs()).max(v.z.abs())
}

/// Scenes found by a scan to end with finite, enormous speeds and `Ok` (arm, half side, turn). They
/// are tuned to diverge: a solver that stays stable on them one day makes the test below red for a
/// good reason, and the scenes are then to be replaced by others that still diverge (the test of a
/// body started beyond the bound does not depend on them).
const SCENES: [(f32, f32, f32); 3] = [(3.0, 0.01, 1.0), (6.0, 0.02, 1.0), (10.0, 0.02, 2.0)];

/// Steps `w` until a step is refused or `STEPS` pass; every step that succeeds must leave the body
/// within the bounds, and a refused step must leave the world as it was. Returns the refusal.
fn run_until_refused(
    w: &mut World,
    door: BodyHandle,
    label: &str,
) -> Result<Option<PhysicsError>, PhysicsError> {
    for step in 0..STEPS {
        let before = w.state_hash();
        if let Err(error) = w.step(DT) {
            assert_eq!(
                w.state_hash(),
                before,
                "{label}: a refused step changed the world"
            );
            return Ok(Some(error));
        }
        let body = w.body(door)?;
        let (v, spin) = (
            largest(body.linear_velocity()),
            largest(body.angular_velocity()),
        );
        assert!(
            v <= MAX_SPEED && spin <= MAX_SPIN,
            "{label}: step {step} is Ok with speed {v:e} spin {spin:e}"
        );
    }
    Ok(None)
}

#[test]
fn a_step_never_succeeds_with_a_speed_beyond_the_bound_and_a_diverging_scene_is_refused()
-> Result<(), PhysicsError> {
    for gravity in [Vec3::ZERO, STANDARD_GRAVITY] {
        for (arm, half, turn) in SCENES {
            let (mut w, door) = tiny_on_a_long_arm(gravity, arm, half, turn)?;
            let label = format!("g {gravity:?} arm {arm} half {half} turn {turn}");
            assert_eq!(
                run_until_refused(&mut w, door, &label)?,
                Some(PhysicsError::Diverged),
                "{label}: how the run ended"
            );
        }
    }
    Ok(())
}

/// The bound is far above anything a scene does: a body falling for a long time, thrown at a
/// thousand metres per second and spun fast still steps.
#[test]
fn a_fast_body_below_the_bound_still_steps() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let fast = w.create_body(&BodyDesc {
        linear_velocity: Vec3::new(1000.0, -1000.0, 0.0),
        angular_velocity: Vec3::new(0.0, 0.0, 500.0),
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, PIVOT_HEIGHT, 0.0),
            Shape3::Sphere { radius: 0.5 },
        )
    })?;
    for _ in 0..60 {
        w.step(DT)?;
    }
    assert!(w.body(fast)?.linear_velocity().length() > 900.0);
    Ok(())
}

/// Card 8-36: a speed above the bound is refused where it is given, at creation and at the
/// setters; exactly the bound is allowed; a refused call changes nothing.
#[test]
fn a_speed_beyond_the_bound_is_refused_at_create_and_at_set_in_3d() -> Result<(), PhysicsError> {
    let desc3 = |linear: Vec3, angular: Vec3| BodyDesc {
        linear_velocity: linear,
        angular_velocity: angular,
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::ZERO,
            Shape3::Sphere { radius: 0.5 },
        )
    };
    let mut w = world(Vec3::ZERO)?;
    for (linear, angular) in [
        (Vec3::new(MAX_GIVEN_SPEED * 1.01, 0.0, 0.0), Vec3::ZERO),
        (Vec3::new(MAX_SPEED, 0.0, 0.0), Vec3::ZERO),
        (
            Vec3::new(0.0, -MAX_GIVEN_SPEED, MAX_GIVEN_SPEED),
            Vec3::ZERO,
        ),
        (Vec3::ZERO, Vec3::new(0.0, MAX_GIVEN_SPIN * 1.01, 0.0)),
        (Vec3::ZERO, Vec3::new(0.0, MAX_SPIN, 0.0)),
    ] {
        assert_eq!(
            w.create_body(&desc3(linear, angular)),
            Err(PhysicsError::SpeedTooHigh)
        );
    }
    // Exactly the largest speed that is given is allowed.
    let body = w.create_body(&desc3(
        Vec3::new(MAX_GIVEN_SPEED, 0.0, 0.0),
        Vec3::new(0.0, MAX_GIVEN_SPIN, 0.0),
    ))?;
    let calm = w.create_body(&desc3(Vec3::ZERO, Vec3::ZERO))?;
    let before = w.state_hash();
    assert_eq!(
        w.set_linear_velocity(calm, Vec3::new(0.0, MAX_GIVEN_SPEED * 1.01, 0.0)),
        Err(PhysicsError::SpeedTooHigh)
    );
    assert_eq!(
        w.set_angular_velocity(calm, Vec3::new(0.0, 0.0, MAX_GIVEN_SPIN * 1.01)),
        Err(PhysicsError::SpeedTooHigh)
    );
    assert_eq!(w.state_hash(), before, "a refused setter changed the world");
    w.set_linear_velocity(calm, Vec3::new(0.0, MAX_GIVEN_SPEED, 0.0))?;
    w.set_angular_velocity(calm, Vec3::new(0.0, 0.0, MAX_GIVEN_SPIN))?;
    assert!(w.body(body).is_ok());
    Ok(())
}

/// The same in 2D, where the angular speed is a scalar (the check is in the shared core).
#[test]
fn a_speed_beyond_the_bound_is_refused_at_create_and_at_set_in_2d() -> Result<(), PhysicsError> {
    let mut w2 = physics::d2::World::new(physics::d2::WorldConfig {
        max_bodies: 4,
        gravity: base::math::Vec2::ZERO,
        max_pairs: 16,
        broadphase_margin: physics::d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: physics::d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: physics::d2::SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DISABLED,
    })?;
    let desc2 = |linear: base::math::Vec2, angular: f32| physics::d2::BodyDesc {
        linear_velocity: linear,
        angular_velocity: angular,
        ..physics::d2::BodyDesc::new(
            physics::d2::BodyKind::Dynamic,
            base::math::Vec2::ZERO,
            physics::d2::Shape2::Circle { radius: 0.5 },
        )
    };
    assert_eq!(
        w2.create_body(&desc2(
            base::math::Vec2::new(MAX_GIVEN_SPEED * 1.01, 0.0),
            0.0
        )),
        Err(PhysicsError::SpeedTooHigh)
    );
    assert_eq!(
        w2.create_body(&desc2(base::math::Vec2::ZERO, -MAX_GIVEN_SPIN * 1.01)),
        Err(PhysicsError::SpeedTooHigh)
    );
    let flat = w2.create_body(&desc2(
        base::math::Vec2::new(MAX_GIVEN_SPEED, 0.0),
        -MAX_GIVEN_SPIN,
    ))?;
    let before = w2.state_hash();
    assert_eq!(
        w2.set_linear_velocity(flat, base::math::Vec2::new(0.0, MAX_GIVEN_SPEED * 1.01)),
        Err(PhysicsError::SpeedTooHigh)
    );
    assert_eq!(
        w2.set_angular_velocity(flat, MAX_GIVEN_SPIN * 1.01),
        Err(PhysicsError::SpeedTooHigh)
    );
    assert_eq!(w2.state_hash(), before);
    Ok(())
}

/// Card 8-46: every speed that `create_body` and the setters accept can be stepped. Before, a body
/// given exactly `MAX_SPEED` was accepted and the step refused it after 24 steps with no contact,
/// because gravity added a little speed to a speed already at the bound. Now the largest given
/// speed leaves room below the bound, in 3D and in 2D.
#[test]
fn a_body_given_the_largest_allowed_speed_can_be_stepped() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    w.create_body(&BodyDesc {
        linear_velocity: Vec3::new(-MAX_GIVEN_SPEED, 0.0, 0.0),
        angular_velocity: Vec3::new(0.0, MAX_GIVEN_SPIN, 0.0),
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(8.0, 2.0, 0.0),
            Shape3::Sphere { radius: 0.1 },
        )
    })?;
    for step in 0..600 {
        assert_eq!(w.step(1.0 / 60.0), Ok(()), "3D step {step}");
    }
    let mut w2 = physics::d2::World::new(physics::d2::WorldConfig {
        max_bodies: 4,
        gravity: physics::d2::STANDARD_GRAVITY,
        max_pairs: 16,
        broadphase_margin: physics::d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: physics::d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: physics::d2::SolverConfig::DEFAULT,
        sleep: physics::d2::SleepConfig::DISABLED,
    })?;
    w2.create_body(&physics::d2::BodyDesc {
        linear_velocity: base::math::Vec2::new(-MAX_GIVEN_SPEED, 0.0),
        angular_velocity: MAX_GIVEN_SPIN,
        ..physics::d2::BodyDesc::new(
            physics::d2::BodyKind::Dynamic,
            base::math::Vec2::new(8.0, 2.0),
            physics::d2::Shape2::Circle { radius: 0.1 },
        )
    })?;
    for step in 0..600 {
        assert_eq!(w2.step(1.0 / 60.0), Ok(()), "2D step {step}");
    }
    Ok(())
}

/// The way out of a refusal of a diverging scene: the world is left as it was, so the cause is
/// still in it and the same call gives the same answer; destroying the body lets the next step
/// succeed.
#[test]
fn a_diverging_world_steps_again_after_the_body_is_destroyed() -> Result<(), PhysicsError> {
    let (mut w, door) = tiny_on_a_long_arm(STANDARD_GRAVITY, 3.0, 0.01, 1.0)?;
    let label = "tiny on a long arm";
    assert_eq!(
        run_until_refused(&mut w, door, label)?,
        Some(PhysicsError::Diverged)
    );
    assert_eq!(
        w.step(DT),
        Err(PhysicsError::Diverged),
        "the same answer again"
    );
    w.destroy_body(door)?;
    assert_eq!(w.step(DT), Ok(()));
    Ok(())
}

/// Card 8-36: a force acts in the next step only. A huge one that makes the step diverge used to
/// stay on the body when the step was refused, so every later step was refused too until the
/// caller applied the opposite force or destroyed the body. A refused step now consumes the
/// forces and torques that were applied for it.
#[test]
fn a_refused_step_drops_the_forces_and_torques_applied_for_it() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let body = w.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::ZERO,
        Shape3::Sphere { radius: 0.5 },
    ))?;
    w.apply_force(body, Vec3::new(1.0e30, 0.0, 0.0))?;
    assert!(w.step(DT).is_err(), "the huge force is refused");
    assert_eq!(
        w.step(DT),
        Ok(()),
        "the force was spent on the refused step"
    );
    assert!(w.body(body)?.linear_velocity().length() < 1.0);

    w.apply_torque(body, Vec3::new(0.0, 1.0e30, 0.0))?;
    assert!(w.step(DT).is_err(), "the huge torque is refused");
    assert_eq!(w.step(DT), Ok(()), "the torque was spent too");

    // A force of the next step is not lost by a step that succeeds, as before.
    w.apply_force(body, Vec3::new(100.0, 0.0, 0.0))?;
    w.step(DT)?;
    assert!(w.body(body)?.linear_velocity().x > 0.0);
    Ok(())
}
