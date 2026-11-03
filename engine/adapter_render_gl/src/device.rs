//! `GlDevice`: the render device port on OpenGL 4.5 (Direct State Access).
//!
//! A bookkeeping [`NullDevice`] sits inside: it issues the handles and checks every call and
//! every stream by the rules the Null device defines, so this device accepts and refuses exactly
//! what the port says and the rules exist once. Only after it agrees does the GL side act, and
//! when GL then fails the handle is given back. Whatever GL needs to remember about a handle
//! lives in [`Resources`](crate::records::Resources).
//!
//! A shader module's blob is GLSL 450 source text (UTF-8) and its entry point is `main`; the
//! shader tool of card 3-11 will produce such blobs from the engine's shader language.
//! Texture rows are in OpenGL order: row 0 is the bottom row of the picture.
//!
//! Known limits of version 0:
//! - Index values live on the GPU, so an indexed draw is checked against the vertex buffer's
//!   size only for the index range, not for the values inside. An index past the end of the
//!   vertex buffer reads outside it: OpenGL leaves that undefined unless the context was made
//!   with robust buffer access, which this adapter does not request yet (it needs a context
//!   creation flag in `wgl`/`glx` and a capability in `RenderCaps`). A caller must keep its
//!   indices inside the buffer it binds.
//! - The size of the backbuffer, and so the default viewport of a pass on it, is the size given
//!   to `create_surface` or the last `resize_surface`. A caller must call `resize_surface` when
//!   the window's framebuffer changes size.
//! - Destroying a buffer does not detach it from the vertex arrays it was attached to; a draw
//!   re-attaches the buffer it binds, and a pipeline without attributes reads none.

use crate::api::{
    AttribFormat, MAX_TEXTURE_SIZE, Region, TextureStorage, Transfer,
    UNIFORM_BUFFER_OFFSET_ALIGNMENT,
};
use crate::context::GlContext;
use crate::exec::Executor;
use crate::mapping::{
    ENTRY, TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER, TEXTURE_WRAP_S, TEXTURE_WRAP_T,
    attribute_layout, error_from_gl, mag_filter, min_filter, shader_kind, texture_layout,
    wrap_mode,
};
use crate::records::{
    BufferRec, PipelineRec, Record, Resources, SamplerRec, ShaderRec, SurfaceRec, TextureRec,
};
use crate::table::Table;
use base::pool::Handle;
use platform::error::PlatformError;
use platform::window::NativeToken;
use render::caps::RenderCaps;
use render::desc::{
    BlendMode, BufferDesc, PipelineDesc, Rect, SamplerDesc, ShaderDesc, SubmitStats, SurfaceDesc,
    TextureDesc, TextureFormat, VertexAttribute,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, FrameId, PipelineId, SamplerId, ShaderId, SurfaceId, TextureId};
use render::null::NullDevice;
use render::stream::CommandStream;

const INVALID: RenderError = RenderError::InvalidDescription;
/// Resources of each kind the device can hold (the bookkeeping device keeps no bytes, so this
/// costs a few bytes per slot).
const CAPACITY: u32 = 1024;
/// Error flags drained after a call: GL keeps one flag per kind, so a handful covers them.
const MAX_ERROR_FLAGS: usize = 8;
const DEVICE_NAME: &str = "OpenGL 4.5";

/// The render device on an OpenGL 4.5 core context.
///
/// Made from a window's native token; the context stays current on the creating thread (the
/// device is not `Send`). Version 0 serves one window: only a surface of that window's token can
/// be created. Drop it before the window is destroyed.
pub struct GlDevice {
    shadow: NullDevice,
    res: Resources,
    caps: RenderCaps,
    token: NativeToken,
    frame_surface: Option<SurfaceId>,
    shader_log: core::cell::RefCell<String>,
    // Last: the context is deleted after everything that calls into it is gone.
    ctx: GlContext,
}

impl GlDevice {
    /// Makes the device and its OpenGL 4.5 core context for the window named by `token`.
    ///
    /// # Errors
    /// What [`GlContext::create`] reports: no window, no OpenGL 4.5, a missing function.
    pub fn create(token: NativeToken) -> Result<Self, PlatformError> {
        let ctx = GlContext::create(token)?;
        // Flags left by the context's own setup are not this device's.
        let _setup_flags = drain_errors(&ctx);
        let shadow = NullDevice::bookkeeping_only(CAPACITY);
        let caps = caps_of(&ctx, shadow.caps());
        Ok(Self {
            shadow,
            res: Resources::new(),
            caps,
            token,
            frame_surface: None,
            shader_log: core::cell::RefCell::new(String::new()),
            ctx,
        })
    }

    /// The oldest OpenGL error flag, cleared; 0 is "no error". The device checks and clears the
    /// flags after its own calls, so a non-zero value here is a bug worth a report.
    #[must_use]
    pub fn take_gl_error(&self) -> u32 {
        self.ctx.take_error()
    }

    /// The driver's message for the last shader or pipeline the device refused with
    /// `ShaderRejected` (at most 1024 bytes, invalid UTF-8 replaced); empty when none was.
    /// The port's error value carries no text, so this is how a shader author learns why.
    #[must_use]
    pub fn last_shader_log(&self) -> String {
        self.shader_log.borrow().clone()
    }

    /// What the driver reports about the context (vendor, renderer, version).
    #[must_use]
    pub fn context_info(&self) -> &crate::ContextInfo {
        self.ctx.info()
    }

    /// Drains the error flags: `Ok` when there were none, else the first one as a port error.
    fn check(&self) -> Result<(), RenderError> {
        drain_errors(&self.ctx)
    }

    /// Accepts a freshly made record when its main GL object exists and GL raised no error;
    /// otherwise deletes the record's objects and says why.
    fn finish<R: Record>(&self, name: u32, record: R) -> Result<R, RenderError> {
        let checked = self.check();
        if name == 0 || checked.is_err() {
            record.delete(self.ctx.api());
            return Err(checked.err().unwrap_or(RenderError::Unsupported));
        }
        Ok(record)
    }

    /// Keeps `log` as the last shader log and gives the port error for a refused shader.
    fn reject(&self, log: String) -> RenderError {
        self.discard_flags();
        *self.shader_log.borrow_mut() = log;
        RenderError::ShaderRejected
    }

    /// Clears the error flags nobody asked about, so a stale one cannot surface in a later call.
    fn discard_flags(&self) {
        let _stale = self.check();
    }

    fn frame_size(&self) -> Result<(u32, u32), RenderError> {
        let surface = self.frame_surface.ok_or(RenderError::InvalidHandle)?;
        let record = self
            .res
            .surfaces
            .get(surface)
            .ok_or(RenderError::SurfaceLost)?;
        Ok((record.width, record.height))
    }

    fn make_buffer(&self, desc: BufferDesc) -> Result<BufferRec, RenderError> {
        let size = usize::try_from(desc.size).map_err(|_| INVALID)?;
        let name = self.ctx.api().create_buffer(size);
        self.finish(name, BufferRec { name })
    }

    fn make_texture(&self, desc: TextureDesc) -> Result<TextureRec, RenderError> {
        let layout = texture_layout(desc.format);
        let storage = TextureStorage {
            levels: gl_int(desc.mip_count)?,
            internal: layout.internal,
            width: gl_int(desc.width)?,
            height: gl_int(desc.height)?,
        };
        let name = self.ctx.api().create_texture(storage);
        self.ctx.api().limit_levels(name, storage.levels);
        let record = TextureRec {
            name,
            width: desc.width,
            height: desc.height,
            layout,
            depth: desc.format == TextureFormat::Depth32F,
            framebuffer: None,
        };
        self.finish(name, record)
    }

    fn make_sampler(&self, desc: SamplerDesc) -> Result<SamplerRec, RenderError> {
        let api = self.ctx.api();
        let name = api.create_sampler();
        api.sampler_parameter(name, TEXTURE_MIN_FILTER, min_filter(desc.min_filter));
        api.sampler_parameter(name, TEXTURE_MAG_FILTER, mag_filter(desc.mag_filter));
        api.sampler_parameter(name, TEXTURE_WRAP_S, wrap_mode(desc.address));
        api.sampler_parameter(name, TEXTURE_WRAP_T, wrap_mode(desc.address));
        self.finish(name, SamplerRec { name })
    }

    fn make_shader(&self, desc: ShaderDesc<'_>) -> Result<ShaderRec, RenderError> {
        if desc.entry != ENTRY {
            return Err(self.reject(format!("the entry point must be `{ENTRY}`")));
        }
        if core::str::from_utf8(desc.code).is_err() {
            return Err(self.reject("the shader blob is not UTF-8 GLSL source".into()));
        }
        let compiled = self
            .ctx
            .api()
            .compile_shader(shader_kind(desc.stage), desc.code);
        let name = compiled.map_err(|log| self.reject(log))?;
        self.finish(name, ShaderRec { name })
    }

    fn make_pipeline(&self, desc: &PipelineDesc<'_>) -> Result<PipelineRec, RenderError> {
        let vertex = self.res.shaders.get(desc.vertex_shader);
        let fragment = self.res.shaders.get(desc.fragment_shader);
        let (Some(vertex), Some(fragment)) = (vertex, fragment) else {
            return Err(RenderError::InvalidHandle);
        };
        let api = self.ctx.api();
        let linked = api.link_program(vertex.name, fragment.name);
        let program = linked.map_err(|log| self.reject(log))?;
        let vao = api.create_vertex_array();
        for attribute in desc.attributes {
            api.vertex_array_attribute(vao, attribute.location, attribute_format(attribute));
        }
        let record = PipelineRec {
            program,
            vao,
            stride: desc.vertex_stride,
            blend: desc.blend == BlendMode::Alpha,
            depth_test: desc.depth.test,
            depth_write: desc.depth.write,
        };
        self.finish(vao, record)
    }
}

impl Drop for GlDevice {
    fn drop(&mut self) {
        self.res.delete_all(self.ctx.api());
    }
}

/// What the device can do: the port's limits, narrowed by what the driver reports.
fn caps_of(ctx: &GlContext, port: RenderCaps) -> RenderCaps {
    let api = ctx.api();
    let driver_size = u32::try_from(api.integer(MAX_TEXTURE_SIZE)).unwrap_or(0);
    let alignment = u32::try_from(api.integer(UNIFORM_BUFFER_OFFSET_ALIGNMENT))
        .unwrap_or(port.uniform_buffer_alignment);
    RenderCaps {
        max_texture_size: driver_size.min(port.max_texture_size),
        uniform_buffer_alignment: alignment,
        index_u16: true,
        index_u32: true,
        // `glClipControl(LOWER_LEFT, ZERO_TO_ONE)` is set when the context is made.
        depth_zero_to_one: true,
        device_name: DEVICE_NAME,
    }
}

/// Reads and clears the error flags (a few at most): `Ok` when there were none, else the first
/// one as a port error.
fn drain_errors(ctx: &GlContext) -> Result<(), RenderError> {
    let codes = core::iter::repeat_with(|| ctx.take_error())
        .take(MAX_ERROR_FLAGS)
        .take_while(|code| *code != 0);
    let mut first = Ok(());
    for code in codes {
        if first.is_ok() {
            first = error_from_gl(code);
        }
    }
    first
}

fn attribute_format(attribute: &VertexAttribute) -> AttribFormat {
    let layout = attribute_layout(attribute.format);
    AttribFormat {
        components: layout.components,
        data_type: layout.data_type,
        normalized: layout.normalized,
        offset: attribute.offset,
    }
}

/// A count as the `i32` OpenGL takes.
fn gl_int(value: u32) -> Result<i32, RenderError> {
    i32::try_from(value).map_err(|_| INVALID)
}

fn region_of(rect: Rect) -> Result<Region, RenderError> {
    Ok(Region {
        x: rect.x,
        y: rect.y,
        width: gl_int(rect.width)?,
        height: gl_int(rect.height)?,
    })
}

/// Stores `record` for `id`; when the table cannot, deletes the record's GL objects.
fn keep<R: Record, M>(
    table: &mut Table<R>,
    id: Handle<M>,
    record: R,
    ctx: &GlContext,
) -> Result<(), RenderError> {
    table.insert(id, record).map_err(|(error, rejected)| {
        rejected.delete(ctx.api());
        error
    })
}

impl IRenderDevice for GlDevice {
    fn caps(&self) -> RenderCaps {
        self.caps
    }

    fn create_surface(&mut self, desc: SurfaceDesc) -> Result<SurfaceId, RenderError> {
        let id = self.shadow.create_surface(desc)?;
        let record = SurfaceRec {
            width: desc.width,
            height: desc.height,
        };
        let made = if desc.native_token == self.token.0 {
            keep(&mut self.res.surfaces, id, record, &self.ctx)
        } else {
            Err(RenderError::Unsupported)
        };
        if made.is_err() {
            self.shadow.destroy_surface(id);
        }
        made.map(|()| id)
    }

    fn resize_surface(
        &mut self,
        surface: SurfaceId,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.shadow.resize_surface(surface, width, height)?;
        let record = self
            .res
            .surfaces
            .get_mut(surface)
            .ok_or(RenderError::InvalidHandle)?;
        record.width = width;
        record.height = height;
        Ok(())
    }

    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.shadow.destroy_surface(surface);
        self.res.surfaces.remove(surface);
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId, RenderError> {
        let id = self.shadow.create_buffer(desc)?;
        let made = self
            .make_buffer(desc)
            .and_then(|record| keep(&mut self.res.buffers, id, record, &self.ctx));
        if made.is_err() {
            self.shadow.destroy_buffer(id);
        }
        made.map(|()| id)
    }

    fn create_texture(&mut self, desc: TextureDesc) -> Result<TextureId, RenderError> {
        let id = self.shadow.create_texture(desc)?;
        let made = self
            .make_texture(desc)
            .and_then(|record| keep(&mut self.res.textures, id, record, &self.ctx));
        if made.is_err() {
            self.shadow.destroy_texture(id);
        }
        made.map(|()| id)
    }

    fn create_sampler(&mut self, desc: SamplerDesc) -> Result<SamplerId, RenderError> {
        let id = self.shadow.create_sampler(desc)?;
        let made = self
            .make_sampler(desc)
            .and_then(|record| keep(&mut self.res.samplers, id, record, &self.ctx));
        if made.is_err() {
            self.shadow.destroy_sampler(id);
        }
        made.map(|()| id)
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId, RenderError> {
        let id = self.shadow.create_shader(desc)?;
        let made = self
            .make_shader(desc)
            .and_then(|record| keep(&mut self.res.shaders, id, record, &self.ctx));
        if made.is_err() {
            self.shadow.destroy_shader(id);
        }
        made.map(|()| id)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> Result<PipelineId, RenderError> {
        let id = self.shadow.create_pipeline(desc)?;
        let made = self
            .make_pipeline(&desc)
            .and_then(|record| keep(&mut self.res.pipelines, id, record, &self.ctx));
        if made.is_err() {
            self.shadow.destroy_pipeline(id);
        }
        made.map(|()| id)
    }

    fn write_buffer(
        &mut self,
        buffer: BufferId,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        self.shadow.write_buffer(buffer, offset, bytes)?;
        let record = self
            .res
            .buffers
            .get(buffer)
            .ok_or(RenderError::InvalidHandle)?;
        let offset = usize::try_from(offset).map_err(|_| INVALID)?;
        self.ctx.api().buffer_sub_data(record.name, offset, bytes);
        self.check()
    }

    fn write_texture(
        &mut self,
        texture: TextureId,
        region: Rect,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        self.shadow.write_texture(texture, region, bytes)?;
        let record = self
            .res
            .textures
            .get(texture)
            .ok_or(RenderError::InvalidHandle)?;
        let transfer = Transfer {
            format: record.layout.format,
            data_type: record.layout.data_type,
        };
        let written =
            self.ctx
                .api()
                .texture_sub_image(record.name, region_of(region)?, transfer, bytes);
        if !written {
            return Err(INVALID);
        }
        self.check()
    }

    fn destroy_buffer(&mut self, buffer: BufferId) {
        self.shadow.destroy_buffer(buffer);
        if let Some(record) = self.res.buffers.remove(buffer) {
            record.delete(self.ctx.api());
        }
        self.discard_flags();
    }

    fn destroy_texture(&mut self, texture: TextureId) {
        self.shadow.destroy_texture(texture);
        if let Some(record) = self.res.textures.remove(texture) {
            record.delete(self.ctx.api());
        }
        self.discard_flags();
    }

    fn destroy_shader(&mut self, shader: ShaderId) {
        self.shadow.destroy_shader(shader);
        if let Some(record) = self.res.shaders.remove(shader) {
            record.delete(self.ctx.api());
        }
        self.discard_flags();
    }

    fn destroy_sampler(&mut self, sampler: SamplerId) {
        self.shadow.destroy_sampler(sampler);
        if let Some(record) = self.res.samplers.remove(sampler) {
            record.delete(self.ctx.api());
        }
        self.discard_flags();
    }

    fn destroy_pipeline(&mut self, pipeline: PipelineId) {
        self.shadow.destroy_pipeline(pipeline);
        if let Some(record) = self.res.pipelines.remove(pipeline) {
            record.delete(self.ctx.api());
        }
        self.discard_flags();
    }

    fn begin_frame(&mut self, surface: SurfaceId) -> Result<FrameId, RenderError> {
        let frame = self.shadow.begin_frame(surface)?;
        self.frame_surface = Some(surface);
        self.discard_flags();
        Ok(frame)
    }

    fn submit(
        &mut self,
        frame: FrameId,
        stream: &CommandStream,
    ) -> Result<SubmitStats, RenderError> {
        let stats = self.shadow.submit(frame, stream)?;
        let size = self.frame_size()?;
        let ran = Executor::new(&self.ctx, &mut self.res, size).run(stream);
        let checked = self.check();
        ran.and(checked).map(|()| stats)
    }

    fn end_frame(&mut self, frame: FrameId) -> Result<(), RenderError> {
        self.shadow.end_frame(frame)?;
        self.frame_surface = None;
        let presented = self.ctx.present();
        self.discard_flags();
        if presented {
            Ok(())
        } else {
            Err(RenderError::SurfaceLost)
        }
    }

    fn read_back(&mut self, texture: TextureId, out: &mut [u8]) -> Result<(), RenderError> {
        let record = self
            .res
            .textures
            .get(texture)
            .ok_or(RenderError::InvalidHandle)?;
        let texels = u64::from(record.width) * u64::from(record.height);
        if u64::try_from(out.len()).ok() != texels.checked_mul(4) {
            return Err(INVALID);
        }
        let transfer = Transfer {
            format: record.layout.format,
            data_type: record.layout.data_type,
        };
        self.ctx.api().get_texture_image(record.name, transfer, out);
        self.check()
    }
}
