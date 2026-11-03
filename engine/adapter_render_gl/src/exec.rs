//! Runs a command stream on the GL context.
//!
//! The stream has been validated whole by the bookkeeping device before it gets here, so every
//! handle is live and every rule holds; a command that still cannot be carried out (a record
//! that is missing, a framebuffer that is not complete) is reported as an error value.

use crate::api::{Api, IndexedDraw};
use crate::context::GlContext;
use crate::mapping::index_type;
use crate::records::Resources;
use render::desc::{IndexType, PassDesc, Rect, RenderTarget};
use render::error::RenderError;
use render::ids::{BufferId, PipelineId, SamplerId, TextureId};
use render::stream::{Command, CommandStream};

const INVALID: RenderError = RenderError::InvalidDescription;

/// The bound pipeline's vertex array and stride.
#[derive(Clone, Copy)]
struct PipelineBinding {
    vao: u32,
    stride: u32,
}

/// The bound index buffer.
#[derive(Clone, Copy)]
struct IndexBinding {
    name: u32,
    offset: usize,
    index_type: IndexType,
}

/// What the stream has bound so far; applied to the vertex array at a draw.
#[derive(Default)]
struct Bound {
    pipeline: Option<PipelineBinding>,
    vertex: Option<(u32, usize)>,
    index: Option<IndexBinding>,
}

/// One run of a stream.
pub struct Executor<'a> {
    ctx: &'a GlContext,
    res: &'a mut Resources,
    backbuffer: (u32, u32),
    bound: Bound,
}

impl<'a> Executor<'a> {
    /// An executor drawing on `ctx`; `backbuffer` is the size of the frame's surface.
    pub fn new(ctx: &'a GlContext, res: &'a mut Resources, backbuffer: (u32, u32)) -> Self {
        Self {
            ctx,
            res,
            backbuffer,
            bound: Bound::default(),
        }
    }

    /// Runs every command of `stream` in order, stopping at the first that fails.
    ///
    /// # Errors
    /// `InvalidDescription` for an undecodable command or a missing record; `Unsupported` when
    /// a framebuffer cannot be completed.
    pub fn run(&mut self, stream: &CommandStream) -> Result<(), RenderError> {
        for command in stream.commands() {
            self.execute(command.map_err(|_| INVALID)?)?;
        }
        Ok(())
    }

    fn api(&self) -> &'a Api {
        self.ctx.api()
    }

    fn execute(&mut self, command: Command) -> Result<(), RenderError> {
        match command {
            Command::BeginPass(pass) => self.begin_pass(&pass),
            Command::SetPipeline(pipeline) => self.set_pipeline(pipeline),
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
            Command::BindVertexBuffer { buffer, offset, .. } => self.bind_vertex(buffer, offset),
            Command::BindIndexBuffer {
                buffer,
                offset,
                index_type,
            } => self.bind_index(buffer, offset, index_type),
            Command::Draw {
                vertices,
                instances,
            } => self.draw(vertices, instances),
            Command::DrawIndexed { indices, instances } => self.draw_indexed(indices, instances),
            // A pass that draws into a texture and a later pass that samples it need no barrier
            // in OpenGL: the driver orders them. The pass boundaries are all there is to do.
            Command::EndPass | Command::Barrier { .. } => Ok(()),
        }
    }

    fn begin_pass(&mut self, pass: &PassDesc) -> Result<(), RenderError> {
        let (framebuffer, width, height) = self.target(pass.target)?;
        let api = self.api();
        api.bind_draw_framebuffer(framebuffer);
        self.ctx
            .gl()
            .set_viewport(0, 0, to_i32(width), to_i32(height));
        if let Some(rgba) = pass.clear_color {
            api.clear_colour(framebuffer, rgba);
        }
        if let Some(depth) = pass.clear_depth {
            api.clear_depth(framebuffer, depth);
        }
        Ok(())
    }

    /// The framebuffer name (0 is the window's) and size of a pass target; a texture gets its
    /// framebuffer the first time it is drawn into.
    fn target(&mut self, target: RenderTarget) -> Result<(u32, u32, u32), RenderError> {
        let api = self.api();
        let RenderTarget::Texture(id) = target else {
            return Ok((0, self.backbuffer.0, self.backbuffer.1));
        };
        let record = self.res.textures.get_mut(id).ok_or(INVALID)?;
        let framebuffer = record.framebuffer(api)?;
        Ok((framebuffer, record.width, record.height))
    }

    fn set_viewport(&self, rect: Rect) {
        let gl = self.ctx.gl();
        gl.set_viewport(rect.x, rect.y, to_i32(rect.width), to_i32(rect.height));
    }

    fn set_pipeline(&mut self, pipeline: PipelineId) -> Result<(), RenderError> {
        let record = self.res.pipelines.get(pipeline).ok_or(INVALID)?;
        let api = self.api();
        api.use_program(record.program);
        api.bind_vertex_array(record.vao);
        api.set_blend(record.blend);
        api.set_depth(record.depth_test, record.depth_write);
        self.bound.pipeline = Some(PipelineBinding {
            vao: record.vao,
            stride: record.stride,
        });
        Ok(())
    }

    fn bind_uniforms(&self, slot: u32, buffer: BufferId) -> Result<(), RenderError> {
        let record = self.res.buffers.get(buffer).ok_or(INVALID)?;
        self.api().bind_uniform_buffer(slot, record.name);
        Ok(())
    }

    fn bind_texture(
        &self,
        slot: u32,
        texture: TextureId,
        sampler: SamplerId,
    ) -> Result<(), RenderError> {
        let texture = self.res.textures.get(texture).ok_or(INVALID)?;
        let sampler = self.res.samplers.get(sampler).ok_or(INVALID)?;
        self.api()
            .bind_texture_and_sampler(slot, texture.name, sampler.name);
        Ok(())
    }

    fn bind_vertex(&mut self, buffer: BufferId, offset: u32) -> Result<(), RenderError> {
        let record = self.res.buffers.get(buffer).ok_or(INVALID)?;
        let offset = usize::try_from(offset).map_err(|_| INVALID)?;
        self.bound.vertex = Some((record.name, offset));
        Ok(())
    }

    fn bind_index(
        &mut self,
        buffer: BufferId,
        offset: u32,
        index_type: IndexType,
    ) -> Result<(), RenderError> {
        let record = self.res.buffers.get(buffer).ok_or(INVALID)?;
        self.bound.index = Some(IndexBinding {
            name: record.name,
            offset: usize::try_from(offset).map_err(|_| INVALID)?,
            index_type,
        });
        Ok(())
    }

    /// Points the bound pipeline's vertex array at the bound vertex buffer. A draw without a
    /// vertex buffer is valid (the shader makes its vertices from `gl_VertexID`, and the
    /// validator accepts it), so then there is nothing to attach.
    fn attach_vertices(&self) -> Result<PipelineBinding, RenderError> {
        let pipeline = self.bound.pipeline.ok_or(INVALID)?;
        if let Some((buffer, offset)) = self.bound.vertex {
            self.api()
                .vertex_array_vertex_buffer(pipeline.vao, buffer, offset, pipeline.stride);
        }
        Ok(pipeline)
    }

    fn draw(&self, vertices: u32, instances: u32) -> Result<(), RenderError> {
        self.attach_vertices()?;
        self.api().draw_arrays(to_i32(vertices), to_i32(instances));
        Ok(())
    }

    fn draw_indexed(&self, indices: u32, instances: u32) -> Result<(), RenderError> {
        let pipeline = self.attach_vertices()?;
        let index = self.bound.index.ok_or(INVALID)?;
        let api = self.api();
        api.vertex_array_element_buffer(pipeline.vao, index.name);
        api.draw_indexed(IndexedDraw {
            count: to_i32(indices),
            index_type: index_type(index.index_type),
            offset: index.offset,
            instances: to_i32(instances),
        });
        Ok(())
    }
}

/// A count as the `i32` OpenGL takes; saturates (GL then reports the error).
fn to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::to_i32;

    #[test]
    fn counts_saturate_at_the_largest_i32() {
        assert_eq!(to_i32(3), 3);
        assert_eq!(to_i32(u32::MAX), i32::MAX);
    }
}
