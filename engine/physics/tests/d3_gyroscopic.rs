//! Card 8-26: the gyroscopic term of the 3D bodies. A torque-free rigid body keeps its angular
//! momentum, not its angular velocity; with unequal principal moments the two differ and the body
//! tumbles. The term is opt-in (`SolverConfig::gyroscopic`, off by default), so every other scene
//! keeps its result bit for bit.

use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    BallJoint3, BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Shape3,
    SleepConfig, SolverConfig, World, WorldConfig,
};

const DT: f32 = 1.0 / 240.0;

/// Steps of two seconds at [`DT`].
const TWO_SECONDS: u32 = 480;

/// Half extents of the box under test. Its principal moments per unit mass are proportional to
/// 1.0625 about x (the middle one), 0.3125 about y (the smallest) and 1.25 about z (the largest).
const HALF: Vec3 = Vec3::new(0.5, 1.0, 0.25);

fn world(gyroscopic: bool) -> Result<World, PhysicsError> {
    World::new(config(gyroscopic))
}

fn config(gyroscopic: bool) -> WorldConfig {
    WorldConfig {
        max_bodies: 4,
        gravity: Vec3::ZERO,
        max_pairs: 16,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig {
            gyroscopic,
            ..SolverConfig::DEFAULT
        },
        sleep: SleepConfig::DEFAULT,
    }
}

fn tumbling_box(w: &mut World, spin: Vec3) -> Result<physics::d3::BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::ZERO,
        Shape3::Box { half_extents: HALF },
    );
    desc.angular_velocity = spin;
    desc.angular_damping = 0.0;
    w.create_body(&desc)
}

/// The angular momentum in world axes: `R diag(I) R^T w`, with `I` the inverse of the stored
/// inverse inertia.
fn angular_momentum(w: &World, handle: physics::d3::BodyHandle) -> Result<Vec3, PhysicsError> {
    let body = w.body(handle)?;
    let q: Quat = body.rotation();
    let inv = body.inverse_inertia();
    let local = q.conjugate().rotate(body.angular_velocity());
    Ok(q.rotate(Vec3::new(local.x / inv.x, local.y / inv.y, local.z / inv.z)))
}

fn run(w: &mut World, steps: u32) -> Result<(), PhysicsError> {
    for _ in 0..steps {
        w.step(DT)?;
    }
    Ok(())
}

#[test]
fn without_the_term_a_torque_free_body_keeps_its_angular_velocity() -> Result<(), PhysicsError> {
    let mut w = world(false)?;
    let spin = Vec3::new(1.0, 4.0, 0.5);
    let handle = tumbling_box(&mut w, spin)?;
    run(&mut w, 480)?;
    assert_eq!(w.body(handle)?.angular_velocity(), spin);
    Ok(())
}

#[test]
fn with_the_term_a_torque_free_body_keeps_its_angular_momentum() -> Result<(), PhysicsError> {
    let mut w = world(true)?;
    let handle = tumbling_box(&mut w, Vec3::new(1.0, 4.0, 0.5))?;
    let before = angular_momentum(&w, handle)?;
    run(&mut w, TWO_SECONDS)?;
    let after = angular_momentum(&w, handle)?;
    let drift = (after - before).length() / before.length();
    // The angular velocity itself has changed a lot (measured 3.96 rad/s): that is the tumbling.
    let moved = (w.body(handle)?.angular_velocity() - Vec3::new(1.0, 4.0, 0.5)).length();
    assert!(moved > 1.0, "the spin barely changed: {moved}");
    // Measured 0.8 % over 2 s (2.6 % with the backward Euler step of card 8-26); the rest is the
    // first-order turn of the orientation (see the module doc of `gyroscopic.rs`).
    assert!(drift < 0.05, "angular momentum drifted by {drift}");
    Ok(())
}

#[test]
fn the_same_body_without_the_term_does_not_keep_its_angular_momentum() -> Result<(), PhysicsError> {
    let mut w = world(false)?;
    let handle = tumbling_box(&mut w, Vec3::new(1.0, 4.0, 0.5))?;
    let before = angular_momentum(&w, handle)?;
    run(&mut w, TWO_SECONDS)?;
    let after = angular_momentum(&w, handle)?;
    let drift = (after - before).length() / before.length();
    // Measured 82 %: the momentum turns with the body while the angular velocity stays put.
    assert!(drift > 0.5, "the control did not drift: {drift}");
    Ok(())
}

#[test]
fn a_spin_about_the_middle_axis_flips_with_the_term_and_never_without_it()
-> Result<(), PhysicsError> {
    // The middle axis of the box is x. A little spin about the others is enough to turn it over.
    let spin = Vec3::new(6.0, 0.05, 0.05);
    let mut flipped_with = false;
    let mut flipped_without = false;
    for (gyroscopic, flipped) in [(true, &mut flipped_with), (false, &mut flipped_without)] {
        let mut w = world(gyroscopic)?;
        let handle = tumbling_box(&mut w, spin)?;
        for _ in 0..TWO_SECONDS {
            w.step(DT)?;
            let local = w
                .body(handle)?
                .rotation()
                .conjugate()
                .rotate(w.body(handle)?.angular_velocity());
            *flipped |= local.x < 0.0;
        }
    }
    assert!(flipped_with, "the body did not turn over with the term");
    // Without the term the spin is constant in the world, so this half is a control by construction.
    assert!(!flipped_without, "the body turned over without the term");
    Ok(())
}

#[test]
fn a_sphere_is_the_same_with_and_without_the_term() -> Result<(), PhysicsError> {
    let spin = Vec3::new(1.0, 4.0, 0.5);
    let mut ends = Vec::new();
    for gyroscopic in [false, true] {
        let mut w = world(gyroscopic)?;
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::ZERO,
            Shape3::Sphere { radius: 0.5 },
        );
        desc.angular_velocity = spin;
        desc.angular_damping = 0.0;
        let handle = w.create_body(&desc)?;
        run(&mut w, 480)?;
        ends.push(w.body(handle)?.angular_velocity());
    }
    assert!((ends[0] - ends[1]).length() < 1e-5, "{ends:?}");
    Ok(())
}

/// The same box with a ball joint through its centre to a static sphere there: the solver sets the
/// body up, so this is the path of a body that a constraint touches. A joint at the centre of mass
/// puts no torque on the body.
fn pinned_box(
    gyroscopic: bool,
    spin: Vec3,
) -> Result<(World, physics::d3::BodyHandle), PhysicsError> {
    let mut w = World::new(WorldConfig {
        max_joints: 1,
        ..config(gyroscopic)
    })?;
    let anchor = w.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::ZERO,
        Shape3::Sphere { radius: 0.1 },
    ))?;
    let handle = tumbling_box(&mut w, spin)?;
    // `JointDesc::new` keeps the two bodies from colliding, so the sphere makes no contact.
    let joint = BallJoint3::new(Vec3::ZERO, Vec3::ZERO);
    w.create_joint(&physics::d3::JointDesc::new(
        anchor,
        handle,
        physics::d3::Joint3::Ball(joint),
    ))?;
    Ok((w, handle))
}

#[test]
fn a_body_the_solver_sets_up_keeps_its_angular_momentum_with_the_term() -> Result<(), PhysicsError>
{
    let spin = Vec3::new(1.0, 4.0, 0.5);
    let (mut w, handle) = pinned_box(true, spin)?;
    let before = angular_momentum(&w, handle)?;
    run(&mut w, TWO_SECONDS)?;
    let drift = (angular_momentum(&w, handle)? - before).length() / before.length();
    let moved = (w.body(handle)?.angular_velocity() - spin).length();
    assert!(moved > 1.0, "the spin barely changed: {moved}");
    assert!(drift < 0.05, "angular momentum drifted by {drift}");
    Ok(())
}

#[test]
fn a_body_the_solver_sets_up_keeps_its_angular_velocity_without_the_term()
-> Result<(), PhysicsError> {
    let spin = Vec3::new(1.0, 4.0, 0.5);
    let (mut w, handle) = pinned_box(false, spin)?;
    run(&mut w, TWO_SECONDS)?;
    let moved = (w.body(handle)?.angular_velocity() - spin).length();
    assert!(moved < 1e-4, "the spin changed by {moved}");
    Ok(())
}

fn energy(w: &World, h: physics::d3::BodyHandle) -> Result<f32, PhysicsError> {
    let b = w.body(h)?;
    let local = b.rotation().conjugate().rotate(b.angular_velocity());
    let inv = b.inverse_inertia();
    Ok(0.5 * (local.x * local.x / inv.x + local.y * local.y / inv.y + local.z * local.z / inv.z))
}

/// Card 8-73: how far the momentum vector and the energy of the tumbling box move over `seconds`
/// at `hz`, for a body nothing touches or one a joint touches (set up by the solver, sub-stepped).
fn tumble_errors(hz: u16, seconds: u16, pinned: bool) -> Result<(f32, f32), PhysicsError> {
    let spin = Vec3::new(1.0, 4.0, 0.5);
    let (mut w, handle) = if pinned {
        pinned_box(true, spin)?
    } else {
        let mut w = world(true)?;
        let handle = tumbling_box(&mut w, spin)?;
        (w, handle)
    };
    let (before, energy_before) = (angular_momentum(&w, handle)?, energy(&w, handle)?);
    let mut energy_error = 0.0_f32;
    for _ in 0..u32::from(hz) * u32::from(seconds) {
        w.step(1.0 / f32::from(hz))?;
        energy_error =
            energy_error.max((energy(&w, handle)? - energy_before).abs() / energy_before);
    }
    let drift = (angular_momentum(&w, handle)? - before).length() / before.length();
    Ok((drift, energy_error))
}

/// Measured: the momentum vector moves 0.8 % at 240 Hz and 3.3 % at 60 Hz for a free body (backward
/// Euler: 2.6 % and 10 %) and the energy to 1.4e-4 (1.3e-3 for a body a joint holds); 10 s at 240 Hz gives 0.8 %, the loss does
/// not grow with time (backward Euler: 7.8 %). What is left is the first-order turn of the
/// orientation, not the gyroscopic term.
#[test]
fn the_midpoint_step_keeps_the_energy_and_loses_little_momentum_at_240_and_60_hz()
-> Result<(), PhysicsError> {
    for (hz, seconds, bound) in [(240, 2, 0.02), (60, 2, 0.07), (240, 10, 0.02)] {
        let (drift, energy_error) = tumble_errors(hz, seconds, false)?;
        assert!(
            drift < bound,
            "{hz} Hz, {seconds} s: momentum moved by {drift}"
        );
        assert!(
            energy_error < 5.0e-4,
            "{hz} Hz, {seconds} s: energy moved by {energy_error}"
        );
    }
    Ok(())
}

#[test]
fn a_body_the_solver_sets_up_keeps_the_energy_and_loses_little_momentum() -> Result<(), PhysicsError>
{
    for (hz, seconds, bound) in [(240, 2, 0.006), (60, 2, 0.02), (240, 10, 0.006)] {
        let (drift, energy_error) = tumble_errors(hz, seconds, true)?;
        assert!(
            drift < bound,
            "{hz} Hz, {seconds} s: momentum moved by {drift}"
        );
        assert!(
            energy_error < 3.0e-3,
            "{hz} Hz, {seconds} s: energy moved by {energy_error}"
        );
    }
    Ok(())
}
