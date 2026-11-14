//! `VulkanDevice`: the render device port on Vulkan 1.3 (card 6-9, part 4 of the Vulkan adapter).
//!
//! A bookkeeping [`NullDevice`] sits inside, as in the GL adapter: it issues the handles and
//! checks every call and every stream by the rules the Null device defines, so this device
//! accepts and refuses exactly what the port says and the rules exist once. Only after it agrees
//! does the Vulkan side act, and when Vulkan then fails the handle is given back. What Vulkan
//! needs to remember about a handle lives in tables keyed by it.
//!
//! Frames: `begin_frame` acquires a swapchain image and opens the frame's command buffer, each
//! `submit` records its stream into that buffer, and `end_frame` moves the backbuffer to the
//! present layout, submits the buffer and presents. Images are moved between layouts by the
//! device as passes need them; a `Barrier` command moves one on request ([`exec`]).
//!
//! Shaders and pipelines (card 6-14): a shader blob is SPIR-V (backend byte 1 of the `dshd` file),
//! checked and renumbered into the adapter's fixed binding scheme ([`crate::spirv`]); a pipeline
//! is made twice, for an `R8G8B8A8` and for a `B8G8R8A8` target, because the port does not tie a
//! pipeline to the target it is used with and Vulkan does. Each surface has a descriptor pool per
//! frame in flight, reset when the frame's turn comes round.
//!
//! Known limits of this part:
//! - A shader has the entry point `main`, binds at most 8 uniform buffers and 8 textures, and has
//!   no storage buffer (`Unsupported`); a frame draws at most 1024 times with changed bindings
//!   (`OutOfMemory` after that, until the next frame).
//! - One live surface per window: the Vulkan surface of a window can exist once.
//! - Writes (`write_buffer`, `write_texture`) and `read_back` run at once on their own queue
//!   submission, so they see the work of the frames already ended and not the commands recorded
//!   in the open frame: write before the `submit` that uses the data, and read back after
//!   `end_frame`.
//! - `write_texture` and `read_back` are refused (`Unsupported`) while the open frame holds
//!   recorded commands: the layouts they would start from are not yet the layouts on the queue.
//! - Destroying a resource waits for the device to be idle first (a stall, but a resource may be
//!   in use by a frame still in flight). A resource destroyed while the open frame holds recorded
//!   commands is kept until `end_frame` has put the frame on the queue.
//! - A failed submit at `end_frame` drops the surface's presenter: the next `begin_frame` on it
//!   reports `SurfaceLost`, and the surface is made again.
//! - A frame on a window without area (minimised) executes nothing, texture passes included.

use crate::draw::{Draw, PipelineSpec, Stage};
use crate::error::VulkanError;
use crate::frame::FRAMES_IN_FLIGHT;
use crate::presenter::{Begin, FrameOutcome, Presenter};
use crate::resources::TextureId as OwnTexture;
use crate::resources::{BufferId as OwnBuffer, Resources, SamplerId as OwnSampler};
use crate::spirv;
use crate::table::Table;
use crate::vk_draw::Handle;
use crate::vk_present::FORMAT_B8G8R8A8_UNORM;
use crate::vk_resource::FORMAT_R8G8B8A8_UNORM;
use crate::{ValidationReport, VulkanContext, VulkanOptions};
use core::ffi::CStr;
use exec::{Exec, FrameState, Objects, present_layout};
use platform::window::NativeToken;
use render::caps::RenderCaps;
use render::desc::{
    BlendMode, BufferDesc, PipelineDesc, Rect, SamplerDesc, ShaderDesc, SubmitStats, SurfaceDesc,
    TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{BufferId, FrameId, PipelineId, SamplerId, ShaderId, SurfaceId, TextureId};
use render::null::NullDevice;
use render::stream::CommandStream;

mod convert;
mod exec;
#[cfg(test)]
mod tests;

use convert::{
    attribute, buffer_desc, depth_format, region, render_error, sampler_desc, spirv_error,
    texture_desc,
};

/// Resources of each kind the device can hold (the bookkeeping device keeps no bytes, so this
/// costs a few bytes per slot).
const CAPACITY: u32 = 1024;
const DEVICE_NAME: &str = "Vulkan 1.3";
/// The entry point every shader has (the shader compiler writes `main`).
const ENTRY: &str = "main";
const ENTRY_C: &CStr = c"main";

/// What the device remembers about a texture besides its Vulkan objects.
pub struct TextureRec {
    pub id: OwnTexture,
    pub width: u32,
    pub height: u32,
    pub depth: bool,
    /// `B8G8R8A8` (else `R8G8B8A8` or depth): picks the pipeline variant of a pass.
    pub bgra: bool,
}

/// The frame between `begin_frame` and `end_frame`.
struct OpenFrame {
    surface: SurfaceId,
    /// `None` when the surface gave no image (minimised, or the swapchain was just rebuilt):
    /// the frame executes nothing.
    state: Option<FrameState>,
    /// A stream was recorded into the frame's buffer, which the queue has not seen yet.
    recorded: bool,
}

/// A resource destroyed while the open frame's buffer, which may name it, was not yet submitted.
enum Pending {
    Buffer(OwnBuffer),
    Texture(OwnTexture),
    Sampler(OwnSampler),
    Pipeline([Handle; 2]),
}

/// The render device on a Vulkan 1.3 device and queue.
///
/// Made from a window's native token; single thread, like the context. Drop it before the window
/// is destroyed.
pub struct VulkanDevice {
    shadow: NullDevice,
    caps: RenderCaps,
    token: NativeToken,
    buffers: Table<OwnBuffer>,
    textures: Table<TextureRec>,
    samplers: Table<OwnSampler>,
    surfaces: Table<Presenter>,
    shaders: Table<Handle>,
    pipelines: Table<[Handle; 2]>,
    /// The descriptor pool of each frame in flight, per surface.
    pools: Table<[Handle; FRAMES_IN_FLIGHT]>,
    open: Option<OpenFrame>,
    pending: Vec<Pending>,
    // After the tables (the presenters wait for the device when they go): the draw objects, the
    // resources and the context outlive what uses them.
    draw: Draw,
    resources: Resources,
    context: VulkanContext,
}

impl VulkanDevice {
    /// Starts Vulkan and makes the device for the window named by `token`.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] for the null token, and what [`VulkanContext::create`] and
    /// [`VulkanContext::create_resources`] report: no loader, no suitable device, a missing
    /// function.
    pub fn create(token: NativeToken, options: VulkanOptions) -> Result<Self, VulkanError> {
        if token == NativeToken::NULL {
            return Err(VulkanError::Unsupported("the null token names no window"));
        }
        Self::with_context(VulkanContext::create(options)?, token)
    }

    /// The device on a context that is already running.
    pub(crate) fn with_context(
        context: VulkanContext,
        token: NativeToken,
    ) -> Result<Self, VulkanError> {
        let resources = context.create_resources()?;
        let draw = Draw::new(context.core())?;
        let shadow = NullDevice::bookkeeping_only(CAPACITY);
        let port = shadow.caps();
        let caps = RenderCaps {
            max_texture_size: port.max_texture_size.min(context.core().max_texture_extent),
            device_name: DEVICE_NAME,
            ..port
        };
        Ok(Self {
            shadow,
            caps,
            token,
            buffers: Table::new(),
            textures: Table::new(),
            samplers: Table::new(),
            surfaces: Table::new(),
            shaders: Table::new(),
            pipelines: Table::new(),
            pools: Table::new(),
            open: None,
            pending: Vec::new(),
            draw,
            resources,
            context,
        })
    }

    /// The name the driver gives the GPU in use (`caps().device_name` is the API's name).
    #[must_use]
    pub fn gpu_name(&self) -> &str {
        &self.context.info().name
    }

    /// What the validation layer said so far; clean when it was not installed.
    #[must_use]
    pub fn validation_report(&self) -> ValidationReport {
        self.context.validation_report()
    }

    /// Whether the validation layer is on.
    #[must_use]
    pub fn validation_active(&self) -> bool {
        self.context.info().validation_active
    }

    fn make_presenter(&self, desc: SurfaceDesc) -> Result<Presenter, RenderError> {
        if desc.native_token != self.token.0 {
            return Err(RenderError::Unsupported);
        }
        self.context
            .create_presenter(self.token, desc.width, desc.height)
            .map_err(|error| render_error(&error))
    }

    /// Makes the buffer and files it under `id`; what cannot be filed is destroyed again.
    fn make_buffer(&mut self, id: BufferId, desc: BufferDesc) -> Result<(), RenderError> {
        let own = self
            .resources
            .create_buffer(buffer_desc(desc))
            .map_err(|error| render_error(&error))?;
        if let Err((error, own)) = self.buffers.insert(id, own) {
            self.resources.destroy_buffer(own);
            return Err(error);
        }
        Ok(())
    }

    fn make_texture(&mut self, id: TextureId, desc: TextureDesc) -> Result<(), RenderError> {
        let own = self
            .resources
            .create_texture(texture_desc(desc))
            .map_err(|error| render_error(&error))?;
        let record = TextureRec {
            id: own,
            width: desc.width,
            height: desc.height,
            depth: desc.format == TextureFormat::Depth32F,
            bgra: desc.format == TextureFormat::Bgra8,
        };
        if let Err((error, record)) = self.textures.insert(id, record) {
            self.resources.destroy_texture(record.id);
            return Err(error);
        }
        Ok(())
    }

    fn make_sampler(&mut self, id: SamplerId, desc: SamplerDesc) -> Result<(), RenderError> {
        let own = self
            .resources
            .create_sampler(sampler_desc(desc))
            .map_err(|error| render_error(&error))?;
        if let Err((error, own)) = self.samplers.insert(id, own) {
            self.resources.destroy_sampler(own);
            return Err(error);
        }
        Ok(())
    }

    /// Starts the frame on the surface's presenter. A swapchain that was out of date is rebuilt
    /// by the first try, so the frame tries once more; a surface that still gives no image
    /// (minimised) gives `None`, and the frame executes nothing.
    fn acquire(&mut self, surface: SurfaceId) -> Result<Option<FrameState>, VulkanError> {
        let presenter = self
            .surfaces
            .get_mut(surface)
            .ok_or(VulkanError::SurfaceLost)?;
        let mut outcome = presenter.begin_frame()?;
        if matches!(outcome, Begin::Skipped(FrameOutcome::Recreated)) {
            outcome = presenter.begin_frame()?;
        }
        match outcome {
            Begin::Ready(back) => {
                self.reset_pool(surface, back.slot)?;
                Ok(Some(FrameState::new(back)))
            }
            Begin::Skipped(_) => Ok(None),
        }
    }

    /// Gives back the sets of the frame slot that just came round (its fence was waited for).
    fn reset_pool(&self, surface: SurfaceId, slot: usize) -> Result<(), VulkanError> {
        let pool = self
            .pools
            .get(surface)
            .and_then(|pools| pools.get(slot))
            .ok_or(VulkanError::SurfaceLost)?;
        self.draw.reset_pool(*pool)
    }

    /// The pools of a surface that is being made; what was made is destroyed on a failure.
    fn make_pools(&mut self) -> Result<[Handle; FRAMES_IN_FLIGHT], RenderError> {
        let mut made = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let result = (0..FRAMES_IN_FLIGHT).try_for_each(|_| {
            made.push(self.draw.create_pool()?);
            Ok::<(), VulkanError>(())
        });
        if let Err(error) = result {
            self.destroy_pools(made);
            return Err(render_error(&error));
        }
        made.try_into().map_err(|_| RenderError::Unsupported)
    }

    fn destroy_pools(&mut self, pools: impl IntoIterator<Item = Handle>) {
        for pool in pools {
            self.draw.destroy_pool(pool);
        }
    }

    fn destroy_pipelines(&mut self, pipelines: impl IntoIterator<Item = Handle>) {
        for pipeline in pipelines {
            self.draw.destroy_pipeline(pipeline);
        }
    }

    fn file_pools(&mut self, surface: SurfaceId) -> Result<(), RenderError> {
        let pools = self.make_pools()?;
        if let Err((error, pools)) = self.pools.insert(surface, pools) {
            self.destroy_pools(pools);
            return Err(error);
        }
        Ok(())
    }

    /// Destroys the pools of a surface that is gone (after the device has finished with them).
    fn drop_pools(&mut self, surface: SurfaceId) {
        if let Some(pools) = self.pools.remove(surface) {
            self.settle();
            self.destroy_pools(pools);
        }
    }

    fn make_shader(&mut self, id: ShaderId, desc: ShaderDesc<'_>) -> Result<(), RenderError> {
        if desc.entry != ENTRY {
            return Err(RenderError::InvalidDescription);
        }
        let mut words = spirv::words(desc.code).map_err(spirv_error)?;
        spirv::retarget(&mut words).map_err(spirv_error)?;
        let module = self
            .draw
            .create_shader(&words)
            .map_err(|error| render_error(&error))?;
        if let Err((error, module)) = self.shaders.insert(id, module) {
            self.draw.destroy_shader(module);
            return Err(error);
        }
        Ok(())
    }

    /// Makes both variants of the pipeline (one per target format) and files them under `id`.
    fn make_pipeline(
        &mut self,
        id: PipelineId,
        desc: &PipelineDesc<'_>,
    ) -> Result<(), RenderError> {
        let vertex = *self
            .shaders
            .get(desc.vertex_shader)
            .ok_or(RenderError::InvalidHandle)?;
        let fragment = *self
            .shaders
            .get(desc.fragment_shader)
            .ok_or(RenderError::InvalidHandle)?;
        let attributes: Vec<_> = desc.attributes.iter().copied().map(attribute).collect();
        let spec = |color_format| PipelineSpec {
            vertex: Stage {
                module: vertex,
                entry: ENTRY_C,
            },
            fragment: Stage {
                module: fragment,
                entry: ENTRY_C,
            },
            stride: desc.vertex_stride,
            attributes: &attributes,
            blend: desc.blend == BlendMode::Alpha,
            depth_test: desc.depth.test,
            depth_write: desc.depth.write,
            color_format,
            depth_format: depth_format(desc.depth_format),
        };
        let rgba = self
            .draw
            .create_pipeline(&spec(FORMAT_R8G8B8A8_UNORM))
            .map_err(|error| render_error(&error))?;
        let bgra = match self.draw.create_pipeline(&spec(FORMAT_B8G8R8A8_UNORM)) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                self.draw.destroy_pipeline(rgba);
                return Err(render_error(&error));
            }
        };
        // The index of a variant is `usize::from(target is bgra)`.
        if let Err((error, variants)) = self.pipelines.insert(id, [rgba, bgra]) {
            self.destroy_pipelines(variants);
            return Err(error);
        }
        Ok(())
    }

    fn frame_error<T>() -> Result<T, RenderError> {
        Err(RenderError::InvalidHandle)
    }

    /// Waits until the device has finished everything it was given, before a destroy.
    fn settle(&self) {
        let _ = self.resources.wait_idle();
    }

    /// The open frame holds recorded commands that the queue has not run.
    fn recording(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.recorded)
    }

    /// Destroys what was kept back during a frame; called once the frame is on the queue.
    fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        self.settle();
        for item in std::mem::take(&mut self.pending) {
            self.free(&item);
        }
    }

    fn free(&mut self, item: &Pending) {
        match item {
            Pending::Buffer(own) => self.resources.destroy_buffer(*own),
            Pending::Texture(own) => self.resources.destroy_texture(*own),
            Pending::Sampler(own) => self.resources.destroy_sampler(*own),
            Pending::Pipeline(variants) => self.destroy_pipelines(*variants),
        }
    }

    /// Frees a resource now, or after the open frame is on the queue if that frame may use it.
    fn retire(&mut self, item: Pending) {
        if self.recording() {
            self.pending.push(item);
        } else {
            self.settle();
            self.free(&item);
        }
    }
}

impl IRenderDevice for VulkanDevice {
    fn caps(&self) -> RenderCaps {
        self.caps
    }

    fn create_surface(&mut self, desc: SurfaceDesc) -> Result<SurfaceId, RenderError> {
        let id = self.shadow.create_surface(desc)?;
        let made = self.make_presenter(desc).and_then(|presenter| {
            self.surfaces
                .insert(id, presenter)
                .map_err(|(error, _)| error)
        });
        let made = made.and_then(|()| self.file_pools(id));
        if made.is_err() {
            self.surfaces.remove(id);
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
        let presenter = self
            .surfaces
            .get_mut(surface)
            .ok_or(RenderError::InvalidHandle)?;
        presenter
            .resize(width, height)
            .map_err(|error| render_error(&error))
    }

    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.shadow.destroy_surface(surface);
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.surface == surface)
        {
            self.open = None;
        }
        self.surfaces.remove(surface);
        self.drop_pools(surface);
        self.flush_pending();
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId, RenderError> {
        let id = self.shadow.create_buffer(desc)?;
        let made = self.make_buffer(id, desc);
        if made.is_err() {
            self.shadow.destroy_buffer(id);
        }
        made.map(|()| id)
    }

    fn create_texture(&mut self, desc: TextureDesc) -> Result<TextureId, RenderError> {
        let id = self.shadow.create_texture(desc)?;
        let made = self.make_texture(id, desc);
        if made.is_err() {
            self.shadow.destroy_texture(id);
        }
        made.map(|()| id)
    }

    fn create_sampler(&mut self, desc: SamplerDesc) -> Result<SamplerId, RenderError> {
        let id = self.shadow.create_sampler(desc)?;
        let made = self.make_sampler(id, desc);
        if made.is_err() {
            self.shadow.destroy_sampler(id);
        }
        made.map(|()| id)
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId, RenderError> {
        let id = self.shadow.create_shader(desc)?;
        let made = self.make_shader(id, desc);
        if made.is_err() {
            self.shadow.destroy_shader(id);
        }
        made.map(|()| id)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> Result<PipelineId, RenderError> {
        let id = self.shadow.create_pipeline(desc)?;
        let made = self.make_pipeline(id, &desc);
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
        let own = *self.buffers.get(buffer).ok_or(RenderError::InvalidHandle)?;
        self.resources
            .write_buffer(own, offset, bytes)
            .map_err(|error| render_error(&error))
    }

    fn write_texture(
        &mut self,
        texture: TextureId,
        area: Rect,
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        self.shadow.write_texture(texture, area, bytes)?;
        if self.recording() {
            return Err(RenderError::Unsupported);
        }
        let record = self
            .textures
            .get(texture)
            .ok_or(RenderError::InvalidHandle)?;
        self.resources
            .write_texture(record.id, region(area), bytes)
            .map_err(|error| render_error(&error))
    }

    fn destroy_buffer(&mut self, buffer: BufferId) {
        self.shadow.destroy_buffer(buffer);
        if let Some(own) = self.buffers.remove(buffer) {
            self.retire(Pending::Buffer(own));
        }
    }

    fn destroy_texture(&mut self, texture: TextureId) {
        self.shadow.destroy_texture(texture);
        if let Some(record) = self.textures.remove(texture) {
            self.retire(Pending::Texture(record.id));
        }
    }

    fn destroy_shader(&mut self, shader: ShaderId) {
        self.shadow.destroy_shader(shader);
        // A pipeline keeps its own copy of the code, so the module can go at once.
        if let Some(module) = self.shaders.remove(shader) {
            self.draw.destroy_shader(module);
        }
    }

    fn destroy_sampler(&mut self, sampler: SamplerId) {
        self.shadow.destroy_sampler(sampler);
        if let Some(own) = self.samplers.remove(sampler) {
            self.retire(Pending::Sampler(own));
        }
    }

    fn destroy_pipeline(&mut self, pipeline: PipelineId) {
        self.shadow.destroy_pipeline(pipeline);
        if let Some(variants) = self.pipelines.remove(pipeline) {
            self.retire(Pending::Pipeline(variants));
        }
    }

    fn begin_frame(&mut self, surface: SurfaceId) -> Result<FrameId, RenderError> {
        let frame = self.shadow.begin_frame(surface)?;
        match self.acquire(surface) {
            Ok(image) => {
                self.open = Some(OpenFrame {
                    surface,
                    state: image,
                    recorded: false,
                });
                Ok(frame)
            }
            Err(error) => {
                // The bookkeeping side must not stay in a frame the Vulkan side never began.
                let _ = self.shadow.end_frame(frame);
                Err(render_error(&error))
            }
        }
    }

    fn submit(
        &mut self,
        frame: FrameId,
        stream: &CommandStream,
    ) -> Result<SubmitStats, RenderError> {
        let stats = self.shadow.submit(frame, stream)?;
        let Some(open) = self.open.as_mut() else {
            return Self::frame_error();
        };
        let Some(image) = open.state.as_mut() else {
            return Ok(stats);
        };
        open.recorded = true;
        let presenter = self
            .surfaces
            .get(open.surface)
            .ok_or(RenderError::SurfaceLost)?;
        let pool = self
            .pools
            .get(open.surface)
            .and_then(|pools| pools.get(image.back.slot))
            .ok_or(RenderError::SurfaceLost)?;
        let mut run = Exec {
            gear: presenter.gear(),
            cmd: image.back.cmd,
            textures: &self.textures,
            resources: &mut self.resources,
            state: image,
            draw: &self.draw,
            pool: *pool,
            objects: Objects {
                pipelines: &self.pipelines,
                buffers: &self.buffers,
                samplers: &self.samplers,
            },
        };
        run.run(stream).map(|()| stats)
    }

    fn end_frame(&mut self, frame: FrameId) -> Result<(), RenderError> {
        self.shadow.end_frame(frame)?;
        let Some(open) = self.open.take() else {
            return Self::frame_error();
        };
        let Some(mut state) = open.state else {
            return Ok(());
        };
        let presenter = self
            .surfaces
            .get_mut(open.surface)
            .ok_or(RenderError::SurfaceLost)?;
        present_layout(presenter.gear(), state.back.cmd, &mut state);
        let ended = presenter.end_frame(state.back);
        if ended.is_err() {
            // A failed submit leaves the frame's fence unsignalled, so the next frame on this
            // presenter would wait for it forever: the surface is lost, and made again.
            self.surfaces.remove(open.surface);
            self.drop_pools(open.surface);
        }
        self.flush_pending();
        ended.map(|_| ()).map_err(|error| render_error(&error))
    }

    fn read_back(&mut self, texture: TextureId, out: &mut [u8]) -> Result<(), RenderError> {
        let record = self
            .textures
            .get(texture)
            .ok_or(RenderError::InvalidHandle)?;
        if self.recording() {
            return Err(RenderError::Unsupported);
        }
        self.resources
            .read_back(record.id, out)
            .map_err(|error| render_error(&error))
    }
}
