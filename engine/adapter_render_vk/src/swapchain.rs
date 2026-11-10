//! A swapchain with its image views and one render-finished semaphore per image.
#![expect(
    unsafe_code,
    reason = "the swapchain is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::error::{VulkanError, check_created};
use crate::gear::{Gear, ok};
use crate::plan::Plan;
use crate::surface::Surface;
use crate::vk::SUCCESS;
use crate::vk_present::{
    Extent2D, IMAGE_VIEW_TYPE_2D, ImageHandle, ImageViewCreateInfo, ImageViewHandle,
    PRESENT_MODE_FIFO, SHARING_EXCLUSIVE, STYPE_IMAGE_VIEW_CREATE_INFO,
    STYPE_SEMAPHORE_CREATE_INFO, STYPE_SWAPCHAIN_CREATE_INFO, SemaphoreHandle, SubresourceRange,
    SwapchainCreateInfo, SwapchainHandle, SyncCreateInfo,
};

/// More images than any swapchain has; a larger count is cut here.
const MAX_IMAGES: u32 = 64;

/// The swapchain and what is made per image. Not `Drop`: the presenter destroys it with
/// [`Swapchain::destroy`] because the functions live in the [`Gear`].
pub struct Swapchain {
    pub handle: SwapchainHandle,
    pub extent: Extent2D,
    pub usage: u32,
    pub images: Vec<ImageHandle>,
    views: Vec<ImageViewHandle>,
    render_done: Vec<SemaphoreHandle>,
}

impl Swapchain {
    /// Makes the swapchain of `plan` for `surface`, handing `old` (zero for none) over. Whatever
    /// was made before an error is destroyed. `old` is retired by the call even when it fails, so
    /// the caller must not use it again (only destroy it).
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result.
    pub fn create(
        gear: &Gear,
        surface: &Surface,
        plan: &Plan,
        old: SwapchainHandle,
    ) -> Result<Self, VulkanError> {
        let info = SwapchainCreateInfo {
            s_type: STYPE_SWAPCHAIN_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            surface: surface.handle(),
            min_image_count: plan.image_count,
            image_format: plan.format.format,
            image_color_space: plan.format.color_space,
            image_extent: plan.extent,
            image_array_layers: 1,
            image_usage: plan.usage,
            image_sharing_mode: SHARING_EXCLUSIVE,
            queue_family_index_count: 0,
            queue_family_indices: core::ptr::null(),
            pre_transform: plan.transform,
            composite_alpha: plan.alpha,
            present_mode: PRESENT_MODE_FIFO,
            clipped: 1,
            old_swapchain: old,
        };
        let mut handle: SwapchainHandle = 0;
        // SAFETY: `info` is valid for the call; the surface is live; `handle` is a valid output.
        let result = unsafe {
            (gear.fns.create_swapchain)(
                gear.device,
                &raw const info,
                core::ptr::null(),
                &raw mut handle,
            )
        };
        check_created("vkCreateSwapchainKHR", result, handle == 0)?;
        let mut swapchain = Self {
            handle,
            extent: plan.extent,
            usage: plan.usage,
            images: Vec::new(),
            views: Vec::new(),
            render_done: Vec::new(),
        };
        if let Err(error) = swapchain.fill(gear, plan) {
            swapchain.destroy(gear);
            return Err(error);
        }
        Ok(swapchain)
    }

    /// Fetches the images and makes a view and a semaphore for each.
    fn fill(&mut self, gear: &Gear, plan: &Plan) -> Result<(), VulkanError> {
        self.images = self.fetch_images(gear)?;
        for image in self.images.clone() {
            let view = make_view(gear, image, plan.format.format)?;
            self.views.push(view);
            let done = make_semaphore(gear)?;
            self.render_done.push(done);
        }
        Ok(())
    }

    fn fetch_images(&self, gear: &Gear) -> Result<Vec<ImageHandle>, VulkanError> {
        let call = "vkGetSwapchainImagesKHR";
        let mut count = 0_u32;
        // SAFETY: a null array asks for the count only.
        let result = unsafe {
            (gear.fns.get_swapchain_images)(
                gear.device,
                self.handle,
                &raw mut count,
                core::ptr::null_mut(),
            )
        };
        ok(call, result)?;
        count = count.min(MAX_IMAGES);
        let room = usize::try_from(count).unwrap_or(0);
        let mut images = vec![0; room];
        // SAFETY: `images` holds `count` entries, which the driver fills (lowering `count`).
        let result = unsafe {
            (gear.fns.get_swapchain_images)(
                gear.device,
                self.handle,
                &raw mut count,
                images.as_mut_ptr(),
            )
        };
        if result < SUCCESS {
            return Err(VulkanError::Call { call, result });
        }
        images.truncate(usize::try_from(count).unwrap_or(0).min(room));
        Ok(images)
    }

    /// The view of image `index`.
    pub fn view(&self, index: usize) -> Option<ImageViewHandle> {
        self.views.get(index).copied()
    }

    /// The semaphore that image `index`'s rendering signals and its present waits for.
    pub fn render_done(&self, index: usize) -> Option<SemaphoreHandle> {
        self.render_done.get(index).copied()
    }

    /// Destroys the semaphores and views, then the swapchain. The device must not be using any
    /// of them (the presenter waits for it to be idle first).
    pub fn destroy(&mut self, gear: &Gear) {
        for semaphore in self.render_done.drain(..) {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (gear.fns.destroy_semaphore)(gear.device, semaphore, core::ptr::null()) };
        }
        for view in self.views.drain(..) {
            // SAFETY: made on this device and destroyed once, here, before the swapchain.
            unsafe { (gear.fns.destroy_image_view)(gear.device, view, core::ptr::null()) };
        }
        self.images.clear();
        if self.handle != 0 {
            // SAFETY: made on this device and destroyed once, here, after its views.
            unsafe { (gear.fns.destroy_swapchain)(gear.device, self.handle, core::ptr::null()) };
            self.handle = 0;
        }
    }
}

fn make_view(gear: &Gear, image: ImageHandle, format: u32) -> Result<ImageViewHandle, VulkanError> {
    let info = ImageViewCreateInfo {
        s_type: STYPE_IMAGE_VIEW_CREATE_INFO,
        p_next: core::ptr::null(),
        flags: 0,
        image,
        view_type: IMAGE_VIEW_TYPE_2D,
        format,
        components: [0; 4],
        subresource_range: SubresourceRange::COLOR,
    };
    let mut view: ImageViewHandle = 0;
    // SAFETY: `info` is valid for the call; the image belongs to a live swapchain; `view` is a
    // valid output.
    let result = unsafe {
        (gear.fns.create_image_view)(
            gear.device,
            &raw const info,
            core::ptr::null(),
            &raw mut view,
        )
    };
    check_created("vkCreateImageView", result, view == 0)?;
    Ok(view)
}

/// Makes a semaphore (shared with the frames).
pub fn make_semaphore(gear: &Gear) -> Result<SemaphoreHandle, VulkanError> {
    let info = SyncCreateInfo {
        s_type: STYPE_SEMAPHORE_CREATE_INFO,
        p_next: core::ptr::null(),
        flags: 0,
    };
    let mut semaphore: SemaphoreHandle = 0;
    // SAFETY: `info` is valid for the call; `semaphore` is a valid output.
    let result = unsafe {
        (gear.fns.create_semaphore)(
            gear.device,
            &raw const info,
            core::ptr::null(),
            &raw mut semaphore,
        )
    };
    check_created("vkCreateSemaphore", result, semaphore == 0)?;
    Ok(semaphore)
}
