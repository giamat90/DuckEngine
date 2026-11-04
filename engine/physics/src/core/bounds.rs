//! The axis-aligned box a broadphase tree stores, as a trait so that the tree is written once for
//! every dimension (the 2D instance supplies a rectangle, a 3D one would supply a box).

use super::hash::HashBits;

/// An axis-aligned bounding box of some dimension.
pub trait Bounds: Copy + Default + PartialEq + HashBits {
    /// True when the two boxes share at least one point. Boxes that only touch (an edge or a face
    /// of one lies exactly on the other) do overlap.
    fn overlaps(&self, other: &Self) -> bool;

    /// The smallest box that holds both.
    #[must_use]
    fn union(&self, other: &Self) -> Self;

    /// True when `inner` lies inside `self` or on its boundary.
    fn contains(&self, inner: &Self) -> bool;

    /// The cost measure of the tree heuristic: the perimeter in 2D, the surface area in 3D.
    fn measure(&self) -> f32;

    /// The box grown by `margin` on every side.
    #[must_use]
    fn fattened(&self, margin: f32) -> Self;

    /// True when every coordinate is finite and no minimum is above its maximum.
    fn is_valid(&self) -> bool;
}
