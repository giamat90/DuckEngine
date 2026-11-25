//! Card 8-89: capsules lying alongside each other that are a little off parallel. Three logs, two
//! side by side on a floor and one in the groove between them, each turned a few milli-radians about
//! the vertical axis from its neighbour: before the card every pair of logs touched in one point (the
//! far end was not a contact unless the axes were within 1 milli-radian), so the logs rocked on it
//! and the three never slept (0 of 3 asleep after 4000 steps at turns of 0.002 to 0.05).

use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
const LOG_HALF_LENGTH: f32 = 1.0;
const LOG_RADIUS: f32 = 0.25;
/// How far the two lower logs are from each other, centre to centre (a little more than a diameter).
const LOWER_SPACING: f32 = 0.52;
/// The height of the upper log above the lower ones: it sits in the groove between them.
const GROOVE_RISE: f32 = 0.43;
/// Steps within which the three logs must be asleep (measured: 34 to 66 with the card, never before).
const SLEEP_STEPS: u32 = 400;

fn about(axis: Vec3, angle: f32) -> Quat {
    Quat::from_axis_angle(axis, angle).unwrap_or(Quat::IDENTITY)
}

/// A log lying along z, turned by `yaw` about the vertical axis.
fn log(at: Vec3, yaw: f32) -> BodyDesc {
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        at,
        Shape3::Capsule {
            half_height: LOG_HALF_LENGTH,
            radius: LOG_RADIUS,
        },
    );
    desc.rotation = about(Vec3::Y, yaw) * about(Vec3::X, std::f32::consts::FRAC_PI_2);
    desc
}

/// The logs, a floor, and the world that holds them.
fn three_logs(yaw: f32) -> Result<(World, Vec<BodyHandle>), PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 8,
        gravity: STANDARD_GRAVITY,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        Shape3::Box {
            half_extents: Vec3::new(5.0, 0.5, 5.0),
        },
    ))?;
    let rest = LOG_RADIUS;
    let logs = [
        log(Vec3::new(-LOWER_SPACING / 2.0, rest, 0.0), yaw),
        log(Vec3::new(LOWER_SPACING / 2.0, rest, 0.0), -1.7 * yaw),
        log(Vec3::new(0.0, rest + GROOVE_RISE, 0.0), 2.89 * yaw),
    ];
    let mut handles = Vec::new();
    for desc in &logs {
        handles.push(world.create_body(desc)?);
    }
    Ok((world, handles))
}

fn asleep(world: &World, handles: &[BodyHandle]) -> Result<usize, PhysicsError> {
    let mut count = 0;
    for handle in handles {
        count += usize::from(world.is_asleep(*handle)?);
    }
    Ok(count)
}

#[test]
fn three_logs_a_few_milliradians_off_parallel_come_to_rest_and_sleep() -> Result<(), PhysicsError> {
    for yaw in [0.002, 0.005, 0.01, 0.02, 0.05] {
        let (mut world, handles) = three_logs(yaw)?;
        for _ in 0..SLEEP_STEPS {
            world.step(DT)?;
        }
        assert_eq!(
            asleep(&world, &handles)?,
            handles.len(),
            "turn {yaw}: logs asleep"
        );
    }
    Ok(())
}
