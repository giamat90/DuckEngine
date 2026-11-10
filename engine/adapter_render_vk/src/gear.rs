//! The device functions presenting needs, and the shared handle bundle the presenter's parts use.
#![expect(
    unsafe_code,
    reason = "the functions are a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::entry::missing;
use crate::error::VulkanError;
use crate::vk::{Bool32, DeviceHandle, QueueHandle, SUCCESS, VkResult};
use crate::vk_present::{
    BufferCreateInfo, BufferHandle, BufferImageCopy, CommandBufferAllocateInfo,
    CommandBufferBeginInfo, CommandBufferHandle, CommandPoolCreateInfo, CommandPoolHandle,
    DependencyInfo, FenceHandle, ImageHandle, ImageViewCreateInfo, ImageViewHandle,
    MemoryAllocateInfo, MemoryHandle, MemoryRequirements, PresentInfo, RenderingInfo,
    SemaphoreHandle, SubmitInfo2, SwapchainCreateInfo, SwapchainHandle, SyncCreateInfo,
};
use core::ffi::{CStr, c_void};
use std::rc::Rc;

function_table! {
    /// The device functions of swapchains, frames and commands.
    pub struct PresentFns {
        create_swapchain: unsafe extern "system" fn(DeviceHandle, *const SwapchainCreateInfo, *const c_void, *mut SwapchainHandle) -> VkResult = c"vkCreateSwapchainKHR",
        destroy_swapchain: unsafe extern "system" fn(DeviceHandle, SwapchainHandle, *const c_void) = c"vkDestroySwapchainKHR",
        get_swapchain_images: unsafe extern "system" fn(DeviceHandle, SwapchainHandle, *mut u32, *mut ImageHandle) -> VkResult = c"vkGetSwapchainImagesKHR",
        acquire_next_image: unsafe extern "system" fn(DeviceHandle, SwapchainHandle, u64, SemaphoreHandle, FenceHandle, *mut u32) -> VkResult = c"vkAcquireNextImageKHR",
        queue_present: unsafe extern "system" fn(QueueHandle, *const PresentInfo) -> VkResult = c"vkQueuePresentKHR",
        create_semaphore: unsafe extern "system" fn(DeviceHandle, *const SyncCreateInfo, *const c_void, *mut SemaphoreHandle) -> VkResult = c"vkCreateSemaphore",
        destroy_semaphore: unsafe extern "system" fn(DeviceHandle, SemaphoreHandle, *const c_void) = c"vkDestroySemaphore",
        create_fence: unsafe extern "system" fn(DeviceHandle, *const SyncCreateInfo, *const c_void, *mut FenceHandle) -> VkResult = c"vkCreateFence",
        destroy_fence: unsafe extern "system" fn(DeviceHandle, FenceHandle, *const c_void) = c"vkDestroyFence",
        wait_for_fences: unsafe extern "system" fn(DeviceHandle, u32, *const FenceHandle, Bool32, u64) -> VkResult = c"vkWaitForFences",
        reset_fences: unsafe extern "system" fn(DeviceHandle, u32, *const FenceHandle) -> VkResult = c"vkResetFences",
        create_command_pool: unsafe extern "system" fn(DeviceHandle, *const CommandPoolCreateInfo, *const c_void, *mut CommandPoolHandle) -> VkResult = c"vkCreateCommandPool",
        destroy_command_pool: unsafe extern "system" fn(DeviceHandle, CommandPoolHandle, *const c_void) = c"vkDestroyCommandPool",
        reset_command_pool: unsafe extern "system" fn(DeviceHandle, CommandPoolHandle, u32) -> VkResult = c"vkResetCommandPool",
        allocate_command_buffers: unsafe extern "system" fn(DeviceHandle, *const CommandBufferAllocateInfo, *mut CommandBufferHandle) -> VkResult = c"vkAllocateCommandBuffers",
        begin_command_buffer: unsafe extern "system" fn(CommandBufferHandle, *const CommandBufferBeginInfo) -> VkResult = c"vkBeginCommandBuffer",
        end_command_buffer: unsafe extern "system" fn(CommandBufferHandle) -> VkResult = c"vkEndCommandBuffer",
        create_image_view: unsafe extern "system" fn(DeviceHandle, *const ImageViewCreateInfo, *const c_void, *mut ImageViewHandle) -> VkResult = c"vkCreateImageView",
        destroy_image_view: unsafe extern "system" fn(DeviceHandle, ImageViewHandle, *const c_void) = c"vkDestroyImageView",
        cmd_pipeline_barrier2: unsafe extern "system" fn(CommandBufferHandle, *const DependencyInfo) = c"vkCmdPipelineBarrier2",
        cmd_begin_rendering: unsafe extern "system" fn(CommandBufferHandle, *const RenderingInfo) = c"vkCmdBeginRendering",
        cmd_end_rendering: unsafe extern "system" fn(CommandBufferHandle) = c"vkCmdEndRendering",
        queue_submit2: unsafe extern "system" fn(QueueHandle, u32, *const SubmitInfo2, FenceHandle) -> VkResult = c"vkQueueSubmit2",
    }
}

function_table! {
    /// The device functions of the one-off readback buffer (replaced by the allocator of 6-8).
    pub struct CaptureFns {
        create_buffer: unsafe extern "system" fn(DeviceHandle, *const BufferCreateInfo, *const c_void, *mut BufferHandle) -> VkResult = c"vkCreateBuffer",
        destroy_buffer: unsafe extern "system" fn(DeviceHandle, BufferHandle, *const c_void) = c"vkDestroyBuffer",
        get_buffer_memory_requirements: unsafe extern "system" fn(DeviceHandle, BufferHandle, *mut MemoryRequirements) = c"vkGetBufferMemoryRequirements",
        allocate_memory: unsafe extern "system" fn(DeviceHandle, *const MemoryAllocateInfo, *const c_void, *mut MemoryHandle) -> VkResult = c"vkAllocateMemory",
        free_memory: unsafe extern "system" fn(DeviceHandle, MemoryHandle, *const c_void) = c"vkFreeMemory",
        bind_buffer_memory: unsafe extern "system" fn(DeviceHandle, BufferHandle, MemoryHandle, u64) -> VkResult = c"vkBindBufferMemory",
        map_memory: unsafe extern "system" fn(DeviceHandle, MemoryHandle, u64, u64, u32, *mut *mut c_void) -> VkResult = c"vkMapMemory",
        unmap_memory: unsafe extern "system" fn(DeviceHandle, MemoryHandle) = c"vkUnmapMemory",
        cmd_copy_image_to_buffer: unsafe extern "system" fn(CommandBufferHandle, ImageHandle, u32, BufferHandle, u32, *const BufferImageCopy) = c"vkCmdCopyImageToBuffer",
    }
}

/// `Ok` for `VK_SUCCESS`, else the call and its result as an error value.
pub fn ok(call: &'static str, result: VkResult) -> Result<(), VulkanError> {
    if result == SUCCESS {
        Ok(())
    } else {
        Err(VulkanError::Call { call, result })
    }
}

/// The core plus the device functions: what each part of the presenter calls Vulkan through.
pub struct Gear {
    pub core: Rc<Core>,
    pub fns: PresentFns,
    pub device: DeviceHandle,
}

impl Gear {
    /// Loads the presenting functions of the core's device.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] naming the first function the driver lacks.
    pub fn load(core: &Rc<Core>) -> Result<Self, VulkanError> {
        let device = core.device.handle();
        let lookup = |name: &CStr| {
            // SAFETY: `device` is the live device of `core`.
            unsafe { core.instance.device_proc_addr(device, name) }
        };
        // SAFETY: the driver returns functions with the declared signatures for these names.
        let fns = unsafe { PresentFns::load(lookup) }.map_err(missing)?;
        Ok(Self {
            core: Rc::clone(core),
            fns,
            device,
        })
    }

    /// The readback functions, loaded when a capture is asked for.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] naming the first function the driver lacks.
    pub fn capture_fns(&self) -> Result<CaptureFns, VulkanError> {
        let lookup = |name: &CStr| {
            // SAFETY: `self.device` is the live device of the core.
            unsafe { self.core.instance.device_proc_addr(self.device, name) }
        };
        // SAFETY: the driver returns functions with the declared signatures for these names.
        unsafe { CaptureFns::load(lookup) }.map_err(missing)
    }
}
