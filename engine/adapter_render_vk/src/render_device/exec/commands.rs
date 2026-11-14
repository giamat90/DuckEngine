//! The state commands and the draws of a stream: pipeline, viewport, binds, draw calls.
//!
//! The port's viewport has its origin at the bottom left (OpenGL's); Vulkan's is at the top left.
//! Into the backbuffer a viewport with a negative height flips the image the way GL's bottom-left
//! origin does, so the same shaders and projections give the same picture in the window; the
//! scissor rectangle has no flip and is converted by hand. Into a texture nothing is flipped:
//! the port says row 0 of a texture is its bottom row, which is where clip space `y = -1` lands
//! with the unflipped viewport, so a texture holds the same rows on both adapters and a later pass
//! samples it the same way.

use super::{Exec, INVALID};
use crate::draw::Bindings;
use crate::resources::{BufferId as OwnBuffer, SamplerId as OwnSampler};
use crate::table::Table;
use crate::vk_draw::{Handle, Viewport};
use crate::vk_present::{Extent2D, Offset2D, Rect2D};
use crate::vk_resource::LAYOUT_SHADER_READ;
use render::desc::{IndexType, Rect};
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SamplerId, TextureId};
use render::stream::Command;

/// The largest viewport side Vulkan guarantees (`maxViewportDimensions` is at least this).
const MAX_VIEWPORT: u32 = 4096;

/// The device's tables a run reads (the pipelines hold one Vulkan pipeline per target format).
pub struct Objects<'a> {
    pub pipelines: &'a Table<[Handle; 2]>,
    pub buffers: &'a Table<OwnBuffer>,
    pub samplers: &'a Table<OwnSampler>,
}

impl Exec<'_> {
    /// Runs a command that is not a pass or a barrier.
    pub(super) fn state_or_draw(&mut self, command: Command) -> Result<(), RenderError> {
        match command {
            Command::SetPipeline(id) => self.set_pipeline(id),
            Command::SetViewport(rect) => {
                self.set_viewport(rect);
                Ok(())
            }
            Command::BindUniforms { slot, buffer } => self.bind_uniforms(slot, buffer),
            Command::BindTexture {
                slot,
                texture,
                sampler,
            } => self.bind_texture(slot, texture, sampler),
            Command::BindVertexBuffer { buffer, offset, .. } => {
                let handle = self.buffer_handle(buffer)?;
                self.draw.bind_vertex(self.cmd, handle, u64::from(offset));
                Ok(())
            }
            Command::BindIndexBuffer {
                buffer,
                offset,
                index_type,
            } => {
                let handle = self.buffer_handle(buffer)?;
                let wide = index_type == IndexType::U32;
                self.draw
                    .bind_index(self.cmd, handle, u64::from(offset), wide);
                Ok(())
            }
            Command::Draw {
                vertices,
                instances,
            } => {
                self.flush_bindings()?;
                self.draw.draw(self.cmd, vertices, instances);
                Ok(())
            }
            Command::DrawIndexed { indices, instances } => {
                self.flush_bindings()?;
                self.draw.draw_indexed(self.cmd, indices, instances);
                Ok(())
            }
            // Passes and barriers are the caller's.
            Command::BeginPass(_) | Command::EndPass | Command::Barrier { .. } => Ok(()),
        }
    }

    fn set_pipeline(&mut self, id: PipelineId) -> Result<(), RenderError> {
        let variants = self
            .objects
            .pipelines
            .get(id)
            .ok_or(RenderError::InvalidHandle)?;
        self.draw
            .bind_pipeline(self.cmd, variants[usize::from(self.state.target.bgra)]);
        Ok(())
    }

    /// A pass starts: nothing is bound, and the whole target is the viewport and the scissor.
    pub(super) fn start_pass_state(&mut self) {
        let (width, height) = self.state.target.extent;
        self.state.bindings = Bindings::default();
        self.state.dirty = true;
        self.set_viewport(Rect {
            x: 0,
            y: 0,
            width,
            height,
        });
    }

    fn set_viewport(&mut self, rect: Rect) {
        let (width, height) = self.state.target.extent;
        let (viewport, scissor) = fitted(rect, width, height, self.state.target.flip);
        self.draw.set_viewport(self.cmd, &viewport, &scissor);
    }

    fn buffer_handle(&self, buffer: BufferId) -> Result<u64, RenderError> {
        let own = self
            .objects
            .buffers
            .get(buffer)
            .ok_or(RenderError::InvalidHandle)?;
        self.resources
            .buffer_handle(*own)
            .map_err(|error| super::render_error(&error))
    }

    fn slot_of(slot: u32) -> Result<usize, RenderError> {
        usize::try_from(slot).map_err(|_| INVALID)
    }

    fn bind_uniforms(&mut self, slot: u32, buffer: BufferId) -> Result<(), RenderError> {
        let handle = self.buffer_handle(buffer)?;
        let place = self
            .state
            .bindings
            .uniforms
            .get_mut(Self::slot_of(slot)?)
            .ok_or(INVALID)?;
        *place = Some(handle);
        self.state.dirty = true;
        Ok(())
    }

    fn bind_texture(
        &mut self,
        slot: u32,
        texture: TextureId,
        sampler: SamplerId,
    ) -> Result<(), RenderError> {
        let record = self
            .textures
            .get(texture)
            .ok_or(RenderError::InvalidHandle)?;
        let handles = self
            .resources
            .texture_handles(record.id)
            .map_err(|error| super::render_error(&error))?;
        // A rendering scope cannot hold a barrier: the texture must be readable already.
        if handles.layout != LAYOUT_SHADER_READ {
            return Err(INVALID);
        }
        let own = self
            .objects
            .samplers
            .get(sampler)
            .ok_or(RenderError::InvalidHandle)?;
        let sampler = self
            .resources
            .sampler_handle(*own)
            .map_err(|error| super::render_error(&error))?;
        let place = self
            .state
            .bindings
            .textures
            .get_mut(Self::slot_of(slot)?)
            .ok_or(INVALID)?;
        *place = Some((handles.view, sampler));
        self.state.dirty = true;
        Ok(())
    }

    /// Takes a new pair of sets and writes what is bound into it, when a bind changed it.
    fn flush_bindings(&mut self) -> Result<(), RenderError> {
        if !self.state.dirty {
            return Ok(());
        }
        self.draw
            .bind_descriptors(self.cmd, self.pool, &self.state.bindings)
            .map_err(|error| super::render_error(&error))?;
        self.state.dirty = false;
        Ok(())
    }
}

/// The Vulkan viewport and scissor for a port rectangle (origin bottom left) on a target of
/// `width` x `height`; `flip` for the backbuffer, whose row 0 is the top. The scissor is clamped
/// to the target, as GL clamps its own.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "target sizes and offsets are far below the 2^24 an f32 holds exactly"
)]
pub fn fitted(rect: Rect, width: u32, height: u32, flip: bool) -> (Viewport, Rect2D) {
    let (w, h) = (i64::from(width), i64::from(height));
    let (x, y) = (i64::from(rect.x), i64::from(rect.y));
    let (rw, rh) = (i64::from(rect.width), i64::from(rect.height));
    // Vulkan wants a viewport with area; the scissor below is empty for a degenerate rectangle.
    let (vw, vh) = (
        rect.width.clamp(1, MAX_VIEWPORT),
        rect.height.clamp(1, MAX_VIEWPORT),
    );
    let viewport = Viewport {
        x: rect.x as f32,
        y: if flip { (h - y) as f32 } else { y as f32 },
        width: vw as f32,
        height: if flip { -(vh as f32) } else { vh as f32 },
        min_depth: 0.0,
        max_depth: 1.0,
    };
    let left = x.clamp(0, w);
    let right = (x + rw).clamp(0, w);
    let (top, bottom) = if flip {
        ((h - (y + rh)).clamp(0, h), (h - y).clamp(0, h))
    } else {
        (y.clamp(0, h), (y + rh).clamp(0, h))
    };
    let scissor = Rect2D {
        offset: Offset2D {
            x: i32::try_from(left).unwrap_or(0),
            y: i32::try_from(top).unwrap_or(0),
        },
        extent: Extent2D {
            width: u32::try_from(right - left).unwrap_or(0),
            height: u32::try_from(bottom - top).unwrap_or(0),
        },
    };
    (viewport, scissor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_whole_target_is_a_flipped_viewport_over_the_whole_target() {
        let (viewport, scissor) = fitted(rect(0, 0, 640, 480), 640, 480, true);
        assert_eq!(
            (viewport.x, viewport.y, viewport.width, viewport.height),
            (0.0, 480.0, 640.0, -480.0)
        );
        assert_eq!((viewport.min_depth, viewport.max_depth), (0.0, 1.0));
        assert_eq!(
            scissor,
            Rect2D {
                offset: Offset2D { x: 0, y: 0 },
                extent: Extent2D {
                    width: 640,
                    height: 480
                }
            }
        );
    }

    #[test]
    fn a_rectangle_from_the_bottom_left_lands_at_the_bottom_of_the_target() {
        // 100 x 50 at x 10, 20 up from the bottom of a 640 x 480 target.
        let (viewport, scissor) = fitted(rect(10, 20, 100, 50), 640, 480, true);
        assert_eq!(
            (viewport.x, viewport.y, viewport.width, viewport.height),
            (10.0, 460.0, 100.0, -50.0)
        );
        // Its top edge is 480 - (20 + 50) rows from the top.
        assert_eq!((scissor.offset.x, scissor.offset.y), (10, 410));
        assert_eq!((scissor.extent.width, scissor.extent.height), (100, 50));
    }

    #[test]
    fn a_texture_target_is_not_flipped_so_row_0_is_the_bottom_as_on_gl() {
        let (viewport, scissor) = fitted(rect(10, 20, 100, 50), 640, 480, false);
        assert_eq!(
            (viewport.x, viewport.y, viewport.width, viewport.height),
            (10.0, 20.0, 100.0, 50.0)
        );
        assert_eq!((scissor.offset.x, scissor.offset.y), (10, 20));
        assert_eq!((scissor.extent.width, scissor.extent.height), (100, 50));
        let (_, clamped) = fitted(rect(-10, -5, 100, 100), 64, 48, false);
        assert_eq!((clamped.offset.x, clamped.offset.y), (0, 0));
        assert_eq!((clamped.extent.width, clamped.extent.height), (64, 48));
    }

    #[test]
    fn a_degenerate_rectangle_still_gives_a_viewport_with_area_and_an_empty_scissor() {
        let (viewport, scissor) = fitted(rect(5, 5, 0, 0), 64, 48, true);
        assert_eq!((viewport.width, viewport.height), (1.0, -1.0));
        assert_eq!((scissor.extent.width, scissor.extent.height), (0, 0));
        let (huge, _) = fitted(rect(0, 0, u32::MAX, 100_000), 64, 48, false);
        assert_eq!((huge.width, huge.height), (4096.0, 4096.0));
    }

    #[test]
    fn the_scissor_is_clamped_to_the_target() {
        let (_, scissor) = fitted(rect(-10, -5, 100, 50), 64, 48, true);
        assert_eq!((scissor.offset.x, scissor.offset.y), (0, 48 - 45));
        assert_eq!((scissor.extent.width, scissor.extent.height), (64, 45));
        let (_, beyond) = fitted(rect(60, 40, 100, 100), 64, 48, true);
        assert_eq!((beyond.offset.x, beyond.offset.y), (60, 0));
        assert_eq!((beyond.extent.width, beyond.extent.height), (4, 8));
    }

    #[test]
    fn a_rectangle_outside_the_target_has_an_empty_scissor() {
        let (_, scissor) = fitted(rect(100, 0, 10, 10), 64, 48, true);
        assert_eq!(scissor.extent.width, 0);
        let (_, below) = fitted(rect(0, -20, 10, 10), 64, 48, true);
        assert_eq!(below.extent.height, 0);
    }
}
