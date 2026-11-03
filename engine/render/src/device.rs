//! The render device port `IRenderDevice` v0, shaped after Vulkan and D3D12: explicit resources,
//! pipeline state objects, one command stream per frame. v0 limits: triangle lists only, no culling,
//! vertex data from slot 0 only, uniform buffers bind whole. Object-safe; held as `dyn IRenderDevice`.

use crate::caps::RenderCaps;
use crate::desc::{
    BufferDesc, PipelineDesc, Rect, SamplerDesc, ShaderDesc, SubmitStats, SurfaceDesc, TextureDesc,
};
use crate::error::RenderError;
use crate::ids::{BufferId, FrameId, PipelineId, SamplerId, ShaderId, SurfaceId, TextureId};
use crate::stream::CommandStream;

/// The device port. Every method reports failure as a value.
pub trait IRenderDevice {
    /// What this device can do.
    fn caps(&self) -> RenderCaps;

    /// Creates a presentable surface for a native window token.
    ///
    /// # Errors
    /// `InvalidDescription` for a zero size or unusable token, `OutOfMemory`, `Unsupported`.
    fn create_surface(&mut self, desc: SurfaceDesc) -> Result<SurfaceId, RenderError>;

    /// Tells the device the framebuffer size changed (a call, not an event it polls).
    ///
    /// # Errors
    /// `InvalidHandle` for a stale surface, `InvalidDescription` for a zero size.
    fn resize_surface(
        &mut self,
        surface: SurfaceId,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError>;

    /// Destroys a surface; a stale handle is ignored.
    fn destroy_surface(&mut self, surface: SurfaceId);

    /// Creates a buffer.
    ///
    /// # Errors
    /// `InvalidDescription` (zero size), `OutOfMemory`.
    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId, RenderError>;

    /// Creates a 2D texture.
    ///
    /// # Errors
    /// `InvalidDescription` (zero size, zero mips, size over the cap), `OutOfMemory`.
    fn create_texture(&mut self, desc: TextureDesc) -> Result<TextureId, RenderError>;

    /// Creates a sampler.
    ///
    /// # Errors
    /// `OutOfMemory`.
    fn create_sampler(&mut self, desc: SamplerDesc) -> Result<SamplerId, RenderError>;

    /// Creates a shader module from a precompiled blob.
    ///
    /// # Errors
    /// `ShaderRejected` for a blob the device cannot use, `OutOfMemory`.
    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId, RenderError>;

    /// Creates a graphics pipeline.
    ///
    /// # Errors
    /// `InvalidHandle` for a stale shader, `InvalidDescription`, `OutOfMemory`.
    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> Result<PipelineId, RenderError>;

    /// Copies `bytes` into a buffer at `offset`.
    ///
    /// # Errors
    /// `InvalidHandle`, or `InvalidDescription` when the range leaves the buffer.
    fn write_buffer(
        &mut self,
        buffer: BufferId,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), RenderError>;

    /// Copies tightly packed texels into a region of mip level 0.
    ///
    /// # Errors
    /// `InvalidHandle`, or `InvalidDescription` when the region or byte count does not fit.
    fn write_texture(
        &mut self,
        texture: TextureId,
        region: Rect,
        bytes: &[u8],
    ) -> Result<(), RenderError>;

    /// Destroys a buffer; a stale handle is ignored.
    fn destroy_buffer(&mut self, buffer: BufferId);

    /// Destroys a texture; a stale handle is ignored.
    fn destroy_texture(&mut self, texture: TextureId);

    /// Destroys a shader module; a stale handle is ignored. Pipelines already created from it stay
    /// valid (the device keeps what it compiled).
    fn destroy_shader(&mut self, shader: ShaderId);

    /// Destroys a sampler; a stale handle is ignored.
    fn destroy_sampler(&mut self, sampler: SamplerId);

    /// Destroys a pipeline; a stale handle is ignored.
    fn destroy_pipeline(&mut self, pipeline: PipelineId);

    /// Starts a frame on a surface.
    ///
    /// # Errors
    /// `InvalidHandle`, `SurfaceLost`.
    fn begin_frame(&mut self, surface: SurfaceId) -> Result<FrameId, RenderError>;

    /// Executes one stream for the frame.
    ///
    /// # Errors
    /// `InvalidHandle` for a stale frame or resource, `InvalidDescription` for a malformed stream or
    /// a command that breaks the validation rules (pass nesting, pipeline before draw, ranges).
    fn submit(
        &mut self,
        frame: FrameId,
        stream: &CommandStream,
    ) -> Result<SubmitStats, RenderError>;

    /// Ends the frame and presents.
    ///
    /// # Errors
    /// `InvalidHandle`, `SurfaceLost`.
    fn end_frame(&mut self, frame: FrameId) -> Result<(), RenderError>;

    /// Copies a texture's mip level 0 into `out`, tightly packed (for the image test).
    ///
    /// # Errors
    /// `InvalidHandle`, `InvalidDescription` when `out` has the wrong length.
    fn read_back(&mut self, texture: TextureId, out: &mut [u8]) -> Result<(), RenderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_is_object_safe() {
        fn takes(_: Option<&mut dyn IRenderDevice>) {}
        takes(None);
    }
}
