//! What the GL side keeps for each resource handle: the names of the GL objects and the few
//! facts a draw needs. The bookkeeping device that issued the handle holds everything else.

use crate::api::{Api, Attachment};
use crate::mapping::TextureLayout;
use crate::table::Table;
use render::error::RenderError;

/// A resource record that owns GL objects and can delete them.
pub trait Record {
    /// Deletes the GL objects the record names. The context must be current.
    fn delete(self, api: &Api);
}

pub struct BufferRec {
    pub name: u32,
}

pub struct TextureRec {
    pub name: u32,
    pub width: u32,
    pub height: u32,
    pub layout: TextureLayout,
    /// A depth texture is drawn into as the depth attachment, a colour one as colour.
    pub depth: bool,
    /// Made when a pass first draws into the texture.
    pub framebuffer: Option<u32>,
}

pub struct SamplerRec {
    pub name: u32,
}

pub struct ShaderRec {
    pub name: u32,
}

pub struct PipelineRec {
    pub program: u32,
    pub vao: u32,
    pub stride: u32,
    pub blend: bool,
    pub depth_test: bool,
    pub depth_write: bool,
}

/// A surface is the window's own framebuffer: only its size is kept.
pub struct SurfaceRec {
    pub width: u32,
    pub height: u32,
}

impl TextureRec {
    /// The framebuffer that draws into the texture, made on first use.
    ///
    /// # Errors
    /// `Unsupported` when the driver does not complete a framebuffer for this texture.
    pub fn framebuffer(&mut self, api: &Api) -> Result<u32, RenderError> {
        if let Some(framebuffer) = self.framebuffer {
            return Ok(framebuffer);
        }
        let attachment = if self.depth {
            Attachment::Depth
        } else {
            Attachment::Color
        };
        let made = api
            .framebuffer_for(self.name, attachment)
            .ok_or(RenderError::Unsupported)?;
        self.framebuffer = Some(made);
        Ok(made)
    }
}

impl Record for BufferRec {
    fn delete(self, api: &Api) {
        api.delete_buffer(self.name);
    }
}

impl Record for TextureRec {
    fn delete(self, api: &Api) {
        if let Some(framebuffer) = self.framebuffer {
            api.delete_framebuffer(framebuffer);
        }
        api.delete_texture(self.name);
    }
}

impl Record for SamplerRec {
    fn delete(self, api: &Api) {
        api.delete_sampler(self.name);
    }
}

impl Record for ShaderRec {
    fn delete(self, api: &Api) {
        api.delete_shader(self.name);
    }
}

impl Record for PipelineRec {
    fn delete(self, api: &Api) {
        api.delete_vertex_array(self.vao);
        api.delete_program(self.program);
    }
}

impl Record for SurfaceRec {
    fn delete(self, _api: &Api) {}
}

/// Every table of the device.
pub struct Resources {
    pub buffers: Table<BufferRec>,
    pub textures: Table<TextureRec>,
    pub samplers: Table<SamplerRec>,
    pub shaders: Table<ShaderRec>,
    pub pipelines: Table<PipelineRec>,
    pub surfaces: Table<SurfaceRec>,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            buffers: Table::new(),
            textures: Table::new(),
            samplers: Table::new(),
            shaders: Table::new(),
            pipelines: Table::new(),
            surfaces: Table::new(),
        }
    }

    /// Deletes every GL object the tables name and empties them.
    pub fn delete_all(&mut self, api: &Api) {
        self.pipelines.drain().for_each(|record| record.delete(api));
        self.shaders.drain().for_each(|record| record.delete(api));
        self.samplers.drain().for_each(|record| record.delete(api));
        self.textures.drain().for_each(|record| record.delete(api));
        self.buffers.drain().for_each(|record| record.delete(api));
        self.surfaces.drain().for_each(|record| record.delete(api));
    }
}
