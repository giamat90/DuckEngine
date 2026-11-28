//! A 3D body as the solver sees it: velocities, the motion since the start of the step, and the
//! terms that stay the same over the sub-steps. The inverse inertia is the world-space matrix of the
//! start of the step, so a body that turns inside one step is solved with the inertia it began with
//! (the same simplification as the effective masses of the contact points).

use crate::core::{Body, Dim, damping_factor};
use crate::d3::D3;
use base::math::{Mat3, Quat, Vec3};

/// What the solver needs of the step to set a body up.
#[derive(Clone, Copy)]
pub(super) struct StepTerms {
    pub gravity: Vec3,
    /// Length of a sub-step.
    pub h: f32,
    /// Whether a body keeps its angular momentum (card 8-26).
    pub gyroscopic: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SolverBody {
    pub velocity: Vec3,
    pub spin: Vec3,
    /// Translation since the start of the step.
    pub shift: Vec3,
    /// Rotation since the start of the step.
    pub turn: Quat,
    pub inv_mass: f32,
    /// The inverse inertia tensor in world axes.
    pub inv_inertia: Mat3,
    accel: Vec3,
    spin_accel: Vec3,
    /// The orientation at the start of the step and the inverse principal moments, for the
    /// gyroscopic term; the moments are zero when the term is off, which leaves the spin alone.
    start: Quat,
    principal: Vec3,
    linear_factor: f32,
    angular_factor: f32,
}

impl SolverBody {
    pub(super) fn at_rest() -> Self {
        Self {
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            shift: Vec3::ZERO,
            turn: Quat::IDENTITY,
            inv_mass: 0.0,
            inv_inertia: Mat3::from_scale(Vec3::ZERO),
            accel: Vec3::ZERO,
            spin_accel: Vec3::ZERO,
            start: Quat::IDENTITY,
            principal: Vec3::ZERO,
            linear_factor: 1.0,
            angular_factor: 1.0,
        }
    }

    pub(super) fn new(body: &Body<D3>, terms: StepTerms) -> Self {
        let inv_mass = body.inverse_mass();
        let rotation = body.rotation();
        let turned = rotation.to_mat3();
        let inv_inertia = turned * Mat3::from_scale(body.inverse_inertia()) * turned.transpose();
        Self {
            velocity: body.linear_velocity(),
            spin: body.angular_velocity(),
            inv_mass,
            inv_inertia,
            accel: terms.gravity + body.force() * inv_mass,
            spin_accel: D3::apply_inverse_inertia(rotation, body.inverse_inertia(), body.torque()),
            start: rotation,
            principal: if terms.gyroscopic {
                body.inverse_inertia()
            } else {
                Vec3::ZERO
            },
            linear_factor: damping_factor(body.linear_damping(), terms.h),
            angular_factor: damping_factor(body.angular_damping(), terms.h),
            ..Self::at_rest()
        }
    }

    /// Gravity, forces and damping over `h` seconds (semi-implicit Euler, as `Body::integrate`).
    pub(super) fn integrate_velocity(&mut self, h: f32) {
        self.velocity = (self.velocity + self.accel * h) * self.linear_factor;
        let mut spin = self.spin + self.spin_accel * h;
        if self.principal != Vec3::ZERO {
            spin = D3::gyroscopic_spin(self.turn * self.start, self.principal, spin, h);
        }
        self.spin = spin * self.angular_factor;
    }

    /// Moves the body by its velocities over `h` seconds.
    pub(super) fn integrate_position(&mut self, h: f32) {
        self.shift += self.velocity * h;
        self.turn = D3::advance_rotation(self.turn, self.spin, h);
    }

    /// The velocity of the point `anchor` away from the centre.
    pub(super) fn velocity_at(&self, anchor: Vec3) -> Vec3 {
        self.velocity + self.spin.cross(anchor)
    }

    /// Applies `impulse` at the point `anchor` away from the centre.
    pub(super) fn push(&mut self, anchor: Vec3, impulse: Vec3) {
        self.velocity += impulse * self.inv_mass;
        self.spin += self.inv_inertia * anchor.cross(impulse);
    }

    /// Takes the velocity and the spin of `other` and nothing else. They are the only two things a
    /// pass over a colour changes (card 8-105), so the staged colour solve stores these 24 bytes of
    /// a body back instead of the whole of it.
    pub(super) fn take_velocities(&mut self, other: &SolverBody) {
        self.velocity = other.velocity;
        self.spin = other.spin;
    }

    pub(super) fn is_static(&self) -> bool {
        self.inv_mass == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3::{BodyDesc, BodyKind, Shape3, World, WorldConfig};

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() <= 1.0e-5 * b.length().max(1.0)
    }

    fn moving() -> SolverBody {
        let mut body = SolverBody::at_rest();
        body.velocity = Vec3::new(1.0, 2.0, 3.0);
        body.spin = Vec3::new(0.5, -1.0, 2.0);
        body.accel = Vec3::new(0.0, -10.0, 4.0);
        body.spin_accel = Vec3::new(6.0, 0.0, -2.0);
        body.linear_factor = 0.5;
        body.angular_factor = 0.25;
        body
    }

    #[test]
    fn taking_the_velocities_copies_the_velocity_and_the_spin_and_nothing_else() {
        let before = moving();
        let mut target = moving();
        target.shift = Vec3::new(9.0, 9.0, 9.0);
        let shifted = format!("{target:?}");
        let mut source = SolverBody::at_rest();
        source.velocity = Vec3::new(-1.0, -2.0, -3.0);
        source.spin = Vec3::new(7.0, 8.0, 9.0);
        source.shift = Vec3::new(1.0, 1.0, 1.0);
        source.inv_mass = 5.0;
        target.take_velocities(&source);
        assert_eq!(target.velocity, source.velocity);
        assert_eq!(target.spin, source.spin);
        // With the old velocities put back, the body is as it was: no other field moved.
        target.velocity = before.velocity;
        target.spin = before.spin;
        assert_eq!(format!("{target:?}"), shifted);
    }

    #[test]
    fn the_velocities_take_the_acceleration_for_h_seconds_and_then_the_damping() {
        let mut body = moving();
        body.integrate_velocity(0.1);
        // (v + a h) * factor on every axis.
        assert!(near(
            body.velocity,
            Vec3::new(0.5, 0.5 * (2.0 - 1.0), 0.5 * (3.0 + 0.4))
        ));
        assert!(near(
            body.spin,
            Vec3::new(0.25 * (0.5 + 0.6), -0.25, 0.25 * (2.0 - 0.2))
        ));
    }

    #[test]
    fn the_positions_take_the_velocity_for_h_seconds_and_turn_by_the_spin() {
        let mut body = SolverBody::at_rest();
        body.velocity = Vec3::new(1.0, -2.0, 3.0);
        body.spin = Vec3::new(0.0, 0.0, 2.0);
        body.shift = Vec3::new(1.0, 1.0, 1.0);
        body.integrate_position(0.5);
        assert!(near(body.shift, Vec3::new(1.5, 0.0, 2.5)));
        // A turn about +z carries x towards y.
        let turned = body.turn.rotate(Vec3::X);
        assert!(turned.y > 0.1 && turned.x < 1.0, "{turned:?}");
        body.integrate_position(0.5);
        assert!(near(body.shift, Vec3::new(2.0, -1.0, 4.0)));
    }

    #[test]
    fn a_point_moves_with_the_velocity_and_the_spin_cross_the_anchor() {
        let mut body = SolverBody::at_rest();
        body.velocity = Vec3::new(1.0, 0.0, 0.0);
        body.spin = Vec3::new(0.0, 0.0, 2.0);
        assert!(near(
            body.velocity_at(Vec3::new(1.0, 0.0, 0.0)),
            Vec3::new(1.0, 2.0, 0.0)
        ));
        assert!(near(
            body.velocity_at(Vec3::new(0.0, 1.0, 0.0)),
            Vec3::new(-1.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn an_impulse_changes_the_velocity_by_the_inverse_mass_and_the_spin_by_the_inertia() {
        let mut body = SolverBody::at_rest();
        body.inv_mass = 2.0;
        body.inv_inertia = Mat3::from_scale(Vec3::new(1.0, 3.0, 5.0));
        body.push(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 3.0, 0.0));
        assert!(near(body.velocity, Vec3::new(0.0, 6.0, 0.0)));
        // r x J = (1,0,0) x (0,3,0) = (0,0,3), scaled by the inverse inertia on z.
        assert!(near(body.spin, Vec3::new(0.0, 0.0, 15.0)));
    }

    #[test]
    fn only_a_body_with_no_inverse_mass_is_static() {
        let mut body = SolverBody::at_rest();
        assert!(body.is_static());
        body.inv_mass = 0.5;
        assert!(!body.is_static());
    }

    #[test]
    fn a_body_is_set_up_with_its_inertia_turned_into_world_axes()
    -> Result<(), crate::core::PhysicsError> {
        let mut world = World::new(WorldConfig {
            max_bodies: 2,
            gravity: Vec3::ZERO,
            max_pairs: 4,
            broadphase_margin: crate::d3::DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 4,
            max_joints: 0,
            speculative_margin: crate::d3::DEFAULT_SPECULATIVE_MARGIN,
            solver: crate::d3::SolverConfig::DEFAULT,
            sleep: crate::d3::SleepConfig::DISABLED,
        })?;
        let quarter =
            Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::ZERO,
            Shape3::Box {
                half_extents: Vec3::new(0.5, 1.0, 0.25),
            },
        );
        desc.rotation = quarter;
        desc.linear_velocity = Vec3::new(1.0, 2.0, 3.0);
        let handle = world.create_body(&desc)?;
        let body = world.body(handle)?;
        let solver_body = SolverBody::new(
            body,
            StepTerms {
                gravity: Vec3::new(0.0, -10.0, 0.0),
                h: 0.01,
                gyroscopic: false,
            },
        );
        let local = body.inverse_inertia();
        // A quarter turn about z swaps x and y: the world x axis has the local y inertia.
        assert!(near(
            solver_body.inv_inertia * Vec3::X,
            Vec3::new(local.y, 0.0, 0.0)
        ));
        assert!(near(
            solver_body.inv_inertia * Vec3::Y,
            Vec3::new(0.0, local.x, 0.0)
        ));
        assert!(near(
            solver_body.inv_inertia * Vec3::Z,
            Vec3::new(0.0, 0.0, local.z)
        ));
        assert!(near(solver_body.velocity, Vec3::new(1.0, 2.0, 3.0)));
        assert!((solver_body.inv_mass - body.inverse_mass()).abs() < 1.0e-9);
        assert!(near(solver_body.accel, Vec3::new(0.0, -10.0, 0.0)));
        assert!(!solver_body.is_static());
        Ok(())
    }
}
