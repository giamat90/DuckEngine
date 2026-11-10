//! The presenter: a window's surface and swapchain, two frames in flight, and a frame that
//! clears the window and shows it (card 6-7, the first visible result of the Vulkan adapter).
//!
//! It draws nothing but a colour. The pass, pipeline and resource commands of the render port
//! come in cards 6-8 and 6-9; what is here is what they build on: acquiring an image, the
//! per-frame fence, the per-image render-finished semaphore, submit and present, and recreating
//! the swapchain when the window changes.
#![expect(
    unsafe_code,
    reason = "submit and present are C calls; every unsafe block has a SAFETY comment"
)]

use crate::capture::Capture;
use crate::context::{Core, VulkanContext};
use crate::error::VulkanError;
use crate::frame::{FRAMES_IN_FLIGHT, Frame, Target, record_clear};
use crate::gear::{Gear, ok};
use crate::plan::make_plan;
use crate::surface::Surface;
use crate::swapchain::Swapchain;
use crate::vk::SUCCESS;
use crate::vk_present::{
    CommandBufferSubmitInfo, ERROR_OUT_OF_DATE, ERROR_SURFACE_LOST, EXTENT_FOLLOWS_SWAPCHAIN,
    FOREVER, IMAGE_USAGE_TRANSFER_SRC, PresentInfo, STAGE_ALL_COMMANDS,
    STAGE_COLOR_ATTACHMENT_OUTPUT, STYPE_COMMAND_BUFFER_SUBMIT_INFO, STYPE_PRESENT_INFO,
    STYPE_SEMAPHORE_SUBMIT_INFO, STYPE_SUBMIT_INFO_2, SUBOPTIMAL, SemaphoreSubmitInfo, SubmitInfo2,
};
use platform::window::NativeToken;
use std::rc::Rc;

#[cfg(test)]
mod tests;

/// What a call to [`Presenter::present_clear`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    /// The frame was drawn and queued for display.
    Presented,
    /// The swapchain no longer matched the window and was recreated; call again to draw.
    Recreated,
    /// The window has no area (it is minimised), so nothing was drawn.
    Minimized,
}

/// What acquiring an image gave.
enum Acquired {
    Image { index: u32, suboptimal: bool },
    OutOfDate,
}

/// A window the adapter can clear and present to.
///
/// Holds the shared core, so the device cannot go away under it. Single thread, like the
/// context. If a submit fails the device is lost or out of memory: drop the presenter.
pub struct Presenter {
    swapchain: Option<Swapchain>,
    frames: [Frame; FRAMES_IN_FLIGHT],
    frame_index: usize,
    capture: Option<Capture>,
    requested: (u32, u32),
    // After the parts above, which are destroyed in `drop` using the next two.
    surface: Surface,
    gear: Gear,
}

impl VulkanContext {
    /// Makes a presenter for the window named by `token` with a framebuffer of `width` by
    /// `height` pixels (used when the surface leaves the size to the swapchain).
    ///
    /// The token must name a window that is still alive and stays alive as long as the presenter
    /// exists: Vulkan does not check the handle, and a stale one is undefined behaviour in the
    /// driver. The window adapter owns the window; the app destroys the presenter first.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] when the system or the device cannot present, the token names
    /// no window, the graphics queue family cannot present to it, or the surface lacks the
    /// engine's format; [`VulkanError::Call`] and [`VulkanError::MissingFunction`] when the
    /// driver fails. Whatever was made before the failure is destroyed.
    pub fn create_presenter(
        &self,
        token: NativeToken,
        width: u32,
        height: u32,
    ) -> Result<Presenter, VulkanError> {
        Presenter::create(self.core(), token, width, height)
    }
}

impl Presenter {
    fn create(
        core: &Rc<Core>,
        token: NativeToken,
        width: u32,
        height: u32,
    ) -> Result<Self, VulkanError> {
        let surface = Surface::create(core, token)?;
        if !surface.supports_present(core.queue_family)? {
            return Err(VulkanError::Unsupported(
                "the graphics queue family cannot present to this window",
            ));
        }
        let mut presenter = Self {
            swapchain: None,
            frames: [Frame::EMPTY; FRAMES_IN_FLIGHT],
            frame_index: 0,
            capture: None,
            requested: (width, height),
            surface,
            gear: Gear::load(core)?,
        };
        // On an error the presenter is dropped here, which destroys what exists.
        presenter.init()?;
        Ok(presenter)
    }

    fn init(&mut self) -> Result<(), VulkanError> {
        for frame in &mut self.frames {
            frame.create(&self.gear, self.gear.core.queue_family)?;
        }
        self.rebuild()
    }

    /// The size of the swapchain images; zero by zero without a swapchain.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.swapchain.as_ref().map_or((0, 0), |swapchain| {
            (swapchain.extent.width, swapchain.extent.height)
        })
    }

    /// How many images the swapchain has; zero without a swapchain.
    #[must_use]
    pub fn image_count(&self) -> usize {
        self.swapchain
            .as_ref()
            .map_or(0, |swapchain| swapchain.images.len())
    }

    /// Tells the presenter the window's new framebuffer size and recreates the swapchain.
    ///
    /// # Errors
    /// As [`VulkanContext::create_presenter`].
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), VulkanError> {
        self.requested = (width, height);
        self.rebuild()
    }

    /// Makes the swapchain for the surface as it is now and retires the old one. Without area
    /// there is no swapchain afterwards.
    fn rebuild(&mut self) -> Result<(), VulkanError> {
        let caps = self.surface.capabilities()?;
        let formats = self.surface.formats()?;
        // The old swapchain's images may still be in use by frames in flight.
        self.gear.core.device.wait_idle()?;
        let plan = make_plan(&caps, &formats, self.requested.0, self.requested.1)?;
        let Some(plan) = plan else {
            self.retire_swapchain();
            return Ok(());
        };
        let old = self
            .swapchain
            .as_ref()
            .map_or(0, |swapchain| swapchain.handle);
        let made = Swapchain::create(&self.gear, &self.surface, &plan, old);
        self.install(made)
    }

    /// Puts a new swapchain in place of the old one, which is destroyed. When making it failed,
    /// the driver has retired the old swapchain anyway: it must not be handed over again, so it
    /// goes too and the next frame starts from nothing.
    fn install(&mut self, made: Result<Swapchain, VulkanError>) -> Result<(), VulkanError> {
        let new = match made {
            Ok(new) => new,
            Err(error) => {
                self.retire_swapchain();
                return Err(error);
            }
        };
        if let Some(mut old) = self.swapchain.replace(new) {
            old.destroy(&self.gear);
        }
        Ok(())
    }

    /// Whether the surface now asks for another size than the swapchain has. A suboptimal result
    /// that is not about the size (for example a rotated display) does not rebuild every frame.
    fn surface_changed(&self) -> Result<bool, VulkanError> {
        let current = self.surface.capabilities()?.current_extent;
        let follows = current.width == EXTENT_FOLLOWS_SWAPCHAIN;
        Ok(!follows
            && self.swapchain.as_ref().is_some_and(|swapchain| {
                swapchain.extent.width != current.width || swapchain.extent.height != current.height
            }))
    }

    /// Destroys the swapchain, if there is one. The device must be idle.
    fn retire_swapchain(&mut self) {
        if let Some(mut old) = self.swapchain.take() {
            old.destroy(&self.gear);
        }
    }

    /// Makes sure there is a swapchain; `false` when the surface has no area.
    fn ensure_swapchain(&mut self) -> Result<bool, VulkanError> {
        if self.swapchain.is_none() {
            self.rebuild()?;
        }
        Ok(self.swapchain.is_some())
    }

    /// Clears the window to `color` (red, green, blue, alpha, 0 to 1) and presents it.
    ///
    /// # Errors
    /// [`VulkanError::SurfaceLost`] when the window's surface is gone; [`VulkanError::Call`] when
    /// the driver fails.
    pub fn present_clear(&mut self, color: [f32; 4]) -> Result<FrameOutcome, VulkanError> {
        if !self.ensure_swapchain()? {
            return Ok(FrameOutcome::Minimized);
        }
        let frame = self.frames[self.frame_index];
        self.wait_for(frame.fence)?;
        let (index, mut stale) = match self.acquire(&frame)? {
            Acquired::Image { index, suboptimal } => (index, suboptimal),
            Acquired::OutOfDate => {
                self.rebuild()?;
                return Ok(FrameOutcome::Recreated);
            }
        };
        self.draw(&frame, index, color)?;
        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        match self.present(index)? {
            SUCCESS => {}
            SUBOPTIMAL => stale = true,
            _ => {
                self.rebuild()?;
                return Ok(FrameOutcome::Recreated);
            }
        }
        if stale && self.surface_changed()? {
            self.rebuild()?;
        }
        Ok(FrameOutcome::Presented)
    }

    fn wait_for(&self, fence: u64) -> Result<(), VulkanError> {
        // SAFETY: the fence belongs to this device; one fence at the given address.
        let result = unsafe {
            (self.gear.fns.wait_for_fences)(self.gear.device, 1, &raw const fence, 1, FOREVER)
        };
        ok("vkWaitForFences", result)
    }

    fn acquire(&self, frame: &Frame) -> Result<Acquired, VulkanError> {
        let Some(swapchain) = &self.swapchain else {
            return Ok(Acquired::OutOfDate);
        };
        let mut index = 0_u32;
        // SAFETY: the swapchain and the semaphore are live; the semaphore is unsignalled (the
        // frame's previous use finished, as the fence wait showed); `index` is a valid output.
        let result = unsafe {
            (self.gear.fns.acquire_next_image)(
                self.gear.device,
                swapchain.handle,
                FOREVER,
                frame.acquire,
                0,
                &raw mut index,
            )
        };
        match result {
            SUCCESS => Ok(Acquired::Image {
                index,
                suboptimal: false,
            }),
            SUBOPTIMAL => Ok(Acquired::Image {
                index,
                suboptimal: true,
            }),
            ERROR_OUT_OF_DATE => Ok(Acquired::OutOfDate),
            ERROR_SURFACE_LOST => Err(VulkanError::SurfaceLost),
            result => Err(VulkanError::Call {
                call: "vkAcquireNextImageKHR",
                result,
            }),
        }
    }

    /// Records and submits the frame for the acquired image.
    fn draw(&self, frame: &Frame, index: u32, color: [f32; 4]) -> Result<(), VulkanError> {
        let slot = usize::try_from(index).unwrap_or(usize::MAX);
        let (Some(swapchain), Some(image)) = (
            &self.swapchain,
            self.swapchain
                .as_ref()
                .and_then(|swapchain| swapchain.images.get(slot).copied()),
        ) else {
            return Err(VulkanError::Unsupported(
                "the acquired image does not exist",
            ));
        };
        let (Some(view), Some(done)) = (swapchain.view(slot), swapchain.render_done(slot)) else {
            return Err(VulkanError::Unsupported("the acquired image has no view"));
        };
        // The fence is not reset yet: until the submit it stays signalled, so a failure while
        // recording leaves the next wait on this frame returning.
        frame.reset(&self.gear)?;
        let target = Target {
            image,
            view,
            extent: swapchain.extent,
            color,
            capture: self.capture.as_ref(),
        };
        record_clear(&self.gear, frame.cmd, &target)?;
        self.submit(frame, done)
    }

    fn submit(&self, frame: &Frame, done: u64) -> Result<(), VulkanError> {
        // Only now is the fence reset: the submit that follows signals it again. A failed acquire
        // or a failed recording must leave it signalled, or the next wait would never return.
        // SAFETY: the fence belongs to this device and is signalled (waited for above).
        let result =
            unsafe { (self.gear.fns.reset_fences)(self.gear.device, 1, &raw const frame.fence) };
        ok("vkResetFences", result)?;
        let wait = SemaphoreSubmitInfo {
            s_type: STYPE_SEMAPHORE_SUBMIT_INFO,
            p_next: core::ptr::null(),
            semaphore: frame.acquire,
            value: 0,
            stage_mask: STAGE_COLOR_ATTACHMENT_OUTPUT,
            device_index: 0,
        };
        let signal = SemaphoreSubmitInfo {
            s_type: STYPE_SEMAPHORE_SUBMIT_INFO,
            p_next: core::ptr::null(),
            semaphore: done,
            value: 0,
            stage_mask: STAGE_ALL_COMMANDS,
            device_index: 0,
        };
        let buffer = CommandBufferSubmitInfo {
            s_type: STYPE_COMMAND_BUFFER_SUBMIT_INFO,
            p_next: core::ptr::null(),
            command_buffer: frame.cmd,
            device_mask: 0,
        };
        let info = SubmitInfo2 {
            s_type: STYPE_SUBMIT_INFO_2,
            p_next: core::ptr::null(),
            flags: 0,
            wait_semaphore_info_count: 1,
            wait_semaphore_infos: &raw const wait,
            command_buffer_info_count: 1,
            command_buffer_infos: &raw const buffer,
            signal_semaphore_info_count: 1,
            signal_semaphore_infos: &raw const signal,
        };
        // SAFETY: `info` and what it points to live until the call returns; the semaphores, the
        // buffer (recorded, ended) and the fence belong to this device and queue family.
        let result = unsafe {
            (self.gear.fns.queue_submit2)(self.gear.core.queue, 1, &raw const info, frame.fence)
        };
        ok("vkQueueSubmit2", result)
    }

    /// Queues the image for display; the result of the present when it is a usable one.
    fn present(&self, index: u32) -> Result<i32, VulkanError> {
        let slot = usize::try_from(index).unwrap_or(usize::MAX);
        let Some((swapchain, done)) = self.swapchain.as_ref().and_then(|swapchain| {
            swapchain
                .render_done(slot)
                .map(|done| (swapchain.handle, done))
        }) else {
            return Err(VulkanError::Unsupported(
                "the acquired image does not exist",
            ));
        };
        let info = PresentInfo {
            s_type: STYPE_PRESENT_INFO,
            p_next: core::ptr::null(),
            wait_semaphore_count: 1,
            wait_semaphores: &raw const done,
            swapchain_count: 1,
            swapchains: &raw const swapchain,
            image_indices: &raw const index,
            results: core::ptr::null_mut(),
        };
        // SAFETY: `info` and what it points to live until the call returns; the semaphore is
        // signalled by the submit just made.
        let result =
            unsafe { (self.gear.fns.queue_present)(self.gear.core.queue, &raw const info) };
        match result {
            SUCCESS | SUBOPTIMAL | ERROR_OUT_OF_DATE => Ok(result),
            ERROR_SURFACE_LOST => Err(VulkanError::SurfaceLost),
            result => Err(VulkanError::Call {
                call: "vkQueuePresentKHR",
                result,
            }),
        }
    }

    /// Starts copying the middle pixel of every frame into a small buffer, for
    /// [`Presenter::captured_pixel`]. A test aid; the allocator of card 6-8 replaces the buffer.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] when the window has no swapchain or its images cannot be copied
    /// from, or the device has no suitable memory; [`VulkanError::Call`] when the driver refuses.
    pub fn enable_capture(&mut self) -> Result<(), VulkanError> {
        let copyable = self
            .swapchain
            .as_ref()
            .is_some_and(|swapchain| swapchain.usage & IMAGE_USAGE_TRANSFER_SRC != 0);
        if !copyable {
            return Err(VulkanError::Unsupported(
                "the swapchain images cannot be copied from",
            ));
        }
        if self.capture.is_none() {
            self.capture = Some(Capture::create(&self.gear)?);
        }
        Ok(())
    }

    /// The middle pixel of the last presented frame as blue, green, red, alpha bytes. Waits for
    /// the device to be idle first.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] without [`Presenter::enable_capture`]; [`VulkanError::Call`]
    /// when the wait fails.
    pub fn captured_pixel(&self) -> Result<[u8; 4], VulkanError> {
        let capture = self
            .capture
            .as_ref()
            .ok_or(VulkanError::Unsupported("capture was not enabled"))?;
        self.gear.core.device.wait_idle()?;
        Ok(capture.read())
    }
}

impl Drop for Presenter {
    fn drop(&mut self) {
        // Work still running would use what is destroyed next; the result does not change what
        // happens.
        let _ = self.gear.core.device.wait_idle();
        if let Some(mut capture) = self.capture.take() {
            capture.destroy();
        }
        for frame in &mut self.frames {
            frame.destroy(&self.gear);
        }
        if let Some(mut swapchain) = self.swapchain.take() {
            swapchain.destroy(&self.gear);
        }
        // The surface is destroyed by its own drop, after the swapchain, as the fields drop.
    }
}
