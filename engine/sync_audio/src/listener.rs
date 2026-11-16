//! Where the sound is heard from, in world space.

/// The listener of a 2D scene: a position along x and how far to each side a sound is still
/// panned. Beyond `half_width` a sound is at the hard left or hard right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    /// World x of the listener, usually the x of the camera.
    pub x: f32,
    /// World distance from the listener to the hard left or right; usually half the view width.
    pub half_width: f32,
}

impl Listener {
    /// The pan in -1 (hard left) to 1 (hard right) of a sound at world `x`. Zero when `x` or
    /// the listener is not a usable number.
    #[must_use]
    pub fn pan(&self, x: f32) -> f32 {
        let usable = x.is_finite()
            && self.x.is_finite()
            && self.half_width.is_finite()
            && self.half_width > 0.0;
        if !usable {
            return 0.0;
        }
        ((x - self.x) / self.half_width).clamp(-1.0, 1.0)
    }
}
