//! What a scene measures while it runs, and the checks that hold the measurements to a bound.
//! Every check prints one `STABILITY | scene | metric | measured | bound` line (visible with
//! `--nocapture`), which is where the tolerance table of `reviews/4-17.md` comes from.

use super::limits::{
    DRIFT, DT, ENERGY_SLACK_PER_BODY, ENERGY_SLACK_RATIO, PENETRATION, REST_SPEED,
};
use super::scene::Scene;
use base::math::Vec2;
use physics::d2::{Joint2, PhysicsError, World};

/// The worst values seen over a run of a scene.
#[derive(Default)]
pub struct Report {
    /// The farthest any tracked body got from where it started, in metres.
    pub max_drift: f32,
    /// The fastest any tracked body moved at any step, in metres per second.
    pub max_speed: f32,
    /// The fastest any tracked body moved after the last step.
    pub final_speed: f32,
    /// The farthest any tracked body got from the point the scene hangs from, in metres.
    pub max_reach: f32,
    /// The deepest contact at any step, in metres (zero when none overlapped).
    pub max_penetration: f32,
    /// The largest separation of the two anchors of a revolute joint at any step, in metres.
    pub worst_hinge: f32,
    pub first_energy: f64,
    pub max_energy: f64,
    pub last_energy: f64,
    /// The number of dynamic bodies in the world.
    pub bodies: usize,
}

/// Steps `scene` for `steps` steps of [`DT`] and reports the worst values.
pub fn observe(scene: &mut Scene, steps: u32) -> Result<Report, PhysicsError> {
    let start = positions(scene)?;
    let first = energy(&scene.world);
    let mut report = Report {
        first_energy: first,
        max_energy: first,
        last_energy: first,
        bodies: dynamic_count(&scene.world),
        ..Report::default()
    };
    for _ in 0..steps {
        scene.world.step(DT)?;
        report.sample(scene, &start)?;
    }
    report.final_speed = fastest(scene)?;
    Ok(report)
}

impl Report {
    fn sample(&mut self, scene: &Scene, start: &[Vec2]) -> Result<(), PhysicsError> {
        for (&handle, &from) in scene.tracked.iter().zip(start) {
            let body = scene.world.body(handle)?;
            self.max_drift = self.max_drift.max((body.position() - from).length());
            self.max_speed = self.max_speed.max(body.linear_velocity().length());
            self.max_reach = self.max_reach.max((body.position() - scene.hang).length());
        }
        self.max_penetration = self.max_penetration.max(deepest(&scene.world));
        self.worst_hinge = self.worst_hinge.max(worst_hinge(&scene.world)?);
        self.last_energy = energy(&scene.world);
        self.max_energy = self.max_energy.max(self.last_energy);
        Ok(())
    }
}

pub fn positions(scene: &Scene) -> Result<Vec<Vec2>, PhysicsError> {
    scene
        .tracked
        .iter()
        .map(|&handle| Ok(scene.world.body(handle)?.position()))
        .collect()
}

/// The fastest tracked body now.
pub fn fastest(scene: &Scene) -> Result<f32, PhysicsError> {
    let mut worst = 0.0_f32;
    for &handle in &scene.tracked {
        worst = worst.max(scene.world.body(handle)?.linear_velocity().length());
    }
    Ok(worst)
}

/// The deepest overlap among the current manifolds, as a positive number (zero when none overlap).
pub fn deepest(world: &World) -> f32 {
    world
        .manifolds()
        .as_slice()
        .iter()
        .flat_map(|entry| entry.manifold.points())
        .map(|point| -point.separation)
        .fold(0.0, f32::max)
}

pub fn dynamic_count(world: &World) -> usize {
    world
        .iter()
        .filter(|(_, body)| body.inverse_mass() > 0.0)
        .count()
}

/// Kinetic plus potential energy of the dynamic bodies in joules (height above `y = 0`).
pub fn energy(world: &World) -> f64 {
    let gravity = f64::from(world.gravity().y);
    world
        .iter()
        .filter(|(_, body)| body.inverse_mass() > 0.0)
        .map(|(_, body)| {
            let mass = f64::from(body.mass());
            let v = body.linear_velocity();
            let w = f64::from(body.angular_velocity());
            let inertia = 1.0 / f64::from(body.inverse_inertia());
            let kinetic = 0.5 * mass * f64::from(v.dot(v)) + 0.5 * inertia * w * w;
            kinetic - mass * gravity * f64::from(body.position().y)
        })
        .sum()
}

/// The largest separation of the anchors of a revolute joint of `world` (zero without joints).
pub fn worst_hinge(world: &World) -> Result<f32, PhysicsError> {
    let mut worst = 0.0_f32;
    for entry in world.joints() {
        if let Joint2::Revolute(shape) = entry.shape {
            let a = anchor(world, entry.first, shape.anchor_a)?;
            let b = anchor(world, entry.second, shape.anchor_b)?;
            worst = worst.max((b - a).length());
        }
    }
    Ok(worst)
}

fn anchor(
    world: &World,
    handle: physics::d2::BodyHandle,
    local: Vec2,
) -> Result<Vec2, PhysicsError> {
    let body = world.body(handle)?;
    Ok(body.position() + body.rotation().rotate(local))
}

/// True when every position, rotation and velocity of `world` is a finite number.
pub fn all_finite(world: &World) -> bool {
    world.iter().all(|(_, body)| {
        let p = body.position();
        let v = body.linear_velocity();
        [
            p.x,
            p.y,
            v.x,
            v.y,
            body.angular_velocity(),
            body.rotation().angle(),
        ]
        .iter()
        .all(|number| number.is_finite())
    })
}

/// How much the energy of a run may rise above its start.
pub fn energy_slack(report: &Report) -> f64 {
    ENERGY_SLACK_RATIO * report.first_energy.abs()
        + ENERGY_SLACK_PER_BODY * f64::from(u32::try_from(report.bodies).unwrap_or(0))
}

/// Prints one line of the tolerance table.
pub fn record(scene: &str, metric: &str, measured: f64, bound: f64) {
    println!("STABILITY | {scene} | {metric} | {measured:.5} | {bound:.5}");
}

/// Records and asserts that `measured` is below `bound`.
pub fn below(scene: &str, metric: &str, measured: f32, bound: f32) {
    record(scene, metric, f64::from(measured), f64::from(bound));
    assert!(
        measured < bound,
        "{scene}: {metric} {measured} is not below {bound}"
    );
}

/// Records and asserts that `measured` is at least `bound`: used only by `findings`, where the
/// bound is the one the solver does not meet.
pub fn above(scene: &str, metric: &str, measured: f32, bound: f32) {
    record(scene, metric, f64::from(measured), f64::from(bound));
    assert!(
        measured >= bound,
        "{scene}: {metric} {measured} is below {bound}: the finding may be fixed, so move the scene back to the settled tests with its original bound"
    );
}

/// The energy of the run never rises above its start by more than the slack, and ends no higher.
pub fn check_energy(scene: &str, report: &Report) {
    let slack = energy_slack(report);
    let rise = report.max_energy - report.first_energy;
    record(scene, "energy rise (J)", rise, slack);
    assert!(
        rise <= slack,
        "{scene}: energy rose by {rise} J (slack {slack})"
    );
    let end = report.last_energy - report.first_energy;
    assert!(end <= slack, "{scene}: energy ends {end} J above its start");
}

/// The checks of a scene that comes to rest: drift, speed at the end, penetration, energy.
pub fn check_resting(scene: &str, report: &Report) {
    below(scene, "max drift (m)", report.max_drift, DRIFT);
    below(scene, "final speed (m/s)", report.final_speed, REST_SPEED);
    below(
        scene,
        "max penetration (m)",
        report.max_penetration,
        PENETRATION,
    );
    check_energy(scene, report);
}
