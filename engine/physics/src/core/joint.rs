//! Joints (card 4-16): handles, the description a joint is created from, and the fixed-capacity
//! table the world keeps them in. What a joint *is* (a revolute hinge, a distance rod or spring) is
//! dimension specific and lives in the instance behind [`JointShape`]; this module only knows that a
//! joint ties two bodies and carries a shape.
//!
//! Joints are generation-checked like bodies. Destroying a body destroys the joints that touch it
//! (their handles go stale), as `Box2D` v3 does **[K]**. A joint is part of the island of its two
//! bodies, and by default the two bodies do not collide with each other (`Box2D`'s
//! `collideConnected = false` **[K]**; [`JointDesc::collide_connected`]).
//!
//! The table keeps a sorted list of the body pairs that must not make a manifold, so the
//! narrowphase asks "do these two bodies share a joint that forbids contact" with one binary search.
//! Nothing here allocates after [`JointTable::new`].

use super::body::BodyHandle;
use super::dim::Dim;
use super::error::PhysicsError;
use base::pool::{Handle, Pool};
use std::fmt::Debug;
use std::marker::PhantomData;

/// The most joints one world may be built for (see [`PhysicsError::JointCapacityTooLarge`]).
pub const MAX_JOINTS: u32 = 1 << 20;

/// The type a [`JointHandle`] is a handle to; it carries the dimension like `BodyTag`.
pub struct JointTag<D>(PhantomData<fn() -> D>);

impl<D> JointTag<D> {
    const fn new() -> Self {
        Self(PhantomData)
    }
}

/// Names one joint of a world: slot index and generation, eight bytes. The handle of a destroyed
/// joint (also of a joint destroyed with its body) is detected as stale.
pub type JointHandle<D> = Handle<JointTag<D>>;

/// What a dimension says about a kind of joint: it can be copied, compared and checked.
pub trait JointShape: Copy + Debug + PartialEq {
    /// Checks the numbers of the description.
    ///
    /// # Errors
    /// [`PhysicsError::NotFinite`] for a number that is not finite, [`PhysicsError::InvalidJoint`]
    /// for a value outside its range.
    fn validate(&self) -> Result<(), PhysicsError>;
}

/// A joint kind for a dimension that has no joints (the test instance of the generic core).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoJoint {}

impl JointShape for NoJoint {
    fn validate(&self) -> Result<(), PhysicsError> {
        match *self {}
    }
}

/// The description a joint is created from.
pub struct JointDesc<D: Dim> {
    /// The first body. May be static (a ground anchor).
    pub first: BodyHandle<D>,
    /// The second body. May be static, but not both.
    pub second: BodyHandle<D>,
    /// True when the two bodies may still collide with each other. False (the default of
    /// [`JointDesc::new`]) keeps them from making a manifold, which is what a hinge or a rod needs:
    /// the shapes around the joint overlap by design.
    pub collide_connected: bool,
    /// The kind of joint and its numbers.
    pub shape: D::Joint,
}

// Written by hand: `#[derive]` would add the bound `D: Clone` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for JointDesc<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for JointDesc<D> {}

impl<D: Dim> JointDesc<D> {
    /// A joint of `shape` between two bodies that do not collide with each other.
    #[must_use]
    pub fn new(first: BodyHandle<D>, second: BodyHandle<D>, shape: D::Joint) -> Self {
        Self {
            first,
            second,
            collide_connected: false,
            shape,
        }
    }
}

/// A live joint as the solver reads it.
pub struct JointEntry<D: Dim> {
    /// The joint's own handle.
    pub handle: JointHandle<D>,
    /// The first body.
    pub first: BodyHandle<D>,
    /// The second body.
    pub second: BodyHandle<D>,
    /// The kind and numbers.
    pub shape: D::Joint,
    /// See [`JointDesc::collide_connected`].
    pub collide_connected: bool,
}

// Written by hand: `#[derive]` would add the bound `D: Clone` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for JointEntry<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for JointEntry<D> {}

/// The key of a body pair in the no-contact list: the two slot indices, lowest first.
type PairKey = (u32, u32);

fn key_of<D: Dim>(entry: &JointEntry<D>) -> PairKey {
    ordered(entry.first.index(), entry.second.index())
}

fn ordered(a: u32, b: u32) -> PairKey {
    if a < b { (a, b) } else { (b, a) }
}

/// The joints of a world.
pub(super) struct JointTable<D: Dim> {
    handles: Pool<JointTag<D>>,
    slots: Vec<Option<JointEntry<D>>>,
    /// The live joints in slot order, for the solver; rebuilt when a joint is created or destroyed.
    live: Vec<JointEntry<D>>,
    /// The body pairs of the joints with `collide_connected == false`, sorted; a pair with two such
    /// joints appears twice.
    no_contact: Vec<PairKey>,
}

impl<D: Dim> JointTable<D> {
    /// Storage for `capacity` joints.
    ///
    /// # Errors
    /// [`PhysicsError::JointCapacityTooLarge`].
    pub(super) fn new(capacity: u32) -> Result<Self, PhysicsError> {
        if capacity > MAX_JOINTS {
            return Err(PhysicsError::JointCapacityTooLarge { max: MAX_JOINTS });
        }
        let room = capacity as usize;
        let mut slots = Vec::with_capacity(room);
        slots.resize_with(room, || None);
        Ok(Self {
            handles: Pool::with_capacity(capacity),
            slots,
            live: Vec::with_capacity(room),
            no_contact: Vec::with_capacity(room),
        })
    }

    pub(super) fn capacity(&self) -> usize {
        self.handles.capacity()
    }

    pub(super) fn len(&self) -> usize {
        self.handles.len()
    }

    /// The live joints, in slot order.
    pub(super) fn live(&self) -> &[JointEntry<D>] {
        &self.live
    }

    /// The joint `handle` names.
    pub(super) fn get(&self, handle: JointHandle<D>) -> Result<&JointEntry<D>, PhysicsError> {
        self.slots
            .get(handle.index() as usize)
            .and_then(Option::as_ref)
            .filter(|entry| entry.handle == handle)
            .ok_or(PhysicsError::StaleJoint)
    }

    /// Adds a joint between two bodies already checked by the world.
    pub(super) fn create(&mut self, desc: &JointDesc<D>) -> Result<JointHandle<D>, PhysicsError> {
        desc.shape.validate()?;
        let handle = self
            .handles
            .insert(JointTag::new())
            .map_err(|_| PhysicsError::JointCapacityExhausted)?;
        let entry = JointEntry {
            handle,
            first: desc.first,
            second: desc.second,
            shape: desc.shape,
            collide_connected: desc.collide_connected,
        };
        if let Some(slot) = self.slots.get_mut(handle.index() as usize) {
            *slot = Some(entry);
        }
        if !entry.collide_connected {
            self.exempt(key_of(&entry));
        }
        self.refresh();
        Ok(handle)
    }

    /// Removes a joint.
    pub(super) fn destroy(
        &mut self,
        handle: JointHandle<D>,
    ) -> Result<JointEntry<D>, PhysicsError> {
        let entry = *self.get(handle)?;
        self.handles.remove(handle);
        if let Some(slot) = self.slots.get_mut(handle.index() as usize) {
            *slot = None;
        }
        if !entry.collide_connected {
            self.unexempt(key_of(&entry));
        }
        self.refresh();
        Ok(entry)
    }

    /// Removes every joint that touches the body in slot `body`, calling `each` with the other
    /// body of each, so the world can wake it. Slot order.
    pub(super) fn destroy_touching(&mut self, body: u32, mut each: impl FnMut(BodyHandle<D>)) {
        if self.handles.is_empty() {
            return;
        }
        for index in 0..self.slots.len() {
            self.destroy_if_touching(index, body, &mut each);
        }
    }

    /// Destroys the joint in slot `index` if it touches the body in slot `body`.
    fn destroy_if_touching(
        &mut self,
        index: usize,
        body: u32,
        each: &mut impl FnMut(BodyHandle<D>),
    ) {
        let Some((handle, other)) = self.touching(index, body) else {
            return;
        };
        each(other);
        let _ = self.destroy(handle);
    }

    /// The handle of the joint in slot `index` and its end that is not the body in slot `body`, if
    /// the joint exists and touches that body.
    fn touching(&self, index: usize, body: u32) -> Option<(JointHandle<D>, BodyHandle<D>)> {
        let entry = self.slots.get(index).copied().flatten()?;
        if entry.first.index() == body {
            Some((entry.handle, entry.second))
        } else if entry.second.index() == body {
            Some((entry.handle, entry.first))
        } else {
            None
        }
    }

    /// Adds one entry for `key` to the no-contact list, keeping it sorted.
    fn exempt(&mut self, key: PairKey) {
        let at = self.no_contact.partition_point(|&other| other < key);
        self.no_contact.insert(at, key);
    }

    /// Removes one entry for `key` from the no-contact list.
    fn unexempt(&mut self, key: PairKey) {
        let at = self.no_contact.partition_point(|&other| other < key);
        if self.no_contact.get(at) == Some(&key) {
            self.no_contact.remove(at);
        }
    }

    /// True when a joint of the table keeps the bodies in slots `a` and `b` from making a
    /// manifold.
    pub(super) fn forbids_contact(&self, a: u32, b: u32) -> bool {
        let key = ordered(a, b);
        let at = self.no_contact.partition_point(|&other| other < key);
        self.no_contact.get(at) == Some(&key)
    }

    fn refresh(&mut self) {
        self.live.clear();
        self.live.extend(self.slots.iter().flatten().copied());
    }
}
