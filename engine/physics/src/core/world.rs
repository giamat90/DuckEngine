//! The world: fixed-capacity body storage with generation-checked handles, the step and the state
//! hash. Dimension-generic; the instance supplies the types through [`Dim`].
//!
//! Capacity is fixed when the world is built. Handles are issued by a `base::pool::Pool` (index
//! order, last freed first, generation checked); the bodies live in a plain vector of the same
//! length, so iteration is in slot-index order and nothing allocates after [`World::new`].

use super::MAX_BODIES;
use super::body::{Body, BodyDesc, BodyHandle, BodyTag};
use super::dim::{Dim, Quantity};
use super::error::PhysicsError;
use super::hash::{HashBits, StateHasher};
use base::pool::Pool;

/// What a world is built from. The capacities are fixed for the life of the world.
pub struct WorldConfig<D: Dim> {
    /// The most bodies alive at once, at most [`MAX_BODIES`].
    pub max_bodies: u32,
    /// Acceleration of gravity in metres per second squared.
    pub gravity: D::Vec,
}

// Written by hand: `#[derive]` would add the bound `D: Clone` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for WorldConfig<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for WorldConfig<D> {}

/// A live body and the handle that names it.
struct Slot<D: Dim> {
    handle: BodyHandle<D>,
    body: Body<D>,
}

/// A rigid-body world.
pub struct World<D: Dim> {
    handles: Pool<BodyTag<D>>,
    slots: Vec<Option<Slot<D>>>,
    capacity: u32,
    gravity: D::Vec,
    steps: u64,
}

/// The live body `handle` names in `slots`, or `StaleHandle`.
fn live_slot<D: Dim>(
    slots: &[Option<Slot<D>>],
    handle: BodyHandle<D>,
) -> Result<&Slot<D>, PhysicsError> {
    slots
        .get(handle.index() as usize)
        .and_then(Option::as_ref)
        .filter(|slot| slot.handle == handle)
        .ok_or(PhysicsError::StaleHandle)
}

/// Mutable form of [`live_slot`].
fn live_slot_mut<D: Dim>(
    slots: &mut [Option<Slot<D>>],
    handle: BodyHandle<D>,
) -> Result<&mut Slot<D>, PhysicsError> {
    slots
        .get_mut(handle.index() as usize)
        .and_then(Option::as_mut)
        .filter(|slot| slot.handle == handle)
        .ok_or(PhysicsError::StaleHandle)
}

impl<D: Dim> World<D> {
    /// Builds a world and allocates all of its storage. This is the only call that allocates.
    ///
    /// # Errors
    /// [`PhysicsError::CapacityTooLarge`] when `max_bodies` is above [`MAX_BODIES`];
    /// [`PhysicsError::NotFinite`] when the gravity is not finite.
    pub fn new(config: WorldConfig<D>) -> Result<Self, PhysicsError> {
        if config.max_bodies > MAX_BODIES {
            return Err(PhysicsError::CapacityTooLarge { max: MAX_BODIES });
        }
        if !config.gravity.is_finite() {
            return Err(PhysicsError::NotFinite);
        }
        let mut slots = Vec::with_capacity(config.max_bodies as usize);
        slots.resize_with(config.max_bodies as usize, || None);
        Ok(Self {
            handles: Pool::with_capacity(config.max_bodies),
            slots,
            capacity: config.max_bodies,
            gravity: config.gravity,
            steps: 0,
        })
    }

    /// The most bodies the world can hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.handles.capacity()
    }

    /// The number of live bodies.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// True when no body is alive.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// The acceleration of gravity.
    #[must_use]
    pub fn gravity(&self) -> D::Vec {
        self.gravity
    }

    /// The number of steps taken so far.
    #[must_use]
    pub fn step_count(&self) -> u64 {
        self.steps
    }

    /// True when `handle` names a live body of this world.
    #[must_use]
    pub fn contains(&self, handle: BodyHandle<D>) -> bool {
        live_slot(&self.slots, handle).is_ok()
    }

    /// Creates a body. Allocates nothing.
    ///
    /// # Errors
    /// The errors of the description ([`PhysicsError::NotFinite`], [`PhysicsError::InvalidShape`]),
    /// or [`PhysicsError::CapacityExhausted`] when every slot is in use. A refused call changes
    /// nothing: no slot is used up.
    pub fn create_body(&mut self, desc: &BodyDesc<D>) -> Result<BodyHandle<D>, PhysicsError> {
        let body = Body::from_desc(desc)?;
        let handle = self
            .handles
            .insert(BodyTag::new())
            .map_err(|_| PhysicsError::CapacityExhausted)?;
        if let Some(slot) = self.slots.get_mut(handle.index() as usize) {
            *slot = Some(Slot { handle, body });
            return Ok(handle);
        }
        // Not reachable: the pool and the slot vector have the same length.
        self.handles.remove(handle);
        Err(PhysicsError::CapacityExhausted)
    }

    /// Destroys a body; its slot is reused by a later body with a higher generation, unless the
    /// generation cannot advance any more: such a slot is retired for good and the number of bodies
    /// that can exist at once then falls below `capacity`.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn destroy_body(&mut self, handle: BodyHandle<D>) -> Result<(), PhysicsError> {
        live_slot(&self.slots, handle)?;
        self.handles.remove(handle);
        if let Some(slot) = self.slots.get_mut(handle.index() as usize) {
            *slot = None;
        }
        Ok(())
    }

    /// The state of a body.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn body(&self, handle: BodyHandle<D>) -> Result<&Body<D>, PhysicsError> {
        live_slot(&self.slots, handle).map(|slot| &slot.body)
    }

    /// Every live body with its handle, in slot-index order.
    pub fn iter(&self) -> impl Iterator<Item = (BodyHandle<D>, &Body<D>)> {
        self.slots
            .iter()
            .flatten()
            .map(|slot| (slot.handle, &slot.body))
    }

    /// Sets the linear velocity of a dynamic body.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn set_linear_velocity(
        &mut self,
        handle: BodyHandle<D>,
        velocity: D::Vec,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .set_linear_velocity(velocity)
    }

    /// Sets the angular velocity of a dynamic body.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn set_angular_velocity(
        &mut self,
        handle: BodyHandle<D>,
        velocity: D::Ang,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .set_angular_velocity(velocity)
    }

    /// Adds a force (newtons) acting through the centre of mass during the next step only.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn apply_force(
        &mut self,
        handle: BodyHandle<D>,
        force: D::Vec,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .apply_force(force)
    }

    /// Adds a torque (newton metres) acting during the next step only.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn apply_torque(
        &mut self,
        handle: BodyHandle<D>,
        torque: D::Ang,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .apply_torque(torque)
    }

    /// Advances every dynamic body by `dt` seconds. The caller passes the same `dt` every step (the
    /// fixed step of the application); allocates nothing.
    ///
    /// # Errors
    /// [`PhysicsError::InvalidTimeStep`] when `dt` is not finite and above zero;
    /// [`PhysicsError::NotFinite`] when the step would make a position, rotation or velocity
    /// overflow. In both cases nothing changes (the step is computed once on copies to check it,
    /// then again to commit, so a step costs two integrations).
    pub fn step(&mut self, dt: f32) -> Result<(), PhysicsError> {
        if !dt.is_finite() || dt <= 0.0 {
            return Err(PhysicsError::InvalidTimeStep);
        }
        let gravity = self.gravity;
        let stays_finite = self.slots.iter().flatten().all(|slot| {
            let mut next = slot.body;
            next.integrate(gravity, dt);
            next.motion_is_finite()
        });
        if !stays_finite {
            return Err(PhysicsError::NotFinite);
        }
        for slot in self.slots.iter_mut().flatten() {
            slot.body.integrate(gravity, dt);
        }
        self.steps += 1;
        Ok(())
    }

    /// A hash of the simulation state: capacity, gravity, then every live body (handle, then all
    /// state) in slot-index order, floats by bit pattern. Equal inputs give equal hashes on every
    /// run; any change of a live body's state changes it. It does not cover the order in which freed
    /// slots will be reused, so two worlds that reached the same live bodies by different destroy
    /// orders hash equal and may hand out different handles on the next create.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u32(self.capacity);
        self.gravity.write_bits(&mut hasher);
        for slot in self.slots.iter().flatten() {
            hasher.write_u32(slot.handle.index());
            hasher.write_u32(slot.handle.generation());
            slot.body.write_bits(&mut hasher);
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BodyKind, Dim};
    use crate::d2::{D2, Rot2, Shape2};
    use base::math::Vec2;
    use base::pool::Handle;

    type World2 = World<D2>;
    type Desc2 = BodyDesc<D2>;

    const DT: f32 = 1.0 / 60.0;
    const G: f32 = -9.8;

    fn world(max_bodies: u32, gravity: Vec2) -> World2 {
        match World::new(WorldConfig {
            max_bodies,
            gravity,
        }) {
            Ok(world) => world,
            Err(error) => panic!("world must build: {error}"),
        }
    }

    fn circle(radius: f32) -> Shape2 {
        Shape2::Circle { radius }
    }

    fn dynamic(x: f32, y: f32) -> Desc2 {
        Desc2::new(BodyKind::Dynamic, Vec2::new(x, y), circle(0.5))
    }

    fn create(world: &mut World2, desc: &Desc2) -> BodyHandle<D2> {
        match world.create_body(desc) {
            Ok(handle) => handle,
            Err(error) => panic!("body must be created: {error}"),
        }
    }

    fn state(world: &World2, handle: BodyHandle<D2>) -> Body<D2> {
        match world.body(handle) {
            Ok(body) => *body,
            Err(error) => panic!("body must exist: {error}"),
        }
    }

    fn step_n(world: &mut World2, steps: u32) {
        for _ in 0..steps {
            assert_eq!(world.step(DT), Ok(()));
        }
    }

    fn stale<T>(result: Result<T, PhysicsError>) -> bool {
        result.err() == Some(PhysicsError::StaleHandle)
    }

    /// Bit-for-bit equality of two floats (the tests compare exact results on purpose).
    fn same(a: f32, b: f32) -> bool {
        a.to_bits() == b.to_bits()
    }

    // ---- creation and capacity -------------------------------------------------------------

    #[test]
    fn a_world_is_built_with_its_capacity_and_is_empty() {
        let mut world = world(8, Vec2::new(0.0, G));
        assert_eq!(
            (world.capacity(), world.len(), world.is_empty()),
            (8, 0, true)
        );
        create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!((world.len(), world.is_empty()), (1, false));
        let world = self::world(8, Vec2::new(0.0, G));
        assert_eq!(world.gravity(), Vec2::new(0.0, G));
        assert_eq!(world.step_count(), 0);
    }

    #[test]
    fn a_capacity_above_the_maximum_is_refused() {
        let config = WorldConfig::<D2> {
            max_bodies: MAX_BODIES + 1,
            gravity: Vec2::ZERO,
        };
        assert!(matches!(
            World::new(config).err(),
            Some(PhysicsError::CapacityTooLarge { max }) if max == MAX_BODIES
        ));
    }

    #[test]
    fn a_gravity_that_is_not_finite_is_refused() {
        for gravity in [Vec2::new(f32::NAN, 0.0), Vec2::new(0.0, f32::INFINITY)] {
            let config = WorldConfig::<D2> {
                max_bodies: 1,
                gravity,
            };
            assert_eq!(World::new(config).err(), Some(PhysicsError::NotFinite));
        }
    }

    #[test]
    fn a_world_of_zero_capacity_refuses_every_body() {
        let mut world = world(0, Vec2::ZERO);
        assert_eq!(
            world.create_body(&dynamic(0.0, 0.0)).err(),
            Some(PhysicsError::CapacityExhausted)
        );
    }

    #[test]
    fn handles_are_issued_in_slot_order_with_generation_zero() {
        assert_eq!(std::mem::size_of::<BodyHandle<D2>>(), 8);
        let mut world = world(3, Vec2::ZERO);
        for expected in 0..3 {
            let handle = create(&mut world, &dynamic(0.0, 0.0));
            assert_eq!((handle.index(), handle.generation()), (expected, 0));
        }
        assert_eq!(world.len(), 3);
    }

    #[test]
    fn creating_past_the_capacity_is_an_error_value_and_changes_nothing() {
        let mut world = world(2, Vec2::ZERO);
        let first = create(&mut world, &dynamic(1.0, 0.0));
        create(&mut world, &dynamic(2.0, 0.0));
        let before = world.state_hash();
        assert_eq!(
            world.create_body(&dynamic(3.0, 0.0)).err(),
            Some(PhysicsError::CapacityExhausted)
        );
        assert_eq!((world.len(), world.state_hash()), (2, before));
        assert!(world.contains(first));
    }

    #[test]
    fn a_refused_description_uses_up_no_slot() {
        let mut world = world(1, Vec2::ZERO);
        let bad = Desc2::new(BodyKind::Dynamic, Vec2::ZERO, circle(-1.0));
        assert_eq!(
            world.create_body(&bad).err(),
            Some(PhysicsError::InvalidShape)
        );
        assert_eq!(world.len(), 0);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!((handle.index(), handle.generation()), (0, 0));
    }

    #[test]
    fn a_description_with_a_value_that_is_not_finite_is_refused() {
        let mut world = world(1, Vec2::ZERO);
        let nan = f32::NAN;
        let mut cases = [dynamic(0.0, 0.0); 6];
        cases[0].position = Vec2::new(nan, 0.0);
        cases[1].linear_velocity = Vec2::new(0.0, f32::INFINITY);
        cases[2].angular_velocity = nan;
        cases[3].rotation = Rot2::from_angle(nan);
        cases[4].linear_damping = nan;
        cases[5].angular_damping = f32::INFINITY;
        for desc in &cases {
            assert_eq!(world.create_body(desc).err(), Some(PhysicsError::NotFinite));
        }
        assert_eq!(world.len(), 0);
    }

    #[test]
    fn negative_damping_and_a_density_of_zero_are_refused() {
        let mut world = world(1, Vec2::ZERO);
        let mut damped = dynamic(0.0, 0.0);
        damped.linear_damping = -0.1;
        let mut spinning = dynamic(0.0, 0.0);
        spinning.angular_damping = -0.1;
        let mut weightless = dynamic(0.0, 0.0);
        weightless.density = 0.0;
        for desc in [damped, spinning, weightless] {
            assert_eq!(
                world.create_body(&desc).err(),
                Some(PhysicsError::InvalidShape)
            );
        }
    }

    #[test]
    fn a_shape_whose_inverse_mass_overflows_is_refused() {
        // A large box of almost no density: the mass is a denormal (inverse mass infinite) while the
        // inertia is large enough for a finite inverse inertia, so only the mass check can refuse it.
        let mut world = world(1, Vec2::ZERO);
        let mut thin = Desc2::new(
            BodyKind::Dynamic,
            Vec2::ZERO,
            Shape2::Box {
                half_extents: Vec2::new(100.0, 100.0),
            },
        );
        thin.density = 5e-44;
        assert_eq!(
            world.create_body(&thin).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    #[test]
    fn a_shape_whose_mass_and_inertia_underflow_is_refused() {
        // Mass 1e-39 is a denormal: valid, but its inverse is infinite.
        let mut world = world(1, Vec2::ZERO);
        let mut tiny = dynamic(0.0, 0.0);
        tiny.shape = circle(1e-2);
        tiny.density = 1e-36;
        assert_eq!(
            world.create_body(&tiny).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    #[test]
    fn the_body_reports_what_it_was_created_from() {
        let mut world = world(2, Vec2::ZERO);
        let mut desc = dynamic(1.0, 2.0);
        desc.rotation = Rot2::from_angle(0.5);
        desc.linear_velocity = Vec2::new(3.0, 4.0);
        desc.angular_velocity = 0.25;
        desc.density = 2.0;
        let handle = create(&mut world, &desc);
        let body = state(&world, handle);
        assert_eq!(body.kind(), BodyKind::Dynamic);
        assert_eq!(body.position(), Vec2::new(1.0, 2.0));
        assert_eq!(body.rotation(), Rot2::from_angle(0.5));
        assert_eq!(body.linear_velocity(), Vec2::new(3.0, 4.0));
        assert!(same(body.angular_velocity(), 0.25));
        assert_eq!(body.shape(), circle(0.5));
        let mass = 2.0 * std::f32::consts::PI * 0.25;
        assert!((body.mass() - mass).abs() < 1e-6);
        assert!((body.inverse_mass() * mass - 1.0).abs() < 1e-6);
        let inertia = 0.5 * mass * 0.25;
        assert!((body.inverse_inertia() * inertia - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_static_body_has_infinite_mass_and_ignores_its_velocity() {
        let mut world = world(1, Vec2::new(0.0, G));
        let mut desc = Desc2::new(BodyKind::Static, Vec2::new(1.0, 1.0), circle(1.0));
        desc.linear_velocity = Vec2::new(5.0, 5.0);
        desc.angular_velocity = 2.0;
        let handle = create(&mut world, &desc);
        let body = state(&world, handle);
        assert_eq!(body.kind(), BodyKind::Static);
        assert_eq!(
            (body.mass(), body.inverse_mass(), body.inverse_inertia()),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            (body.linear_velocity(), body.angular_velocity()),
            (Vec2::ZERO, 0.0)
        );
    }

    #[test]
    fn a_static_body_with_an_invalid_shape_is_refused() {
        let mut world = world(1, Vec2::ZERO);
        let bad = Desc2::new(BodyKind::Static, Vec2::ZERO, circle(0.0));
        assert_eq!(
            world.create_body(&bad).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    fn cloned<T: Clone>(value: &T) -> T {
        value.clone()
    }

    #[test]
    fn descriptions_configs_and_bodies_are_plain_copyable_values() {
        let desc = dynamic(1.0, 2.0);
        let config = WorldConfig::<D2> {
            max_bodies: 3,
            gravity: Vec2::new(0.0, G),
        };
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &cloned(&desc));
        let body = state(&world, handle);
        let copy = cloned(&body);
        assert_eq!(copy.position(), body.position());
        assert_eq!(cloned(&config).max_bodies, 3);
        let Ok(properties) = D2::mass_properties(&circle(1.0), 1.0) else {
            panic!("a unit circle has mass properties");
        };
        assert!(same(cloned(&properties).mass, properties.mass));
    }

    // ---- destruction, stale handles, slot reuse --------------------------------------------

    #[test]
    fn a_destroyed_body_is_gone_and_its_handle_is_stale() {
        let mut world = world(2, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!(world.destroy_body(handle), Ok(()));
        assert_eq!((world.len(), world.contains(handle)), (0, false));
        assert!(stale(world.body(handle)));
        assert!(stale(world.destroy_body(handle)));
        assert!(stale(world.set_linear_velocity(handle, Vec2::ZERO)));
        assert!(stale(world.set_angular_velocity(handle, 0.0)));
        assert!(stale(world.apply_force(handle, Vec2::ZERO)));
        assert!(stale(world.apply_torque(handle, 0.0)));
    }

    #[test]
    fn a_reused_slot_gets_the_next_generation_and_the_old_handle_stays_stale() {
        let mut world = world(1, Vec2::ZERO);
        let old = create(&mut world, &dynamic(1.0, 0.0));
        assert_eq!(world.destroy_body(old), Ok(()));
        let new = create(&mut world, &dynamic(2.0, 0.0));
        assert_eq!((new.index(), new.generation()), (0, 1));
        assert!(stale(world.body(old)));
        assert_eq!(state(&world, new).position(), Vec2::new(2.0, 0.0));
        assert!(!world.contains(old));
        assert!(world.contains(new));
    }

    #[test]
    fn a_forged_handle_is_stale_not_a_panic() {
        let mut world = world(2, Vec2::ZERO);
        let live = create(&mut world, &dynamic(0.0, 0.0));
        let far = Handle::from_raw(1_000_000, 0);
        let wrong_generation = Handle::from_raw(live.index(), live.generation() + 1);
        let never_used = Handle::from_raw(1, 0);
        for handle in [far, wrong_generation, never_used] {
            assert!(stale(world.body(handle)));
            assert!(stale(world.destroy_body(handle)));
        }
        assert!(world.contains(live));
    }

    #[test]
    fn the_free_slot_destroyed_last_is_reused_first() {
        let mut world = world(3, Vec2::ZERO);
        let handles: Vec<_> = (0..3_u8)
            .map(|i| create(&mut world, &dynamic(f32::from(i), 0.0)))
            .collect();
        assert_eq!(world.destroy_body(handles[0]), Ok(()));
        assert_eq!(world.destroy_body(handles[2]), Ok(()));
        assert_eq!(create(&mut world, &dynamic(9.0, 0.0)).index(), 2);
        assert_eq!(create(&mut world, &dynamic(9.0, 0.0)).index(), 0);
    }

    #[test]
    fn iteration_is_in_slot_order_and_skips_destroyed_bodies() {
        let mut world = world(4, Vec2::ZERO);
        let handles: Vec<_> = (0..4_u8)
            .map(|i| create(&mut world, &dynamic(f32::from(i), 0.0)))
            .collect();
        assert_eq!(world.destroy_body(handles[1]), Ok(()));
        let seen: Vec<(u32, f32)> = world
            .iter()
            .map(|(handle, body)| (handle.index(), body.position().x))
            .collect();
        assert_eq!(seen, vec![(0, 0.0), (2, 2.0), (3, 3.0)]);
    }

    // ---- motion setters ---------------------------------------------------------------------

    #[test]
    fn velocities_can_be_set_on_a_dynamic_body() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!(
            world.set_linear_velocity(handle, Vec2::new(1.0, 2.0)),
            Ok(())
        );
        assert_eq!(world.set_angular_velocity(handle, 3.0), Ok(()));
        let body = state(&world, handle);
        assert_eq!(
            (body.linear_velocity(), body.angular_velocity()),
            (Vec2::new(1.0, 2.0), 3.0)
        );
    }

    #[test]
    fn a_static_body_takes_no_motion() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(
            &mut world,
            &Desc2::new(BodyKind::Static, Vec2::ZERO, circle(1.0)),
        );
        let before = world.state_hash();
        let one = Vec2::new(1.0, 1.0);
        assert_eq!(
            world.set_linear_velocity(handle, one),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(
            world.set_angular_velocity(handle, 1.0),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(
            world.apply_force(handle, one),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(
            world.apply_torque(handle, 1.0),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(world.state_hash(), before);
    }

    #[test]
    fn values_that_are_not_finite_are_refused_and_change_nothing() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        let before = world.state_hash();
        let bad = Vec2::new(f32::NAN, 0.0);
        assert_eq!(
            world.set_linear_velocity(handle, bad),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(
            world.set_angular_velocity(handle, f32::NAN),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.apply_force(handle, bad), Err(PhysicsError::NotFinite));
        assert_eq!(
            world.apply_torque(handle, f32::INFINITY),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.state_hash(), before);
    }

    // ---- the step ---------------------------------------------------------------------------

    #[test]
    fn a_time_step_that_is_not_finite_and_above_zero_is_refused_and_changes_nothing() {
        let mut world = world(1, Vec2::new(0.0, G));
        create(&mut world, &dynamic(0.0, 0.0));
        let before = world.state_hash();
        for dt in [0.0, -DT, f32::NAN, f32::INFINITY] {
            assert_eq!(
                world.step(dt),
                Err(PhysicsError::InvalidTimeStep),
                "dt {dt}"
            );
        }
        assert_eq!((world.state_hash(), world.step_count()), (before, 0));
    }

    #[test]
    fn free_fall_follows_the_semi_implicit_euler_closed_form() {
        // v_n = n g dt, y_n = y_0 + g dt^2 n (n + 1) / 2.
        let mut world = world(1, Vec2::new(0.0, G));
        let handle = create(&mut world, &dynamic(0.0, 100.0));
        let steps = 120_u16;
        step_n(&mut world, u32::from(steps));
        let n = f32::from(steps);
        let body = state(&world, handle);
        let velocity = n * G * DT;
        let height = 100.0 + G * DT * DT * n * (n + 1.0) / 2.0;
        assert!((body.linear_velocity().y - velocity).abs() < 1e-3 * velocity.abs());
        assert!((body.position().y - height).abs() < 1e-4 * 100.0);
        assert!(same(body.linear_velocity().x, 0.0));
        assert!(same(body.position().x, 0.0));
        assert_eq!(world.step_count(), 120);
    }

    #[test]
    fn free_fall_stays_within_the_documented_error_of_the_continuous_solution() {
        // The scheme differs from y_0 + g t^2 / 2 by g t dt / 2 (first order in dt).
        for steps in [1_u16, 10, 60, 600] {
            let mut world = world(1, Vec2::new(0.0, G));
            let handle = create(&mut world, &dynamic(0.0, 0.0));
            step_n(&mut world, u32::from(steps));
            let time = f32::from(steps) * DT;
            let exact = 0.5 * G * time * time;
            let bound = 0.5 * G.abs() * time * DT;
            let error = (state(&world, handle).position().y - exact).abs();
            assert!(
                error <= bound * 1.01 + 1e-3,
                "t {time}: error {error} bound {bound}"
            );
        }
    }

    #[test]
    fn gravity_does_not_depend_on_mass() {
        let mut world = world(2, Vec2::new(0.0, G));
        let light = create(&mut world, &dynamic(0.0, 0.0));
        let mut heavy_desc = dynamic(1.0, 0.0);
        heavy_desc.density = 1000.0;
        let heavy = create(&mut world, &heavy_desc);
        step_n(&mut world, 30);
        assert_eq!(
            state(&world, light).position().y.to_bits(),
            state(&world, heavy).position().y.to_bits()
        );
    }

    #[test]
    fn a_static_body_does_not_fall() {
        let mut world = world(1, Vec2::new(0.0, G));
        let handle = create(
            &mut world,
            &Desc2::new(BodyKind::Static, Vec2::new(1.0, 2.0), circle(1.0)),
        );
        let before = world.state_hash();
        step_n(&mut world, 10);
        assert_eq!(state(&world, handle).position(), Vec2::new(1.0, 2.0));
        assert_eq!(world.state_hash(), before);
    }

    #[test]
    fn momentum_is_conserved_without_forces() {
        let mut world = world(2, Vec2::ZERO);
        let mut a = dynamic(0.0, 0.0);
        a.linear_velocity = Vec2::new(3.0, -1.0);
        a.density = 2.0;
        let mut b = dynamic(5.0, 0.0);
        b.linear_velocity = Vec2::new(-1.0, 0.5);
        let (ha, hb) = (create(&mut world, &a), create(&mut world, &b));
        let momentum = |world: &World2| {
            let (pa, pb) = (state(world, ha), state(world, hb));
            pa.linear_velocity() * pa.mass() + pb.linear_velocity() * pb.mass()
        };
        let before = momentum(&world);
        step_n(&mut world, 1000);
        let after = momentum(&world);
        assert_eq!(
            (before.x.to_bits(), before.y.to_bits()),
            (after.x.to_bits(), after.y.to_bits())
        );
        let body = state(&world, ha);
        assert_eq!(body.linear_velocity(), Vec2::new(3.0, -1.0));
        let expected = 3.0 * DT * 1000.0;
        assert!(
            (body.position().x - expected).abs() < 1e-2,
            "x {}",
            body.position().x
        );
    }

    #[test]
    fn a_force_acts_for_one_step_only_and_changes_momentum_by_force_times_time() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        let mass = state(&world, handle).mass();
        assert_eq!(world.apply_force(handle, Vec2::new(10.0, 0.0)), Ok(()));
        assert_eq!(world.apply_force(handle, Vec2::new(2.0, 0.0)), Ok(()));
        step_n(&mut world, 1);
        let velocity = state(&world, handle).linear_velocity().x;
        assert!(
            (velocity * mass - 12.0 * DT).abs() < 1e-5,
            "p {}",
            velocity * mass
        );
        step_n(&mut world, 5);
        assert!(same(state(&world, handle).linear_velocity().x, velocity));
    }

    #[test]
    fn a_torque_acts_for_one_step_only_and_spins_by_torque_over_inertia() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        let inv_inertia = state(&world, handle).inverse_inertia();
        assert_eq!(world.apply_torque(handle, 4.0), Ok(()));
        step_n(&mut world, 1);
        let spin = state(&world, handle).angular_velocity();
        assert!((spin - inv_inertia * 4.0 * DT).abs() < 1e-6);
        step_n(&mut world, 5);
        assert!(same(state(&world, handle).angular_velocity(), spin));
    }

    #[test]
    fn damping_scales_the_velocity_by_one_over_one_plus_dt_times_damping() {
        let mut world = world(1, Vec2::ZERO);
        let mut desc = dynamic(0.0, 0.0);
        desc.linear_velocity = Vec2::new(10.0, 0.0);
        desc.angular_velocity = 4.0;
        desc.linear_damping = 2.0;
        desc.angular_damping = 5.0;
        let handle = create(&mut world, &desc);
        step_n(&mut world, 1);
        let body = state(&world, handle);
        let linear = 10.0 / (1.0 + DT * 2.0);
        let angular = 4.0 / (1.0 + DT * 5.0);
        assert!((body.linear_velocity().x - linear).abs() < 1e-5);
        assert!((body.angular_velocity() - angular).abs() < 1e-5);
    }

    #[test]
    fn a_spinning_body_keeps_its_rotation_pair_normalised_and_turns_at_the_expected_rate() {
        let mut world = world(1, Vec2::ZERO);
        let mut desc = dynamic(0.0, 0.0);
        desc.angular_velocity = 3.0;
        let handle = create(&mut world, &desc);
        step_n(&mut world, 10_000);
        let (rot_cos, rot_sin) = state(&world, handle).rotation().direction();
        let length = rot_cos * rot_cos + rot_sin * rot_sin;
        assert!((length - 1.0).abs() < 1e-6, "length^2 {length}");
        // Each step multiplies by (1 + i w dt) and normalises; the same in f64, with no trig.
        let delta = f64::from(3.0_f32 * DT);
        let (mut cos, mut sin) = (1.0_f64, 0.0_f64);
        for _ in 0..10_000 {
            let (c, s) = (cos - sin * delta, sin + cos * delta);
            let length = (c * c + s * s).sqrt();
            (cos, sin) = (c / length, s / length);
        }
        assert!((f64::from(rot_cos) - cos).abs() < 1e-3);
        assert!((f64::from(rot_sin) - sin).abs() < 1e-3);
    }

    #[test]
    fn a_step_that_would_overflow_is_refused_and_changes_nothing() {
        let mut world = world(2, Vec2::ZERO);
        let calm = create(&mut world, &dynamic(0.0, 0.0));
        let mut fast = dynamic(1.0, 0.0);
        fast.angular_velocity = 1e24;
        let fast = create(&mut world, &fast);
        let before = world.state_hash();
        assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
        assert_eq!(world.state_hash(), before);
        assert_eq!(world.step_count(), 0);
        assert!(same(state(&world, calm).position().y, 0.0));
        assert!(world.set_angular_velocity(fast, 1.0).is_ok());
        assert_eq!(world.step(DT), Ok(()));
        let mut huge = dynamic(f32::MAX, 0.0);
        huge.linear_velocity = Vec2::new(f32::MAX, 0.0);
        let mut world = self::world(1, Vec2::ZERO);
        create(&mut world, &huge);
        assert_eq!(world.step(1.0), Err(PhysicsError::NotFinite));
    }

    #[test]
    fn forces_and_torques_whose_sum_overflows_are_refused() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!(world.apply_force(handle, Vec2::new(f32::MAX, 0.0)), Ok(()));
        let before = world.state_hash();
        assert_eq!(
            world.apply_force(handle, Vec2::new(f32::MAX, 0.0)),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.state_hash(), before);
        assert_eq!(world.apply_torque(handle, f32::MAX), Ok(()));
        let before = world.state_hash();
        assert_eq!(
            world.apply_torque(handle, f32::MAX),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.state_hash(), before);
    }

    // ---- the state hash ---------------------------------------------------------------------

    /// A small scene built the same way every time.
    fn scene() -> World2 {
        let mut world = world(8, Vec2::new(0.0, G));
        create(
            &mut world,
            &Desc2::new(
                BodyKind::Static,
                Vec2::ZERO,
                Shape2::Box {
                    half_extents: Vec2::new(10.0, 0.5),
                },
            ),
        );
        for i in 0..5_u8 {
            let mut desc = dynamic(f32::from(i), 3.0 + f32::from(i));
            desc.angular_velocity = 0.5 * f32::from(i);
            desc.linear_velocity = Vec2::new(1.0, 0.0);
            create(&mut world, &desc);
        }
        world
    }

    #[test]
    fn the_same_inputs_give_the_same_hash_after_every_step() {
        let (mut first, mut second) = (scene(), scene());
        assert_eq!(first.state_hash(), second.state_hash());
        for _ in 0..200 {
            assert_eq!(first.step(DT), Ok(()));
            assert_eq!(second.step(DT), Ok(()));
            assert_eq!(first.state_hash(), second.state_hash());
        }
    }

    #[test]
    fn stepping_changes_the_hash_and_the_hash_is_stable_between_calls() {
        let mut world = scene();
        let before = world.state_hash();
        assert_eq!(before, world.state_hash());
        step_n(&mut world, 1);
        assert_ne!(world.state_hash(), before);
    }

    #[test]
    fn changing_one_field_of_one_body_changes_the_hash() {
        let baseline = {
            let mut world = world(2, Vec2::ZERO);
            create(&mut world, &dynamic(0.0, 0.0));
            world.state_hash()
        };
        let mut variants: Vec<Desc2> = Vec::new();
        let mut push = |change: &dyn Fn(&mut Desc2)| {
            let mut desc = dynamic(0.0, 0.0);
            change(&mut desc);
            variants.push(desc);
        };
        push(&|d| d.kind = BodyKind::Static);
        push(&|d| d.position = Vec2::new(f32::from_bits(0.0_f32.to_bits() | 1), 0.0));
        push(&|d| d.position = Vec2::new(0.0, 1e-30));
        push(&|d| d.rotation = Rot2::from_angle(1e-3));
        push(&|d| d.linear_velocity = Vec2::new(1e-30, 0.0));
        push(&|d| d.linear_velocity = Vec2::new(0.0, 1e-30));
        push(&|d| d.angular_velocity = 1e-30);
        push(&|d| d.shape = circle(0.25));
        push(&|d| {
            d.shape = Shape2::Box {
                half_extents: Vec2::new(0.5, 0.5),
            }
        });
        push(&|d| d.density = 2.0);
        push(&|d| d.linear_damping = 0.1);
        push(&|d| d.angular_damping = 0.1);
        for (i, desc) in variants.iter().enumerate() {
            let mut world = world(2, Vec2::ZERO);
            create(&mut world, desc);
            assert_ne!(world.state_hash(), baseline, "variant {i}");
        }
    }

    #[test]
    fn pending_force_and_torque_and_the_world_settings_are_part_of_the_hash() {
        let build = |capacity: u32, gravity: Vec2| {
            let mut world = world(capacity, gravity);
            let handle = create(&mut world, &dynamic(0.0, 0.0));
            (world, handle)
        };
        let (base_world, _) = build(2, Vec2::ZERO);
        let baseline = base_world.state_hash();
        let (mut forced, handle) = build(2, Vec2::ZERO);
        assert_eq!(forced.apply_force(handle, Vec2::new(0.0, 1e-30)), Ok(()));
        assert_ne!(forced.state_hash(), baseline);
        let (mut torqued, handle) = build(2, Vec2::ZERO);
        assert_eq!(torqued.apply_torque(handle, 1e-30), Ok(()));
        assert_ne!(torqued.state_hash(), baseline);
        assert_ne!(build(3, Vec2::ZERO).0.state_hash(), baseline);
        assert_ne!(build(2, Vec2::new(0.0, 1e-30)).0.state_hash(), baseline);
    }

    #[test]
    fn the_handle_and_the_set_of_live_bodies_are_part_of_the_hash() {
        let mut world = world(2, Vec2::ZERO);
        let first = create(&mut world, &dynamic(0.0, 0.0));
        let with_one = world.state_hash();
        let second = create(&mut world, &dynamic(1.0, 0.0));
        let with_two = world.state_hash();
        assert_ne!(with_one, with_two);
        assert_eq!(world.destroy_body(second), Ok(()));
        assert_eq!(world.state_hash(), with_one);
        // The same body in the same slot with a newer generation is a different state.
        assert_eq!(world.destroy_body(first), Ok(()));
        create(&mut world, &dynamic(0.0, 0.0));
        assert_ne!(world.state_hash(), with_one);
    }

    #[test]
    fn two_bodies_swapped_in_slot_order_hash_differently() {
        let build = |a: f32, b: f32| {
            let mut world = world(2, Vec2::ZERO);
            create(&mut world, &dynamic(a, 0.0));
            create(&mut world, &dynamic(b, 0.0));
            world.state_hash()
        };
        assert_ne!(build(1.0, 2.0), build(2.0, 1.0));
    }
}
