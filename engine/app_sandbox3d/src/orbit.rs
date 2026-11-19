//! The orbit camera as plain data and pure functions: drag and wheel amounts go in, a
//! [`PerspectiveCamera`] comes out. No window, no device, no clock.

use base::dmath::sin_cos;
use base::math::Vec3;
use render::camera::PerspectiveCamera;

/// Radians of turn per pixel of drag.
pub const TURN_PER_PIXEL: f32 = 0.008;
/// The part of the distance one wheel notch takes away (or adds).
pub const ZOOM_PER_NOTCH: f32 = 0.1;
/// Lowest pitch: just above the horizon, radians.
pub const MIN_PITCH: f32 = 0.05;
/// Highest pitch: just short of straight down, radians.
pub const MAX_PITCH: f32 = 1.5;
/// Closest the camera comes to its target, metres.
pub const MIN_DISTANCE: f32 = 4.0;
/// Farthest the camera goes from its target, metres.
pub const MAX_DISTANCE: f32 = 40.0;
/// Vertical field of view, radians (about 55 degrees).
pub const FOV_Y: f32 = 0.96;
/// Near plane, metres.
pub const NEAR: f32 = 0.1;
/// Far plane, metres.
pub const FAR: f32 = 100.0;

/// A camera on a sphere around a target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// Turn about the vertical axis, radians.
    yaw: f32,
    /// Height above the horizon, radians, between [`MIN_PITCH`] and [`MAX_PITCH`].
    pitch: f32,
    /// Distance from the target, metres, between [`MIN_DISTANCE`] and [`MAX_DISTANCE`].
    distance: f32,
    /// The point it looks at.
    target: Vec3,
}

impl OrbitCamera {
    /// A camera looking at `target` from `distance` metres, at `yaw` and `pitch` radians; the
    /// pitch and the distance are brought into their ranges, a value that is not a number
    /// becomes the nearest end.
    #[must_use]
    pub fn new(target: Vec3, yaw: f32, pitch: f32, distance: f32) -> Self {
        Self {
            yaw: if yaw.is_finite() { yaw } else { 0.0 },
            pitch: clamp(pitch, MIN_PITCH, MAX_PITCH),
            distance: clamp(distance, MIN_DISTANCE, MAX_DISTANCE),
            target,
        }
    }

    /// Follows a drag of `dx`, `dy` pixels: right turns the view round the target, down raises it.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() {
            self.yaw -= dx * TURN_PER_PIXEL;
        }
        if dy.is_finite() {
            self.pitch = clamp(self.pitch + dy * TURN_PER_PIXEL, MIN_PITCH, MAX_PITCH);
        }
    }

    /// Follows `notches` of the wheel: forward (positive) comes closer.
    pub fn zoom(&mut self, notches: f32) {
        if notches.is_finite() {
            let factor = 1.0 - notches * ZOOM_PER_NOTCH;
            self.distance = clamp(self.distance * factor, MIN_DISTANCE, MAX_DISTANCE);
        }
    }

    /// The turn about the vertical axis, radians.
    #[must_use]
    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The height above the horizon, radians.
    #[must_use]
    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    /// The distance from the target, metres.
    #[must_use]
    pub fn distance(&self) -> f32 {
        self.distance
    }

    /// Where the camera is.
    #[must_use]
    pub fn position(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = sin_cos(self.yaw);
        let (sin_pitch, cos_pitch) = sin_cos(self.pitch);
        self.target + Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw) * self.distance
    }

    /// The lens for a view `aspect` times as wide as it is high.
    #[must_use]
    pub fn camera(&self, aspect: f32) -> PerspectiveCamera {
        PerspectiveCamera {
            position: self.position(),
            target: self.target,
            up: Vec3::new(0.0, 1.0, 0.0),
            fov_y: FOV_Y,
            aspect,
            near: NEAR,
            far: FAR,
        }
    }
}

/// `value` held between `low` and `high`; a value that is not a number gives `low`.
fn clamp(value: f32, low: f32, high: f32) -> f32 {
    if value.is_nan() {
        low
    } else {
        value.clamp(low, high)
    }
}

#[cfg(test)]
mod tests;
