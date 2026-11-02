//! The Null render device: no GPU, no window, no external code. It implements the whole port,
//! keeps buffer and texture bytes so uploads read back, applies clears to textures, and validates
//! every stream against the rules in [`validate`](self::validate). It is the reference for what any
//! other device must accept, and the device every test can use.

mod resources;
mod validate;

use crate::caps::RenderCaps;
use crate::desc::{
    BufferDesc, PipelineDesc, Rect, SamplerDesc, ShaderDesc, SubmitStats, SurfaceDesc, TextureDesc,
    TextureFormat, VertexFormat,
};
use crate::desc::{PassDesc, RenderTarget, ShaderStage};
use crate::device::IRenderDevice;
use crate::error::RenderError;
use crate::ids::{BufferId, FrameId, PipelineId, SamplerId, ShaderId, SurfaceId, TextureId};
use crate::stream::{Command, CommandStream};
use resources::{
    BYTES_PER_TEXEL, BufferRec, PipelineRec, Resources, SamplerRec, ShaderRec, SurfaceRec,
    TextureRec, insert, lookup, retag,
};
use validate::validate;

/// Uniform and texture slots are numbered `0..MAX_BINDING_SLOTS`.
pub const MAX_BINDING_SLOTS: u32 = 8;
/// Largest texture side the Null device accepts.
const MAX_TEXTURE_SIZE: u32 = 4096;
/// Resources of each kind a default device can hold.
const DEFAULT_CAPACITY: u32 = 256;

const INVALID: RenderError = RenderError::InvalidDescription;

/// A device that draws nothing and checks everything.
pub struct NullDevice {
    resources: Resources,
    frame: Option<(FrameId, SurfaceId)>,
    next_frame: u64,
}

impl Default for NullDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl NullDevice {
    /// A device with room for 256 resources of each kind.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// A device whose pools each hold `capacity` resources; a full pool is `OutOfMemory`.
    #[must_use]
    pub fn with_capacity(capacity: u32) -> Self {
        Self {
            resources: Resources::with_capacity(capacity),
            frame: None,
            next_frame: 0,
        }
    }
}

/// `len` zero bytes, or `OutOfMemory` when the allocator refuses (never an abort).
fn zeroed(len: usize) -> Result<Vec<u8>, RenderError> {
    let mut data = Vec::new();
    data.try_reserve_exact(len)
        .map_err(|_| RenderError::OutOfMemory)?;
    data.resize(len, 0);
    Ok(data)
}

fn texel_count(width: u32, height: u32) -> usize {
    usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(usize::MAX)
}

/// Largest mip count a texture of this size can have: floor(log2(longest side)) + 1.
fn max_mips(width: u32, height: u32) -> u32 {
    32 - width.max(height).leading_zeros()
}

fn attribute_bytes(format: VertexFormat) -> u32 {
    match format {
        VertexFormat::Float2 => 8,
        VertexFormat::Float3 => 12,
        VertexFormat::Float4 => 16,
        VertexFormat::Unorm8x4 => 4,
    }
}

/// Round to nearest of a value clamped to 0..1, as a byte. NaN is 0. A search over the 256 bytes
/// keeps this free of float-to-integer casts; the Null device is not a hot path.
fn unorm8(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    let scaled = value.clamp(0.0, 1.0) * 255.0;
    (0..=u8::MAX)
        .find(|byte| f32::from(*byte) + 0.5 > scaled)
        .unwrap_or(u8::MAX)
}

fn clear_texel(format: TextureFormat, color: [f32; 4]) -> [u8; 4] {
    let [r, g, b, a] = color.map(unorm8);
    match format {
        TextureFormat::Bgra8 => [b, g, r, a],
        _ => [r, g, b, a],
    }
}

fn validate_pipeline_layout(desc: &PipelineDesc<'_>) -> Result<(), RenderError> {
    if desc.color_format == TextureFormat::Depth32F {
        return Err(INVALID);
    }
    if desc.attributes.is_empty() {
        return Ok(());
    }
    let inside = |offset: u32, bytes: u32| {
        u64::from(offset) + u64::from(bytes) <= u64::from(desc.vertex_stride)
    };
    let fits = desc
        .attributes
        .iter()
        .all(|a| inside(a.offset, attribute_bytes(a.format)));
    if fits { Ok(()) } else { Err(INVALID) }
}

impl NullDevice {
    fn apply_clear(&mut self, pass: &PassDesc) {
        let (RenderTarget::Texture(target), Some(color)) = (pass.target, pass.clear_color) else {
            return;
        };
        if let Some(record) = self.resources.textures.get_mut(retag(target)) {
            let texel = clear_texel(record.desc.format, color);
            let paint = |chunk: &mut [u8]| chunk.copy_from_slice(&texel);
            record
                .data
                .chunks_exact_mut(BYTES_PER_TEXEL)
                .for_each(paint);
        }
    }

    fn run_clears(&mut self, stream: &CommandStream) {
        let begins = stream
            .commands()
            .flatten()
            .filter_map(|command| match command {
                Command::BeginPass(pass) => Some(pass),
                _ => None,
            });
        for pass in begins {
            self.apply_clear(&pass);
        }
    }
}

impl IRenderDevice for NullDevice {
    fn caps(&self) -> RenderCaps {
        RenderCaps {
            max_texture_size: MAX_TEXTURE_SIZE,
            uniform_buffer_alignment: 256,
            index_u16: true,
            index_u32: true,
            depth_zero_to_one: true,
            device_name: "null",
        }
    }

    fn create_surface(&mut self, desc: SurfaceDesc) -> Result<SurfaceId, RenderError> {
        if desc.native_token == 0 || desc.width == 0 || desc.height == 0 {
            return Err(INVALID);
        }
        insert(
            &mut self.resources.surfaces,
            SurfaceRec {
                width: desc.width,
                height: desc.height,
            },
        )
    }

    fn resize_surface(
        &mut self,
        surface: SurfaceId,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        lookup(&self.resources.surfaces, surface)?;
        if width == 0 || height == 0 {
            return Err(INVALID);
        }
        let record = self
            .resources
            .surfaces
            .get_mut(retag(surface))
            .ok_or(RenderError::InvalidHandle)?;
        record.width = width;
        record.height = height;
        Ok(())
    }

    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.resources.surfaces.remove(retag(surface));
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId, RenderError> {
        if desc.size == 0 {
            return Err(INVALID);
        }
        let data = zeroed(usize::try_from(desc.size).unwrap_or(usize::MAX))?;
        insert(
            &mut self.resources.buffers,
            BufferRec {
                size: desc.size,
                usage: desc.usage,
                data,
            },
        )
    }

    fn create_texture(&mut self, desc: TextureDesc) -> Result<TextureId, RenderError> {
        let size_ok = desc.width != 0
            && desc.height != 0
            && desc.width <= MAX_TEXTURE_SIZE
            && desc.height <= MAX_TEXTURE_SIZE;
        if !size_ok || desc.mip_count == 0 || desc.mip_count > max_mips(desc.width, desc.height) {
            return Err(INVALID);
        }
        let data = zeroed(texel_count(desc.width, desc.height).saturating_mul(BYTES_PER_TEXEL))?;
        insert(&mut self.resources.textures, TextureRec { desc, data })
    }

    fn create_sampler(&mut self, _desc: SamplerDesc) -> Result<SamplerId, RenderError> {
        insert(&mut self.resources.samplers, SamplerRec)
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId, RenderError> {
        if desc.code.is_empty() {
            return Err(RenderError::ShaderRejected);
        }
        if desc.entry.is_empty() {
            return Err(INVALID);
        }
        insert(&mut self.resources.shaders, ShaderRec { stage: desc.stage })
    }

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> Result<PipelineId, RenderError> {
        let vertex = lookup(&self.resources.shaders, desc.vertex_shader)?.stage;
        let fragment = lookup(&self.resources.shaders, desc.fragment_shader)?.stage;
        if vertex != ShaderStage::Vertex || fragment != ShaderStage::Fragment {
            return Err(INVALID);
        }
        validate_pipeline_layout(&desc)?;
        let record = PipelineRec {
            stride: desc.vertex_stride,
            has_attributes: !desc.attributes.is_empty(),
        };
        insert(&mut self.resources.pipelines, record)
    }

    fn write_buffer(
        &mut self,
        buffer: BufferId,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        let record = self
            .resources
            .buffers
            .get_mut(retag(buffer))
            .ok_or(RenderError::InvalidHandle)?;
        let start = usize::try_from(offset).map_err(|_| INVALID)?;
        let end = start
            .checked_add(bytes.len())
            .filter(|end| *end <= record.data.len())
            .ok_or(INVALID)?;
        record.data[start..end].copy_from_slice(bytes);
        Ok(())
    }

    fn write_texture(
        &mut self,
        texture: TextureId,
        region: Rect,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        let record = self
            .resources
            .textures
            .get_mut(retag(texture))
            .ok_or(RenderError::InvalidHandle)?;
        let (width, height) = (record.desc.width, record.desc.height);
        let x = u32::try_from(region.x).map_err(|_| INVALID)?;
        let y = u32::try_from(region.y).map_err(|_| INVALID)?;
        let inside = region.width > 0
            && region.height > 0
            && u64::from(x) + u64::from(region.width) <= u64::from(width)
            && u64::from(y) + u64::from(region.height) <= u64::from(height);
        let row = texel_count(region.width, 1) * BYTES_PER_TEXEL;
        if !inside || bytes.len() != row * texel_count(1, region.height) {
            return Err(INVALID);
        }
        let stride = texel_count(width, 1) * BYTES_PER_TEXEL;
        let first = texel_count(1, y) * stride + texel_count(x, 1) * BYTES_PER_TEXEL;
        for (line, source) in bytes.chunks_exact(row).enumerate() {
            let at = first + line * stride;
            record.data[at..at + row].copy_from_slice(source);
        }
        Ok(())
    }

    fn destroy_buffer(&mut self, buffer: BufferId) {
        self.resources.buffers.remove(retag(buffer));
    }

    fn destroy_texture(&mut self, texture: TextureId) {
        self.resources.textures.remove(retag(texture));
    }

    fn destroy_shader(&mut self, shader: ShaderId) {
        self.resources.shaders.remove(retag(shader));
    }

    fn destroy_sampler(&mut self, sampler: SamplerId) {
        self.resources.samplers.remove(retag(sampler));
    }

    fn destroy_pipeline(&mut self, pipeline: PipelineId) {
        self.resources.pipelines.remove(retag(pipeline));
    }

    fn begin_frame(&mut self, surface: SurfaceId) -> Result<FrameId, RenderError> {
        lookup(&self.resources.surfaces, surface)?;
        if self.frame.is_some() {
            return Err(INVALID);
        }
        let frame = FrameId(self.next_frame);
        self.next_frame += 1;
        self.frame = Some((frame, surface));
        Ok(frame)
    }

    fn submit(
        &mut self,
        frame: FrameId,
        stream: &CommandStream,
    ) -> Result<SubmitStats, RenderError> {
        if self.frame.map(|(current, _)| current) != Some(frame) {
            return Err(RenderError::InvalidHandle);
        }
        let stats = validate(&self.resources, stream)?;
        self.run_clears(stream);
        Ok(stats)
    }

    fn end_frame(&mut self, frame: FrameId) -> Result<(), RenderError> {
        let (current, surface) = self.frame.ok_or(RenderError::InvalidHandle)?;
        if current != frame {
            return Err(RenderError::InvalidHandle);
        }
        self.frame = None;
        lookup(&self.resources.surfaces, surface)
            .map(|_| ())
            .map_err(|_| RenderError::SurfaceLost)
    }

    fn read_back(&mut self, texture: TextureId, out: &mut [u8]) -> Result<(), RenderError> {
        let record = lookup(&self.resources.textures, texture)?;
        if out.len() != record.data.len() {
            return Err(INVALID);
        }
        out.copy_from_slice(&record.data);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
