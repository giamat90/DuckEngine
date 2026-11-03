//! Plain-data descriptions of resources and passes: what the port is asked to create or begin.

use crate::ids::{ShaderId, TextureId};

/// What a buffer is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferUsage {
    /// Vertex data.
    Vertex,
    /// Index data.
    Index,
    /// Uniform (constant) data.
    Uniform,
}

/// Buffer creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc {
    /// Size in bytes; must be non-zero.
    pub size: u32,
    /// Intended use.
    pub usage: BufferUsage,
}

/// Texel format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    /// 8-bit RGBA, unsigned normalised.
    Rgba8,
    /// 8-bit BGRA, unsigned normalised (typical swap-chain format).
    Bgra8,
    /// 32-bit float depth.
    Depth32F,
}

/// 2D texture creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureDesc {
    /// Width in texels; must be non-zero.
    pub width: u32,
    /// Height in texels; must be non-zero.
    pub height: u32,
    /// Number of mip levels; at least 1.
    pub mip_count: u32,
    /// Texel format.
    pub format: TextureFormat,
}

/// A rectangle of texels, for texture writes and viewports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Texture filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Nearest texel.
    Nearest,
    /// Bilinear.
    Linear,
}

/// Behaviour outside 0..1 texture coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressMode {
    /// Clamp to the edge.
    Clamp,
    /// Repeat.
    Repeat,
}

/// Sampler creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplerDesc {
    /// Filter when minifying.
    pub min_filter: Filter,
    /// Filter when magnifying.
    pub mag_filter: Filter,
    /// Addressing in both directions.
    pub address: AddressMode,
}

/// Which pipeline stage a shader module is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderStage {
    /// Vertex stage.
    Vertex,
    /// Fragment stage.
    Fragment,
}

/// Shader module creation: an opaque precompiled blob and its entry point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderDesc<'a> {
    /// Stage of the module.
    pub stage: ShaderStage,
    /// The compiled blob, format defined by the backend.
    pub code: &'a [u8],
    /// Entry-point name.
    pub entry: &'a str,
}

/// Format of one vertex attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexFormat {
    /// Two 32-bit floats.
    Float2,
    /// Three 32-bit floats.
    Float3,
    /// Four 32-bit floats.
    Float4,
    /// Four 8-bit unsigned normalised values.
    Unorm8x4,
}

/// One attribute of the vertex layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VertexAttribute {
    /// Shader input location.
    pub location: u32,
    /// Data format.
    pub format: VertexFormat,
    /// Byte offset inside one vertex.
    pub offset: u32,
}

/// Colour blending of a pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    /// Source replaces destination.
    Opaque,
    /// Source-alpha blending.
    Alpha,
}

/// Depth state of a pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepthState {
    /// Whether the depth test is on.
    pub test: bool,
    /// Whether depth is written.
    pub write: bool,
}

/// Graphics pipeline creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineDesc<'a> {
    /// Vertex-stage shader.
    pub vertex_shader: ShaderId,
    /// Fragment-stage shader.
    pub fragment_shader: ShaderId,
    /// Bytes between two vertices. v0 has one vertex buffer: attributes read vertex slot 0.
    pub vertex_stride: u32,
    /// Vertex attributes.
    pub attributes: &'a [VertexAttribute],
    /// Blending.
    pub blend: BlendMode,
    /// Depth state.
    pub depth: DepthState,
    /// Format of the colour target.
    pub color_format: TextureFormat,
}

/// Index width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexType {
    /// 16-bit indices.
    U16,
    /// 32-bit indices.
    U32,
}

/// Surface creation: a native window token and the framebuffer size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDesc {
    /// Not a pointer to dereference. The opaque native token of the window (`platform::window::NativeToken`'s value), meaningful
    /// only to the adapter that produced the window.
    pub native_token: u64,
    /// Framebuffer width in pixels.
    pub width: u32,
    /// Framebuffer height in pixels.
    pub height: u32,
}

/// What a render pass draws into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderTarget {
    /// The backbuffer of the current frame.
    Backbuffer,
    /// An offscreen colour texture.
    Texture(TextureId),
}

/// Beginning of a render pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassDesc {
    /// Where the pass draws.
    pub target: RenderTarget,
    /// Clear colour (RGBA), or keep the contents.
    pub clear_color: Option<[f32; 4]>,
    /// Clear depth, or keep the contents.
    pub clear_depth: Option<f32>,
}

/// Counts reported by `submit`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubmitStats {
    /// Commands executed.
    pub commands: u32,
    /// Draw calls executed.
    pub draws: u32,
}

/// The state a resource is in, as far as the graph tracks it. A barrier moves a resource from one
/// state to another; Vulkan and D3D12 map these to layouts and access masks, OpenGL to no-ops or
/// memory barriers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceState {
    /// Contents undefined (never written, or just created).
    Undefined,
    /// Written as the colour target of a pass.
    ColorTarget,
    /// Read by shaders (sampled).
    ShaderRead,
    /// Ready to be presented (meant for the backbuffer; the Null device does not enforce it).
    Present,
}
