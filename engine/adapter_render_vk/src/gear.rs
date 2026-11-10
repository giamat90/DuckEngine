//! The device functions presenting needs, and the shared handle bundle the presenter's parts use.
#![expect(
    unsafe_code,
    reason = "the functions are a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::entry::missing;
use crate::error::VulkanError;
use crate::vk::{Bool32, DeviceHandle, QueueHandle, SUCCESS, VkResult};
use crate::vk_draw::{
    DescriptorPoolCreateInfo, DescriptorSetAllocateInfo, DescriptorSetLayoutCreateInfo,
    GraphicsPipelineCreateInfo, Handle, PipelineLayoutCreateInfo, ShaderModuleCreateInfo, Viewport,
    WriteDescriptorSet,
};
use crate::vk_present::Rect2D;
use crate::vk_present::{
    BufferCreateInfo, BufferHandle, BufferImageCopy, CommandBufferAllocateInfo,
    CommandBufferBeginInfo, CommandBufferHandle, CommandPoolCreateInfo, CommandPoolHandle,
    DependencyInfo, FenceHandle, ImageHandle, ImageViewCreateInfo, ImageViewHandle,
    MemoryAllocateInfo, MemoryHandle, MemoryRequirements, PresentInfo, RenderingInfo,
    SemaphoreHandle, SubmitInfo2, SwapchainCreateInfo, SwapchainHandle, SyncCreateInfo,
};
use crate::vk_resource::{BufferCopy, ImageCreateInfo, SamplerCreateInfo, SamplerHandle};
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

function_table! {
    /// The device functions of the memory sub-allocator.
    pub struct MemoryFns {
        allocate: unsafe extern "system" fn(DeviceHandle, *const MemoryAllocateInfo, *const c_void, *mut MemoryHandle) -> VkResult = c"vkAllocateMemory",
        free: unsafe extern "system" fn(DeviceHandle, MemoryHandle, *const c_void) = c"vkFreeMemory",
        map: unsafe extern "system" fn(DeviceHandle, MemoryHandle, u64, u64, u32, *mut *mut c_void) -> VkResult = c"vkMapMemory",
        unmap: unsafe extern "system" fn(DeviceHandle, MemoryHandle) = c"vkUnmapMemory",
    }
}

function_table! {
    /// The device functions of buffers, images, samplers and the upload commands. None of them
    /// needs the swapchain extension, so a device that cannot present can still hold resources.
    pub struct ResourceFns {
        create_buffer: unsafe extern "system" fn(DeviceHandle, *const BufferCreateInfo, *const c_void, *mut BufferHandle) -> VkResult = c"vkCreateBuffer",
        destroy_buffer: unsafe extern "system" fn(DeviceHandle, BufferHandle, *const c_void) = c"vkDestroyBuffer",
        get_buffer_memory_requirements: unsafe extern "system" fn(DeviceHandle, BufferHandle, *mut MemoryRequirements) = c"vkGetBufferMemoryRequirements",
        bind_buffer_memory: unsafe extern "system" fn(DeviceHandle, BufferHandle, MemoryHandle, u64) -> VkResult = c"vkBindBufferMemory",
        create_image: unsafe extern "system" fn(DeviceHandle, *const ImageCreateInfo, *const c_void, *mut ImageHandle) -> VkResult = c"vkCreateImage",
        destroy_image: unsafe extern "system" fn(DeviceHandle, ImageHandle, *const c_void) = c"vkDestroyImage",
        get_image_memory_requirements: unsafe extern "system" fn(DeviceHandle, ImageHandle, *mut MemoryRequirements) = c"vkGetImageMemoryRequirements",
        bind_image_memory: unsafe extern "system" fn(DeviceHandle, ImageHandle, MemoryHandle, u64) -> VkResult = c"vkBindImageMemory",
        create_image_view: unsafe extern "system" fn(DeviceHandle, *const ImageViewCreateInfo, *const c_void, *mut ImageViewHandle) -> VkResult = c"vkCreateImageView",
        destroy_image_view: unsafe extern "system" fn(DeviceHandle, ImageViewHandle, *const c_void) = c"vkDestroyImageView",
        create_sampler: unsafe extern "system" fn(DeviceHandle, *const SamplerCreateInfo, *const c_void, *mut SamplerHandle) -> VkResult = c"vkCreateSampler",
        destroy_sampler: unsafe extern "system" fn(DeviceHandle, SamplerHandle, *const c_void) = c"vkDestroySampler",
        create_command_pool: unsafe extern "system" fn(DeviceHandle, *const CommandPoolCreateInfo, *const c_void, *mut CommandPoolHandle) -> VkResult = c"vkCreateCommandPool",
        destroy_command_pool: unsafe extern "system" fn(DeviceHandle, CommandPoolHandle, *const c_void) = c"vkDestroyCommandPool",
        reset_command_pool: unsafe extern "system" fn(DeviceHandle, CommandPoolHandle, u32) -> VkResult = c"vkResetCommandPool",
        allocate_command_buffers: unsafe extern "system" fn(DeviceHandle, *const CommandBufferAllocateInfo, *mut CommandBufferHandle) -> VkResult = c"vkAllocateCommandBuffers",
        begin_command_buffer: unsafe extern "system" fn(CommandBufferHandle, *const CommandBufferBeginInfo) -> VkResult = c"vkBeginCommandBuffer",
        end_command_buffer: unsafe extern "system" fn(CommandBufferHandle) -> VkResult = c"vkEndCommandBuffer",
        create_fence: unsafe extern "system" fn(DeviceHandle, *const SyncCreateInfo, *const c_void, *mut FenceHandle) -> VkResult = c"vkCreateFence",
        destroy_fence: unsafe extern "system" fn(DeviceHandle, FenceHandle, *const c_void) = c"vkDestroyFence",
        wait_for_fences: unsafe extern "system" fn(DeviceHandle, u32, *const FenceHandle, Bool32, u64) -> VkResult = c"vkWaitForFences",
        reset_fences: unsafe extern "system" fn(DeviceHandle, u32, *const FenceHandle) -> VkResult = c"vkResetFences",
        queue_submit2: unsafe extern "system" fn(QueueHandle, u32, *const SubmitInfo2, FenceHandle) -> VkResult = c"vkQueueSubmit2",
        cmd_pipeline_barrier2: unsafe extern "system" fn(CommandBufferHandle, *const DependencyInfo) = c"vkCmdPipelineBarrier2",
        cmd_copy_buffer: unsafe extern "system" fn(CommandBufferHandle, BufferHandle, BufferHandle, u32, *const BufferCopy) = c"vkCmdCopyBuffer",
        cmd_copy_buffer_to_image: unsafe extern "system" fn(CommandBufferHandle, BufferHandle, ImageHandle, u32, u32, *const BufferImageCopy) = c"vkCmdCopyBufferToImage",
        cmd_copy_image_to_buffer: unsafe extern "system" fn(CommandBufferHandle, ImageHandle, u32, BufferHandle, u32, *const BufferImageCopy) = c"vkCmdCopyImageToBuffer",
    }
}

function_table! {
    /// The device functions of shaders, pipelines, descriptors and draw commands.
    pub struct DrawFns {
        create_shader_module: unsafe extern "system" fn(DeviceHandle, *const ShaderModuleCreateInfo, *const c_void, *mut Handle) -> VkResult = c"vkCreateShaderModule",
        destroy_shader_module: unsafe extern "system" fn(DeviceHandle, Handle, *const c_void) = c"vkDestroyShaderModule",
        create_descriptor_set_layout: unsafe extern "system" fn(DeviceHandle, *const DescriptorSetLayoutCreateInfo, *const c_void, *mut Handle) -> VkResult = c"vkCreateDescriptorSetLayout",
        destroy_descriptor_set_layout: unsafe extern "system" fn(DeviceHandle, Handle, *const c_void) = c"vkDestroyDescriptorSetLayout",
        create_pipeline_layout: unsafe extern "system" fn(DeviceHandle, *const PipelineLayoutCreateInfo, *const c_void, *mut Handle) -> VkResult = c"vkCreatePipelineLayout",
        destroy_pipeline_layout: unsafe extern "system" fn(DeviceHandle, Handle, *const c_void) = c"vkDestroyPipelineLayout",
        create_descriptor_pool: unsafe extern "system" fn(DeviceHandle, *const DescriptorPoolCreateInfo, *const c_void, *mut Handle) -> VkResult = c"vkCreateDescriptorPool",
        destroy_descriptor_pool: unsafe extern "system" fn(DeviceHandle, Handle, *const c_void) = c"vkDestroyDescriptorPool",
        reset_descriptor_pool: unsafe extern "system" fn(DeviceHandle, Handle, u32) -> VkResult = c"vkResetDescriptorPool",
        allocate_descriptor_sets: unsafe extern "system" fn(DeviceHandle, *const DescriptorSetAllocateInfo, *mut Handle) -> VkResult = c"vkAllocateDescriptorSets",
        update_descriptor_sets: unsafe extern "system" fn(DeviceHandle, u32, *const WriteDescriptorSet, u32, *const c_void) = c"vkUpdateDescriptorSets",
        create_graphics_pipelines: unsafe extern "system" fn(DeviceHandle, Handle, u32, *const GraphicsPipelineCreateInfo, *const c_void, *mut Handle) -> VkResult = c"vkCreateGraphicsPipelines",
        destroy_pipeline: unsafe extern "system" fn(DeviceHandle, Handle, *const c_void) = c"vkDestroyPipeline",
        cmd_bind_pipeline: unsafe extern "system" fn(CommandBufferHandle, u32, Handle) = c"vkCmdBindPipeline",
        cmd_bind_descriptor_sets: unsafe extern "system" fn(CommandBufferHandle, u32, Handle, u32, u32, *const Handle, u32, *const u32) = c"vkCmdBindDescriptorSets",
        cmd_bind_vertex_buffers: unsafe extern "system" fn(CommandBufferHandle, u32, u32, *const BufferHandle, *const u64) = c"vkCmdBindVertexBuffers",
        cmd_bind_index_buffer: unsafe extern "system" fn(CommandBufferHandle, BufferHandle, u64, u32) = c"vkCmdBindIndexBuffer",
        cmd_set_viewport: unsafe extern "system" fn(CommandBufferHandle, u32, u32, *const Viewport) = c"vkCmdSetViewport",
        cmd_set_scissor: unsafe extern "system" fn(CommandBufferHandle, u32, u32, *const Rect2D) = c"vkCmdSetScissor",
        cmd_draw: unsafe extern "system" fn(CommandBufferHandle, u32, u32, u32, u32) = c"vkCmdDraw",
        cmd_draw_indexed: unsafe extern "system" fn(CommandBufferHandle, u32, u32, u32, i32, u32) = c"vkCmdDrawIndexed",
    }
}

/// Loads the memory functions of the core's device.
///
/// # Errors
/// [`VulkanError::MissingFunction`] naming the first function the driver lacks.
pub fn load_memory_fns(core: &Core) -> Result<MemoryFns, VulkanError> {
    let device = core.device.handle();
    let lookup = |name: &CStr| {
        // SAFETY: `device` is the live device of `core`.
        unsafe { core.instance.device_proc_addr(device, name) }
    };
    // SAFETY: the driver returns functions with the declared signatures for these names.
    unsafe { MemoryFns::load(lookup) }.map_err(missing)
}

/// Loads the shader, pipeline, descriptor and draw functions of the core's device.
///
/// # Errors
/// [`VulkanError::MissingFunction`] naming the first function the driver lacks.
pub fn load_draw_fns(core: &Core) -> Result<DrawFns, VulkanError> {
    let device = core.device.handle();
    let lookup = |name: &CStr| {
        // SAFETY: `device` is the live device of `core`.
        unsafe { core.instance.device_proc_addr(device, name) }
    };
    // SAFETY: the driver returns functions with the declared signatures for these names.
    unsafe { DrawFns::load(lookup) }.map_err(missing)
}

/// Loads the resource functions of the core's device.
///
/// # Errors
/// [`VulkanError::MissingFunction`] naming the first function the driver lacks.
pub fn load_resource_fns(core: &Core) -> Result<ResourceFns, VulkanError> {
    let device = core.device.handle();
    let lookup = |name: &CStr| {
        // SAFETY: `device` is the live device of `core`.
        unsafe { core.instance.device_proc_addr(device, name) }
    };
    // SAFETY: the driver returns functions with the declared signatures for these names.
    unsafe { ResourceFns::load(lookup) }.map_err(missing)
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
