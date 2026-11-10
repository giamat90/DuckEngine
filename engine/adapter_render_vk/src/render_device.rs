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
//! Known limits of this part:
//! - Shaders and pipelines are refused (`Unsupported`) until the next part of the card; the
//!   commands that need a pipeline are accepted and do nothing.
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

use crate::error::VulkanError;
use crate::presenter::{Begin, FrameOutcome, Presenter};
use crate::resources::TextureId as OwnTexture;
use crate::resources::{BufferId as OwnBuffer, Resources, SamplerId as OwnSampler};
use crate::table::Table;
use crate::{ValidationReport, VulkanContext, VulkanOptions};
use exec::{Exec, FrameState, present_layout};
use platform::window::NativeToken;
use render::caps::RenderCaps;
use render::desc::{
    BufferDesc, PipelineDesc, Rect, SamplerDesc, ShaderDesc, SubmitStats, SurfaceDesc, TextureDesc,
    TextureFormat,
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

use convert::{buffer_desc, region, render_error, sampler_desc, texture_desc};

/// Resources of each kind the device can hold (the bookkeeping device keeps no bytes, so this
/// costs a few bytes per slot).
const CAPACITY: u32 = 1024;
const DEVICE_NAME: &str = "Vulkan 1.3";

/// What the device remembers about a texture besides its Vulkan objects.
pub struct TextureRec {
    pub id: OwnTexture,
    pub width: u32,
    pub height: u32,
    pub depth: bool,
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
    open: Option<OpenFrame>,
    pending: Vec<Pending>,
    // After the tables: the resources and the context outlive what uses them.
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
            open: None,
            pending: Vec::new(),
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
        Ok(match outcome {
            Begin::Ready(back) => Some(FrameState::new(back)),
            Begin::Skipped(_) => None,
        })
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

    fn create_shader(&mut self, _desc: ShaderDesc<'_>) -> Result<ShaderId, RenderError> {
        Err(RenderError::Unsupported)
    }

    fn create_pipeline(&mut self, _desc: PipelineDesc<'_>) -> Result<PipelineId, RenderError> {
        Err(RenderError::Unsupported)
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

    fn destroy_shader(&mut self, _shader: ShaderId) {}

    fn destroy_sampler(&mut self, sampler: SamplerId) {
        self.shadow.destroy_sampler(sampler);
        if let Some(own) = self.samplers.remove(sampler) {
            self.retire(Pending::Sampler(own));
        }
    }

    fn destroy_pipeline(&mut self, _pipeline: PipelineId) {}

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
        let mut run = Exec {
            gear: presenter.gear(),
            cmd: image.back.cmd,
            textures: &self.textures,
            resources: &mut self.resources,
            state: image,
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
