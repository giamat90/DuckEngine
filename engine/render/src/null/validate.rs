//! Validation of a command stream against the device's resources, with no side effects.
//!
//! Rules: every handle is live; passes do not nest and every command except `begin_pass` sits in
//! one; the pass is closed at the end; a draw needs a pipeline (and a vertex buffer when the
//! pipeline has attributes); an indexed draw needs an index buffer; ranges stay inside the
//! buffers; slots are in range; buffers are bound for their usage. Bindings reset at each pass.

use super::MAX_BINDING_SLOTS;
use super::resources::{Resources, lookup};
use crate::desc::{BufferUsage, IndexType, PassDesc, RenderTarget, SubmitStats, TextureFormat};
use crate::error::RenderError;
use crate::ids::{BufferId, PipelineId};
use crate::stream::{Command, CommandStream};

const INVALID: RenderError = RenderError::InvalidDescription;

#[derive(Clone, Copy)]
struct Bound {
    size: u32,
    offset: u32,
}

#[derive(Default)]
struct PassState {
    open: bool,
    /// Stride and whether the pipeline reads vertex attributes.
    pipeline: Option<(u32, bool)>,
    vertex: Option<Bound>,
    index: Option<(Bound, IndexType)>,
}

/// Checks the whole stream; the counts it returns are what `submit` reports.
pub(super) fn validate(
    res: &Resources,
    stream: &CommandStream,
) -> Result<SubmitStats, RenderError> {
    let mut state = PassState::default();
    let mut totals = SubmitStats::default();
    for command in stream.commands() {
        let command = command.map_err(|_| INVALID)?;
        totals.commands += 1;
        if state.step(res, &command)? {
            totals.draws += 1;
        }
    }
    if state.open { Err(INVALID) } else { Ok(totals) }
}

fn index_bytes(index_type: IndexType) -> u64 {
    match index_type {
        IndexType::U16 => 2,
        IndexType::U32 => 4,
    }
}

fn fits(bound: Bound, bytes: u64) -> bool {
    u64::from(bound.offset) + bytes <= u64::from(bound.size)
}

fn bound_buffer(
    res: &Resources,
    id: BufferId,
    usage: BufferUsage,
    offset: u32,
) -> Result<Bound, RenderError> {
    let record = lookup(&res.buffers, id)?;
    if record.usage != usage || offset > record.size {
        return Err(INVALID);
    }
    Ok(Bound {
        size: record.size,
        offset,
    })
}

fn check_index_alignment(offset: u32, index_type: IndexType) -> Result<(), RenderError> {
    if u64::from(offset) % index_bytes(index_type) == 0 {
        Ok(())
    } else {
        Err(INVALID)
    }
}

fn check_slot(slot: u32) -> Result<(), RenderError> {
    if slot < MAX_BINDING_SLOTS {
        Ok(())
    } else {
        Err(INVALID)
    }
}

/// Version 0 reads vertex data from slot 0 only.
fn check_vertex_slot(slot: u32) -> Result<(), RenderError> {
    if slot == 0 { Ok(()) } else { Err(INVALID) }
}

fn check_target(res: &Resources, pass: &PassDesc) -> Result<(), RenderError> {
    let RenderTarget::Texture(texture) = pass.target else {
        return Ok(());
    };
    let record = lookup(&res.textures, texture)?;
    let colour_into_depth =
        pass.clear_color.is_some() && record.desc.format == TextureFormat::Depth32F;
    if colour_into_depth {
        Err(INVALID)
    } else {
        Ok(())
    }
}

impl PassState {
    /// Applies one command; returns whether it was a draw.
    fn step(&mut self, res: &Resources, command: &Command) -> Result<bool, RenderError> {
        if let Command::BeginPass(pass) = command {
            self.begin(res, pass)?;
            return Ok(false);
        }
        if !self.open {
            return Err(INVALID);
        }
        match command {
            Command::EndPass => self.open = false,
            Command::SetPipeline(pipeline) => self.set_pipeline(res, *pipeline)?,
            Command::Draw { vertices, .. } => {
                self.check_draw(u64::from(*vertices))?;
                return Ok(true);
            }
            Command::DrawIndexed { indices, .. } => {
                self.check_indexed(u64::from(*indices))?;
                return Ok(true);
            }
            other => self.bind(res, other)?,
        }
        Ok(false)
    }

    fn begin(&mut self, res: &Resources, pass: &PassDesc) -> Result<(), RenderError> {
        if self.open {
            return Err(INVALID);
        }
        check_target(res, pass)?;
        *self = Self {
            open: true,
            ..Self::default()
        };
        Ok(())
    }

    fn set_pipeline(&mut self, res: &Resources, pipeline: PipelineId) -> Result<(), RenderError> {
        let record = lookup(&res.pipelines, pipeline)?;
        self.pipeline = Some((record.stride, record.has_attributes));
        Ok(())
    }

    fn bind(&mut self, res: &Resources, command: &Command) -> Result<(), RenderError> {
        match *command {
            Command::BindUniforms { slot, buffer } => {
                check_slot(slot)?;
                bound_buffer(res, buffer, BufferUsage::Uniform, 0).map(|_| ())
            }
            Command::BindTexture {
                slot,
                texture,
                sampler,
            } => {
                check_slot(slot)?;
                lookup(&res.textures, texture)?;
                lookup(&res.samplers, sampler).map(|_| ())
            }
            Command::BindVertexBuffer {
                slot,
                buffer,
                offset,
            } => {
                check_vertex_slot(slot)?;
                self.vertex = Some(bound_buffer(res, buffer, BufferUsage::Vertex, offset)?);
                Ok(())
            }
            Command::BindIndexBuffer {
                buffer,
                offset,
                index_type,
            } => {
                check_index_alignment(offset, index_type)?;
                self.index = Some((
                    bound_buffer(res, buffer, BufferUsage::Index, offset)?,
                    index_type,
                ));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn check_draw(&self, vertices: u64) -> Result<(), RenderError> {
        let (stride, has_attributes) = self.pipeline.ok_or(INVALID)?;
        match self.vertex {
            Some(bound) if !fits(bound, vertices * u64::from(stride)) => Err(INVALID),
            None if has_attributes => Err(INVALID),
            _ => Ok(()),
        }
    }

    fn check_indexed(&self, indices: u64) -> Result<(), RenderError> {
        self.check_draw(0)?;
        let (bound, index_type) = self.index.ok_or(INVALID)?;
        if fits(bound, indices * index_bytes(index_type)) {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
}
