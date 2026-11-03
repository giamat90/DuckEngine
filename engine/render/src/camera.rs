//! The 2D camera: an orthographic view-projection matrix (design: `wiki/architecture/render-port-v0.md`
//! section 5). Right-handed, Y up, depth 0 to 1, column-major, like every matrix in the engine.

use base::math::{Mat4, Vec2};

/// Depth range in front of the camera; sprites sit at z = 0, inside it.
const NEAR: f32 = -1.0;
const FAR: f32 = 1.0;

const TWO_POW_16: f32 = 65_536.0;
const LOW_16: u32 = 0xFFFF;

/// A pixel count as `f32` (exact up to 2^24, correctly rounded above), without a cast.
fn to_f32(count: u32) -> f32 {
    let high = u16::try_from(count >> 16).unwrap_or(u16::MAX);
    let low = u16::try_from(count & LOW_16).unwrap_or(u16::MAX);
    f32::from(high) * TWO_POW_16 + f32::from(low)
}

/// A camera looking at the XY plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrthographicCamera {
    /// The world point at the centre of the view.
    pub position: Vec2,
    /// World units visible from bottom to top.
    pub height: f32,
    /// Width over height of the view (of the viewport).
    pub aspect: f32,
}

impl OrthographicCamera {
    /// A camera that shows whole pixels: one world unit is `pixels_per_unit` pixels of a
    /// `viewport_width` by `viewport_height` viewport, and `position` snaps to the pixel grid. With an odd viewport
    /// size the view edges fall on half pixels; use even sizes for exact pixel alignment.
    /// `None` for an empty viewport or a scale that is not positive and finite.
    #[must_use]
    pub fn pixel_perfect(
        viewport_width: u32,
        viewport_height: u32,
        position: Vec2,
        pixels_per_unit: f32,
    ) -> Option<Self> {
        let scale_ok = pixels_per_unit.is_finite() && pixels_per_unit > 0.0;
        if viewport_width == 0 || viewport_height == 0 || !scale_ok {
            return None;
        }
        let snap = |v: f32| (v * pixels_per_unit).round() / pixels_per_unit;
        let (width, height) = (to_f32(viewport_width), to_f32(viewport_height));
        Some(Self {
            position: Vec2::new(snap(position.x), snap(position.y)),
            height: height / pixels_per_unit,
            aspect: width / height,
        })
    }

    /// The matrix from world to clip space; `None` when the view has no extent or a value is not
    /// finite.
    #[must_use]
    pub fn view_projection(&self) -> Option<Mat4> {
        let half_height = self.height * 0.5;
        let half_width = half_height * self.aspect;
        let min = Vec2::new(self.position.x - half_width, self.position.y - half_height);
        let max = Vec2::new(self.position.x + half_width, self.position.y + half_height);
        if self.height <= 0.0 || self.aspect <= 0.0 {
            return None;
        }
        Mat4::orthographic_rh(min, max, NEAR, FAR)
    }
}

/// The 64 bytes of `matrix` for a uniform buffer: sixteen little-endian floats, column by column.
#[must_use]
pub fn matrix_bytes(matrix: &Mat4) -> [u8; 64] {
    let mut out = [0u8; 64];
    for (chunk, value) in out.chunks_exact_mut(4).zip(matrix.to_cols_array()) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests;
