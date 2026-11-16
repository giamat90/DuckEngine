//! The components of the sandbox that no engine crate owns.

use base::math::Vec2;

/// The velocity a body had after the last step. An impact is a sudden change of it: the glue
/// carries no contact events, so the sandbox compares the velocity of a body with the one it had
/// a step before.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Motion {
    /// Metres per second.
    pub velocity: Vec2,
}
