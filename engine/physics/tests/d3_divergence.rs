//! Card 8-25: a joint scene that diverges (a body tiny for its arm, see review 8-7) used to end
//! with finite but enormous speeds while `World::step` kept returning `Ok`. A step that would give
//! a body a speed beyond the bound is now refused as a whole, so the caller can see it.

use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    AngleLimits, BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN,
    DEFAULT_SPECULATIVE_MARGIN, HingeJoint3, Joint3, JointDesc, MAX_SPEED, MAX_SPIN,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
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

/// A body made to move faster than the bound allows is refused at the first step and the world is
/// left as it was, in both dimensions (the check is in the shared core).
#[test]
fn a_body_started_beyond_the_bound_is_refused_in_3d_and_in_2d() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    w.create_body(&BodyDesc {
        linear_velocity: Vec3::new(MAX_SPEED * 1.01, 0.0, 0.0),
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::ZERO,
            Shape3::Sphere { radius: 0.5 },
        )
    })?;
    let before = w.state_hash();
    assert_eq!(w.step(DT), Err(PhysicsError::Diverged));
    assert_eq!(w.state_hash(), before);

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
    w2.create_body(&physics::d2::BodyDesc {
        angular_velocity: MAX_SPIN * 1.01,
        ..physics::d2::BodyDesc::new(
            physics::d2::BodyKind::Dynamic,
            base::math::Vec2::ZERO,
            physics::d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    let before = w2.state_hash();
    assert_eq!(w2.step(DT), Err(PhysicsError::Diverged));
    assert_eq!(w2.state_hash(), before);
    Ok(())
}

/// The way out of a refusal (the world is left as it was, so the cause is still in it): setting a
/// speed below the bound, or destroying the body, lets the next step succeed.
#[test]
fn a_refused_world_steps_again_after_the_speed_is_set_down_or_the_body_is_destroyed()
-> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let fast = w.create_body(&BodyDesc {
        linear_velocity: Vec3::new(MAX_SPEED * 2.0, 0.0, 0.0),
        ..BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::ZERO,
            Shape3::Sphere { radius: 0.5 },
        )
    })?;
    assert_eq!(w.step(DT), Err(PhysicsError::Diverged));
    assert_eq!(
        w.step(DT),
        Err(PhysicsError::Diverged),
        "the same answer again"
    );
    w.set_linear_velocity(fast, Vec3::new(10.0, 0.0, 0.0))?;
    assert_eq!(w.step(DT), Ok(()));

    w.set_linear_velocity(fast, Vec3::new(0.0, MAX_SPEED * 2.0, 0.0))?;
    assert_eq!(w.step(DT), Err(PhysicsError::Diverged));
    w.destroy_body(fast)?;
    assert_eq!(w.step(DT), Ok(()));
    Ok(())
}
