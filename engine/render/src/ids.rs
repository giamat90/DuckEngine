//! Resource handles. Each is a generational [`Handle`] of its own marker type, so a texture handle
//! cannot be passed where a buffer handle is expected, and a stale handle is detected.

use base::pool::Handle;

/// Marker of [`BufferId`].
#[derive(Debug)]
pub struct Buffer;
/// Marker of [`TextureId`].
#[derive(Debug)]
pub struct Texture;
/// Marker of [`SamplerId`].
#[derive(Debug)]
pub struct Sampler;
/// Marker of [`ShaderId`].
#[derive(Debug)]
pub struct Shader;
/// Marker of [`PipelineId`].
#[derive(Debug)]
pub struct Pipeline;
/// Marker of [`SurfaceId`].
#[derive(Debug)]
pub struct Surface;

/// A vertex, index or uniform buffer.
pub type BufferId = Handle<Buffer>;
/// A 2D texture.
pub type TextureId = Handle<Texture>;
/// A sampler state.
pub type SamplerId = Handle<Sampler>;
/// A shader module.
pub type ShaderId = Handle<Shader>;
/// A graphics pipeline.
pub type PipelineId = Handle<Pipeline>;
/// A presentable surface (a window's framebuffer).
pub type SurfaceId = Handle<Surface>;

/// One frame between `begin_frame` and `end_frame`. A plain counter issued by the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameId(pub u64);
