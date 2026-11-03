//! The Null device's resource tables: fixed-capacity pools of records.

use crate::desc::{BufferUsage, ShaderStage, TextureDesc};
use crate::error::RenderError;
use base::pool::{Handle, Pool};

/// Bytes per texel of every v0 texture format.
pub(super) const BYTES_PER_TEXEL: usize = 4;

pub(super) struct BufferRec {
    pub size: u32,
    pub usage: BufferUsage,
    pub data: Vec<u8>,
}

pub(super) struct TextureRec {
    pub desc: TextureDesc,
    pub data: Vec<u8>,
}

pub(super) struct ShaderRec {
    pub stage: ShaderStage,
}

pub(super) struct PipelineRec {
    pub stride: u32,
    pub has_attributes: bool,
}

pub(super) struct SurfaceRec {
    pub width: u32,
    pub height: u32,
}

/// A sampler holds no state the Null device looks at; the record only occupies a slot.
pub(super) struct SamplerRec;

/// Converts between the public handle types (marker `M`) and the record handles of the pools.
pub(super) fn retag<A, B>(handle: Handle<A>) -> Handle<B> {
    Handle::from_raw(handle.index(), handle.generation())
}

/// Looks a handle up; a stale or foreign one is `InvalidHandle`.
pub(super) fn lookup<R, M>(pool: &Pool<R>, id: Handle<M>) -> Result<&R, RenderError> {
    pool.get(retag(id)).ok_or(RenderError::InvalidHandle)
}

/// Inserts a record; a full pool is `OutOfMemory`.
pub(super) fn insert<R, M>(pool: &mut Pool<R>, record: R) -> Result<Handle<M>, RenderError> {
    pool.insert(record)
        .map(retag)
        .map_err(|_| RenderError::OutOfMemory)
}

pub(super) struct Resources {
    pub buffers: Pool<BufferRec>,
    pub textures: Pool<TextureRec>,
    pub samplers: Pool<SamplerRec>,
    pub shaders: Pool<ShaderRec>,
    pub pipelines: Pool<PipelineRec>,
    pub surfaces: Pool<SurfaceRec>,
}

impl Resources {
    pub fn with_capacity(capacity: u32) -> Self {
        Self {
            buffers: Pool::with_capacity(capacity),
            textures: Pool::with_capacity(capacity),
            samplers: Pool::with_capacity(capacity),
            shaders: Pool::with_capacity(capacity),
            pipelines: Pool::with_capacity(capacity),
            surfaces: Pool::with_capacity(capacity),
        }
    }
}
