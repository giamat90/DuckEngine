//! The POD command stream: backend-neutral, linear, little-endian bytes.
//!
//! Layout: a 4-byte version word ([`STREAM_VERSION`]), then commands. A command is a 16-bit opcode,
//! a 16-bit payload length, then a fixed payload per opcode (numbers little-endian, a handle as its
//! 32-bit index then its 32-bit generation). A `begin_pass` payload of 38 bytes (instead of 30)
//! ends with the handle of a depth attachment; a pass without depth keeps the 30 bytes it always
//! had, so streams recorded before depth existed are unchanged. Decoding is a loop over the buffer
//! ([`Commands`]), no virtual call per command. The buffer keeps its capacity across [`CommandStream::clear`], so
//! refilling a frame to the same size allocates nothing. Decoding accepts only the canonical form
//! (unused fields zero), so decode then encode reproduces the bytes. Opcode 12 and up are reserved.

use crate::desc::{IndexType, PassDesc, Rect, RenderTarget, ResourceState};
use crate::ids::{BufferId, PipelineId, SamplerId, TextureId};
use base::pool::Handle;
use core::fmt;

/// Version word at the start of every stream; v0 of the port is `1`.
pub const STREAM_VERSION: u32 = 1;

const HEADER_LEN: usize = 4;
const COMMAND_HEADER_LEN: usize = 4;

const OP_BEGIN_PASS: u16 = 1;
const OP_END_PASS: u16 = 2;
const OP_SET_PIPELINE: u16 = 3;
const OP_SET_VIEWPORT: u16 = 4;
const OP_BIND_UNIFORMS: u16 = 5;
const OP_BIND_TEXTURE: u16 = 6;
const OP_BIND_VERTEX_BUFFER: u16 = 7;
const OP_BIND_INDEX_BUFFER: u16 = 8;
const OP_DRAW: u16 = 9;
const OP_DRAW_INDEXED: u16 = 10;
const OP_BARRIER: u16 = 11;

const TARGET_BACKBUFFER: u8 = 0;
const TARGET_TEXTURE: u8 = 1;
const FLAG_COLOR: u8 = 0b01;
const FLAG_DEPTH: u8 = 0b10;
const INDEX_U16: u32 = 0;
const INDEX_U32: u32 = 1;

/// One decoded command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Begin a render pass.
    BeginPass(PassDesc),
    /// End the current pass.
    EndPass,
    /// Bind a graphics pipeline.
    SetPipeline(PipelineId),
    /// Set the viewport.
    SetViewport(Rect),
    /// Bind a uniform buffer to a slot.
    BindUniforms {
        /// Uniform slot.
        slot: u32,
        /// The buffer.
        buffer: BufferId,
    },
    /// Bind a texture and sampler to a slot.
    BindTexture {
        /// Texture slot.
        slot: u32,
        /// The texture.
        texture: TextureId,
        /// The sampler.
        sampler: SamplerId,
    },
    /// Bind a vertex buffer to a slot.
    BindVertexBuffer {
        /// Vertex buffer slot.
        slot: u32,
        /// The buffer.
        buffer: BufferId,
        /// Byte offset of the first vertex.
        offset: u32,
    },
    /// Bind the index buffer.
    BindIndexBuffer {
        /// The buffer.
        buffer: BufferId,
        /// Byte offset of the first index.
        offset: u32,
        /// Width of the indices.
        index_type: IndexType,
    },
    /// Draw vertices.
    Draw {
        /// Vertex count.
        vertices: u32,
        /// Instance count.
        instances: u32,
    },
    /// Move a resource from one state to another, between passes.
    Barrier {
        /// The resource.
        target: RenderTarget,
        /// State before.
        from: ResourceState,
        /// State after.
        to: ResourceState,
    },
    /// Draw indexed.
    DrawIndexed {
        /// Index count.
        indices: u32,
        /// Instance count.
        instances: u32,
    },
}

/// Why a byte buffer is not a valid stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// The version word is not [`STREAM_VERSION`].
    BadVersion(u32),
    /// The buffer ends inside the header or a command.
    Truncated,
    /// The opcode is not defined (or is reserved).
    UnknownOpcode(u16),
    /// The payload length is not the one the opcode requires.
    BadLength {
        /// The opcode concerned.
        opcode: u16,
    },
    /// A field holds a value outside its enum.
    BadValue,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadVersion(v) => write!(f, "unsupported stream version {v}"),
            Self::Truncated => f.write_str("stream is truncated"),
            Self::UnknownOpcode(op) => write!(f, "unknown opcode {op}"),
            Self::BadLength { opcode } => write!(f, "wrong payload length for opcode {opcode}"),
            Self::BadValue => f.write_str("field value out of range"),
        }
    }
}

impl std::error::Error for StreamError {}

/// A recorded frame of commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandStream {
    bytes: Vec<u8>,
}

impl Default for CommandStream {
    fn default() -> Self {
        Self::new()
    }
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_handle<T>(out: &mut Vec<u8>, handle: Handle<T>) {
    put_u32(out, handle.index());
    put_u32(out, handle.generation());
}

fn put_pass(out: &mut Vec<u8>, pass: &PassDesc) {
    out.push(match pass.target {
        RenderTarget::Backbuffer => TARGET_BACKBUFFER,
        RenderTarget::Texture(_) => TARGET_TEXTURE,
    });
    let mut flags = 0;
    if pass.clear_color.is_some() {
        flags |= FLAG_COLOR;
    }
    if pass.clear_depth.is_some() {
        flags |= FLAG_DEPTH;
    }
    out.push(flags);
    for component in pass.clear_color.unwrap_or([0.0; 4]) {
        put_u32(out, component.to_bits());
    }
    put_u32(out, pass.clear_depth.unwrap_or(0.0).to_bits());
    match pass.target {
        RenderTarget::Backbuffer => out.extend_from_slice(&[0; 8]),
        RenderTarget::Texture(texture) => put_handle(out, texture),
    }
    if let Some(depth) = pass.depth {
        put_handle(out, depth);
    }
}

fn state_tag(state: ResourceState) -> u32 {
    match state {
        ResourceState::Undefined => 0,
        ResourceState::ColorTarget => 1,
        ResourceState::ShaderRead => 2,
        ResourceState::Present => 3,
        ResourceState::DepthTarget => 4,
    }
}

fn put_barrier(out: &mut Vec<u8>, target: RenderTarget, from: ResourceState, to: ResourceState) {
    match target {
        RenderTarget::Backbuffer => {
            put_u32(out, u32::from(TARGET_BACKBUFFER));
            out.extend_from_slice(&[0; 8]);
        }
        RenderTarget::Texture(texture) => {
            put_u32(out, u32::from(TARGET_TEXTURE));
            put_handle(out, texture);
        }
    }
    put_u32(out, state_tag(from));
    put_u32(out, state_tag(to));
}

fn index_tag(index_type: IndexType) -> u32 {
    match index_type {
        IndexType::U16 => INDEX_U16,
        IndexType::U32 => INDEX_U32,
    }
}

impl CommandStream {
    /// An empty stream (header only).
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// An empty stream whose buffer holds `bytes` before it grows.
    #[must_use]
    pub fn with_capacity(bytes: usize) -> Self {
        let mut buffer = Vec::with_capacity(bytes.max(HEADER_LEN));
        put_u32(&mut buffer, STREAM_VERSION);
        Self { bytes: buffer }
    }

    /// Adopts a recorded buffer (replay, tools) after checking it end to end.
    ///
    /// # Errors
    /// The first [`StreamError`] found in the header or any command.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StreamError> {
        let stream = Self {
            bytes: bytes.to_vec(),
        };
        for command in stream.commands() {
            command?;
        }
        Ok(stream)
    }

    /// Wraps bytes without checking them, to build malformed streams in tests.
    #[cfg(test)]
    pub(crate) fn from_bytes_unchecked(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Drops all commands and keeps the buffer's capacity.
    pub fn clear(&mut self) {
        self.bytes.clear();
        put_u32(&mut self.bytes, STREAM_VERSION);
    }

    /// Whether the stream holds no command.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.len() == HEADER_LEN
    }

    /// Bytes the buffer can hold without growing.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// The raw bytes, header included.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Iterates the commands; the first error ends the iteration.
    #[must_use]
    pub fn commands(&self) -> Commands<'_> {
        Commands::new(&self.bytes)
    }

    fn record(&mut self, opcode: u16, write: impl FnOnce(&mut Vec<u8>)) {
        self.bytes.extend_from_slice(&opcode.to_le_bytes());
        let length_at = self.bytes.len();
        self.bytes.extend_from_slice(&[0, 0]);
        write(&mut self.bytes);
        let payload = self.bytes.len() - length_at - 2;
        // Every payload is at most 38 bytes (see the `largest_payload_fits_the_length_field` test).
        let length = u16::try_from(payload).unwrap_or(u16::MAX);
        self.bytes[length_at..length_at + 2].copy_from_slice(&length.to_le_bytes());
    }

    /// Records the start of a render pass.
    pub fn begin_pass(&mut self, pass: PassDesc) {
        self.record(OP_BEGIN_PASS, |out| put_pass(out, &pass));
    }

    /// Records the end of the current pass.
    pub fn end_pass(&mut self) {
        self.record(OP_END_PASS, |_| {});
    }

    /// Records a pipeline bind.
    pub fn set_pipeline(&mut self, pipeline: PipelineId) {
        self.record(OP_SET_PIPELINE, |out| put_handle(out, pipeline));
    }

    /// Records a viewport change.
    pub fn set_viewport(&mut self, rect: Rect) {
        self.record(OP_SET_VIEWPORT, |out| {
            put_u32(out, rect.x.cast_unsigned());
            put_u32(out, rect.y.cast_unsigned());
            put_u32(out, rect.width);
            put_u32(out, rect.height);
        });
    }

    /// Records a uniform buffer bind.
    pub fn bind_uniforms(&mut self, slot: u32, buffer: BufferId) {
        self.record(OP_BIND_UNIFORMS, |out| {
            put_u32(out, slot);
            put_handle(out, buffer);
        });
    }

    /// Records a texture and sampler bind.
    pub fn bind_texture(&mut self, slot: u32, texture: TextureId, sampler: SamplerId) {
        self.record(OP_BIND_TEXTURE, |out| {
            put_u32(out, slot);
            put_handle(out, texture);
            put_handle(out, sampler);
        });
    }

    /// Records a vertex buffer bind.
    pub fn bind_vertex_buffer(&mut self, slot: u32, buffer: BufferId, offset: u32) {
        self.record(OP_BIND_VERTEX_BUFFER, |out| {
            put_u32(out, slot);
            put_handle(out, buffer);
            put_u32(out, offset);
        });
    }

    /// Records an index buffer bind.
    pub fn bind_index_buffer(&mut self, buffer: BufferId, offset: u32, index_type: IndexType) {
        self.record(OP_BIND_INDEX_BUFFER, |out| {
            put_handle(out, buffer);
            put_u32(out, offset);
            put_u32(out, index_tag(index_type));
        });
    }

    /// Records a state change of a resource (between passes).
    pub fn barrier(&mut self, target: RenderTarget, from: ResourceState, to: ResourceState) {
        self.record(OP_BARRIER, |out| put_barrier(out, target, from, to));
    }

    /// Records a draw.
    pub fn draw(&mut self, vertices: u32, instances: u32) {
        self.record(OP_DRAW, |out| {
            put_u32(out, vertices);
            put_u32(out, instances);
        });
    }

    /// Records an indexed draw.
    pub fn draw_indexed(&mut self, indices: u32, instances: u32) {
        self.record(OP_DRAW_INDEXED, |out| {
            put_u32(out, indices);
            put_u32(out, instances);
        });
    }
}

/// Iterator over the commands of a stream.
#[derive(Clone, Debug)]
pub struct Commands<'a> {
    rest: &'a [u8],
    pending: Option<StreamError>,
}

impl<'a> Commands<'a> {
    /// Iterates the commands of a raw buffer (header included), without copying it.
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        let Some((version, rest)) = bytes.split_first_chunk::<HEADER_LEN>() else {
            return Self {
                rest: &[],
                pending: Some(StreamError::Truncated),
            };
        };
        let version = u32::from_le_bytes(*version);
        let pending = (version != STREAM_VERSION).then_some(StreamError::BadVersion(version));
        Self { rest, pending }
    }

    fn fail(&mut self, error: StreamError) -> Result<Command, StreamError> {
        self.rest = &[];
        Err(error)
    }
}

impl Iterator for Commands<'_> {
    type Item = Result<Command, StreamError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.pending.take() {
            return Some(self.fail(error));
        }
        if self.rest.is_empty() {
            return None;
        }
        let Some((head, after)) = self.rest.split_first_chunk::<COMMAND_HEADER_LEN>() else {
            return Some(self.fail(StreamError::Truncated));
        };
        let opcode = u16::from_le_bytes([head[0], head[1]]);
        let length = usize::from(u16::from_le_bytes([head[2], head[3]]));
        if after.len() < length {
            return Some(self.fail(StreamError::Truncated));
        }
        let (payload, rest) = after.split_at(length);
        match decode(opcode, payload) {
            Ok(command) => {
                self.rest = rest;
                Some(Ok(command))
            }
            Err(error) => Some(self.fail(error)),
        }
    }
}

/// Cursor over one payload.
struct Reader<'a> {
    bytes: &'a [u8],
    opcode: u16,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], StreamError> {
        let (chunk, rest) = self
            .bytes
            .split_first_chunk::<N>()
            .ok_or(StreamError::BadLength {
                opcode: self.opcode,
            })?;
        self.bytes = rest;
        Ok(*chunk)
    }

    fn u8(&mut self) -> Result<u8, StreamError> {
        Ok(self.take::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, StreamError> {
        Ok(u32::from_le_bytes(self.take::<4>()?))
    }

    fn f32(&mut self) -> Result<f32, StreamError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn handle<T>(&mut self) -> Result<Handle<T>, StreamError> {
        let index = self.u32()?;
        let generation = self.u32()?;
        Ok(Handle::from_raw(index, generation))
    }

    fn finish(self) -> Result<(), StreamError> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(StreamError::BadLength {
                opcode: self.opcode,
            })
        }
    }
}

fn decode_pass(r: &mut Reader<'_>) -> Result<PassDesc, StreamError> {
    let target_tag = r.u8()?;
    let flags = r.u8()?;
    let color = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
    let depth = r.f32()?;
    let texture = r.handle()?;
    let depth_attachment = if r.bytes.is_empty() {
        None
    } else {
        Some(r.handle()?)
    };
    let target = match target_tag {
        TARGET_BACKBUFFER => RenderTarget::Backbuffer,
        TARGET_TEXTURE => RenderTarget::Texture(texture),
        _ => return Err(StreamError::BadValue),
    };
    // Canonical form: unused fields are zero, so decode then encode gives the same bytes.
    let unused_color = flags & FLAG_COLOR == 0 && color.iter().any(|c| c.to_bits() != 0);
    let unused_depth = flags & FLAG_DEPTH == 0 && depth.to_bits() != 0;
    let unused_texture =
        target == RenderTarget::Backbuffer && (texture.index(), texture.generation()) != (0, 0);
    if flags & !(FLAG_COLOR | FLAG_DEPTH) != 0 || unused_color || unused_depth || unused_texture {
        return Err(StreamError::BadValue);
    }
    Ok(PassDesc {
        target,
        clear_color: (flags & FLAG_COLOR != 0).then_some(color),
        clear_depth: (flags & FLAG_DEPTH != 0).then_some(depth),
        depth: depth_attachment,
    })
}

fn decode_state(r: &mut Reader<'_>) -> Result<ResourceState, StreamError> {
    match r.u32()? {
        0 => Ok(ResourceState::Undefined),
        1 => Ok(ResourceState::ColorTarget),
        2 => Ok(ResourceState::ShaderRead),
        3 => Ok(ResourceState::Present),
        4 => Ok(ResourceState::DepthTarget),
        _ => Err(StreamError::BadValue),
    }
}

fn decode_barrier(r: &mut Reader<'_>) -> Result<Command, StreamError> {
    let tag = r.u32()?;
    let texture = r.handle()?;
    let target = match u8::try_from(tag) {
        Ok(TARGET_BACKBUFFER) if (texture.index(), texture.generation()) == (0, 0) => {
            RenderTarget::Backbuffer
        }
        Ok(TARGET_TEXTURE) => RenderTarget::Texture(texture),
        _ => return Err(StreamError::BadValue),
    };
    Ok(Command::Barrier {
        target,
        from: decode_state(r)?,
        to: decode_state(r)?,
    })
}

fn decode_index_type(r: &mut Reader<'_>) -> Result<IndexType, StreamError> {
    match r.u32()? {
        INDEX_U16 => Ok(IndexType::U16),
        INDEX_U32 => Ok(IndexType::U32),
        _ => Err(StreamError::BadValue),
    }
}

fn decode_binding(opcode: u16, r: &mut Reader<'_>) -> Result<Command, StreamError> {
    Ok(match opcode {
        OP_BIND_UNIFORMS => Command::BindUniforms {
            slot: r.u32()?,
            buffer: r.handle()?,
        },
        OP_BIND_TEXTURE => Command::BindTexture {
            slot: r.u32()?,
            texture: r.handle()?,
            sampler: r.handle()?,
        },
        OP_BIND_VERTEX_BUFFER => Command::BindVertexBuffer {
            slot: r.u32()?,
            buffer: r.handle()?,
            offset: r.u32()?,
        },
        _ => Command::BindIndexBuffer {
            buffer: r.handle()?,
            offset: r.u32()?,
            index_type: decode_index_type(r)?,
        },
    })
}

fn decode(opcode: u16, payload: &[u8]) -> Result<Command, StreamError> {
    let mut r = Reader {
        bytes: payload,
        opcode,
    };
    let command = match opcode {
        OP_BEGIN_PASS => Command::BeginPass(decode_pass(&mut r)?),
        OP_END_PASS => Command::EndPass,
        OP_SET_PIPELINE => Command::SetPipeline(r.handle()?),
        OP_SET_VIEWPORT => Command::SetViewport(Rect {
            x: r.u32()?.cast_signed(),
            y: r.u32()?.cast_signed(),
            width: r.u32()?,
            height: r.u32()?,
        }),
        OP_BIND_UNIFORMS | OP_BIND_TEXTURE | OP_BIND_VERTEX_BUFFER | OP_BIND_INDEX_BUFFER => {
            decode_binding(opcode, &mut r)?
        }
        OP_DRAW => Command::Draw {
            vertices: r.u32()?,
            instances: r.u32()?,
        },
        OP_BARRIER => decode_barrier(&mut r)?,
        OP_DRAW_INDEXED => Command::DrawIndexed {
            indices: r.u32()?,
            instances: r.u32()?,
        },
        _ => return Err(StreamError::UnknownOpcode(opcode)),
    };
    r.finish()?;
    Ok(command)
}

#[cfg(test)]
mod tests;
