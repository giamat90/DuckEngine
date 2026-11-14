//! Staging and one-shot transfer submits: the way bytes reach device-local buffers and images
//! and the way an image is read back (card 6-8).
//!
//! The uploader owns a host-visible staging buffer (grown when a transfer needs more), one command
//! pool, one command buffer and one fence. A transfer records its commands, submits them and waits
//! for the fence, so when it returns the work is finished and the staging buffer can be reused.
//! This is simple on purpose: the port writes resources between frames, not inside them, and a
//! batched path can replace the wait later without changing the callers.
#![expect(
    unsafe_code,
    reason = "staging and transfer commands are a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::error::{VulkanError, check_created};
use crate::gear::{ResourceFns, ok};
use crate::memory::{Allocation, Kind, MemoryManager, Placement};
use crate::vk::{DeviceHandle, QueueHandle};
use crate::vk_present::{
    ACCESS_NONE, BufferCreateInfo, BufferHandle, BufferImageCopy, COMMAND_BUFFER_LEVEL_PRIMARY,
    COMMAND_BUFFER_ONE_TIME_SUBMIT, COMMAND_POOL_TRANSIENT, CommandBufferAllocateInfo,
    CommandBufferBeginInfo, CommandBufferHandle, CommandBufferSubmitInfo, CommandPoolCreateInfo,
    CommandPoolHandle, DependencyInfo, FENCE_CREATE_SIGNALED, FOREVER, FenceHandle, ImageHandle,
    ImageMemoryBarrier2, MemoryRequirements, QUEUE_FAMILY_IGNORED, SHARING_EXCLUSIVE,
    STYPE_BUFFER_CREATE_INFO, STYPE_COMMAND_BUFFER_ALLOCATE_INFO, STYPE_COMMAND_BUFFER_BEGIN_INFO,
    STYPE_COMMAND_BUFFER_SUBMIT_INFO, STYPE_COMMAND_POOL_CREATE_INFO, STYPE_DEPENDENCY_INFO,
    STYPE_FENCE_CREATE_INFO, STYPE_IMAGE_MEMORY_BARRIER_2, STYPE_SUBMIT_INFO_2, SubmitInfo2,
    SubresourceLayers, SubresourceRange, SyncCreateInfo,
};
use crate::vk_resource::{
    ACCESS_MEMORY_READ, ACCESS_MEMORY_WRITE, BUFFER_USAGE_TRANSFER_DST, BUFFER_USAGE_TRANSFER_SRC,
    BufferCopy, MemoryBarrier2, STAGE_ALL_COMMANDS, STYPE_MEMORY_BARRIER_2,
};
use core::ptr::NonNull;
use std::rc::Rc;

/// The smallest staging buffer: 64 KiB.
const MIN_STAGING: u64 = 64 * 1024;

/// A pipeline stage and the access made in it: one side of a barrier.
pub type StageAccess = (u64, u64);

/// Everything: wait for all earlier work, and make all later work wait. The barrier of a transfer
/// that may touch any resource.
pub const EVERYTHING: StageAccess = (STAGE_ALL_COMMANDS, ACCESS_MEMORY_READ | ACCESS_MEMORY_WRITE);

/// No earlier work to wait for: the first use of an image.
pub const NOTHING: StageAccess = (STAGE_ALL_COMMANDS, ACCESS_NONE);

/// One layout change of an image, with the stage and access on each side.
pub struct ImageTransition {
    pub image: ImageHandle,
    pub range: SubresourceRange,
    pub old_layout: u32,
    pub new_layout: u32,
    pub src: StageAccess,
    pub dst: StageAccess,
}

/// The commands of one transfer being recorded.
pub struct Recorder<'a> {
    fns: &'a ResourceFns,
    cmd: CommandBufferHandle,
}

impl Recorder<'_> {
    /// A barrier on all memory.
    pub fn memory_barrier(&self, src: StageAccess, dst: StageAccess) {
        let barrier = MemoryBarrier2 {
            s_type: STYPE_MEMORY_BARRIER_2,
            p_next: core::ptr::null(),
            src_stage_mask: src.0,
            src_access_mask: src.1,
            dst_stage_mask: dst.0,
            dst_access_mask: dst.1,
        };
        let info = DependencyInfo {
            s_type: STYPE_DEPENDENCY_INFO,
            p_next: core::ptr::null(),
            dependency_flags: 0,
            memory_barrier_count: 1,
            memory_barriers: (&raw const barrier).cast(),
            buffer_memory_barrier_count: 0,
            buffer_memory_barriers: core::ptr::null(),
            image_memory_barrier_count: 0,
            image_memory_barriers: core::ptr::null(),
        };
        // SAFETY: the command buffer is recording; `info` and `barrier` outlive the call.
        unsafe { (self.fns.cmd_pipeline_barrier2)(self.cmd, &raw const info) };
    }

    /// A layout change of an image.
    pub fn image_barrier(&self, change: &ImageTransition) {
        let barrier = ImageMemoryBarrier2 {
            s_type: STYPE_IMAGE_MEMORY_BARRIER_2,
            p_next: core::ptr::null(),
            src_stage_mask: change.src.0,
            src_access_mask: change.src.1,
            dst_stage_mask: change.dst.0,
            dst_access_mask: change.dst.1,
            old_layout: change.old_layout,
            new_layout: change.new_layout,
            src_queue_family_index: QUEUE_FAMILY_IGNORED,
            dst_queue_family_index: QUEUE_FAMILY_IGNORED,
            image: change.image,
            subresource_range: change.range,
        };
        let info = DependencyInfo {
            s_type: STYPE_DEPENDENCY_INFO,
            p_next: core::ptr::null(),
            dependency_flags: 0,
            memory_barrier_count: 0,
            memory_barriers: core::ptr::null(),
            buffer_memory_barrier_count: 0,
            buffer_memory_barriers: core::ptr::null(),
            image_memory_barrier_count: 1,
            image_memory_barriers: &raw const barrier,
        };
        // SAFETY: the command buffer is recording; `info` and `barrier` outlive the call.
        unsafe { (self.fns.cmd_pipeline_barrier2)(self.cmd, &raw const info) };
    }

    /// Copies `size` bytes between buffers.
    pub fn copy_buffer(&self, source: BufferHandle, target: BufferHandle, region: BufferCopy) {
        // SAFETY: the command buffer is recording; the caller checked the ranges.
        unsafe { (self.fns.cmd_copy_buffer)(self.cmd, source, target, 1, &raw const region) };
    }

    /// Copies a buffer into an image that is in the transfer-destination layout.
    pub fn copy_buffer_to_image(
        &self,
        source: BufferHandle,
        image: ImageHandle,
        region: &BufferImageCopy,
    ) {
        // SAFETY: the command buffer is recording; the caller checked the region.
        unsafe {
            (self.fns.cmd_copy_buffer_to_image)(
                self.cmd,
                source,
                image,
                crate::vk_resource::LAYOUT_TRANSFER_DST,
                1,
                region,
            );
        }
    }

    /// Copies an image that is in the transfer-source layout into a buffer.
    pub fn copy_image_to_buffer(
        &self,
        image: ImageHandle,
        target: BufferHandle,
        region: &BufferImageCopy,
    ) {
        // SAFETY: the command buffer is recording; the caller checked the region.
        unsafe {
            (self.fns.cmd_copy_image_to_buffer)(
                self.cmd,
                image,
                crate::vk_present::LAYOUT_TRANSFER_SRC,
                target,
                1,
                region,
            );
        }
    }
}

/// The region of an image level 0 in a tightly packed buffer.
#[must_use]
pub fn tight_region(aspect: u32, x: i32, y: i32, width: u32, height: u32) -> BufferImageCopy {
    BufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image_subresource: SubresourceLayers {
            aspect_mask: aspect,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        image_offset: [x, y, 0],
        image_extent: [width, height, 1],
    }
}

/// The staging buffer and its memory.
struct Staging {
    buffer: BufferHandle,
    allocation: Allocation,
    capacity: u64,
}

/// Staging memory and the objects a one-shot transfer submit needs.
pub struct Uploader {
    _core: Rc<Core>,
    fns: ResourceFns,
    device: DeviceHandle,
    queue: QueueHandle,
    pool: CommandPoolHandle,
    cmd: CommandBufferHandle,
    fence: FenceHandle,
    staging: Option<Staging>,
    /// Work was submitted and its fence has not been seen signalled.
    pending: bool,
}

impl Uploader {
    /// Makes the pool, the command buffer and the fence (signalled). Whatever was made before an error is destroyed.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver result.
    pub fn new(core: &Rc<Core>, fns: ResourceFns) -> Result<Self, VulkanError> {
        let mut uploader = Self {
            _core: Rc::clone(core),
            fns,
            device: core.device.handle(),
            queue: core.queue,
            pool: 0,
            cmd: CommandBufferHandle::NULL,
            fence: 0,
            staging: None,
            pending: false,
        };
        if let Err(error) = uploader.make_objects(core.queue_family) {
            uploader.destroy_objects();
            return Err(error);
        }
        Ok(uploader)
    }

    fn make_objects(&mut self, family: u32) -> Result<(), VulkanError> {
        let pool_info = CommandPoolCreateInfo {
            s_type: STYPE_COMMAND_POOL_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: COMMAND_POOL_TRANSIENT,
            queue_family_index: family,
        };
        let mut pool: CommandPoolHandle = 0;
        // SAFETY: `pool_info` is valid for the call; `pool` is a valid output.
        let result = unsafe {
            (self.fns.create_command_pool)(
                self.device,
                &raw const pool_info,
                core::ptr::null(),
                &raw mut pool,
            )
        };
        check_created("vkCreateCommandPool", result, pool == 0)?;
        self.pool = pool;
        let fence_info = SyncCreateInfo {
            s_type: STYPE_FENCE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: FENCE_CREATE_SIGNALED,
        };
        let mut fence: FenceHandle = 0;
        // SAFETY: `fence_info` is valid for the call; `fence` is a valid output.
        let result = unsafe {
            (self.fns.create_fence)(
                self.device,
                &raw const fence_info,
                core::ptr::null(),
                &raw mut fence,
            )
        };
        check_created("vkCreateFence", result, fence == 0)?;
        self.fence = fence;
        let alloc_info = CommandBufferAllocateInfo {
            s_type: STYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            command_pool: pool,
            level: COMMAND_BUFFER_LEVEL_PRIMARY,
            command_buffer_count: 1,
        };
        let mut cmd = CommandBufferHandle::NULL;
        // SAFETY: `alloc_info` is valid for the call; one buffer is asked for and `cmd` holds one.
        let result = unsafe {
            (self.fns.allocate_command_buffers)(self.device, &raw const alloc_info, &raw mut cmd)
        };
        check_created("vkAllocateCommandBuffers", result, cmd.0.is_null())?;
        self.cmd = cmd;
        Ok(())
    }

    /// Copies `bytes` into the staging buffer (growing it if needed) and returns the buffer; the
    /// bytes start at its offset 0.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the staging buffer cannot be made.
    pub fn stage(
        &mut self,
        memory: &mut MemoryManager,
        bytes: &[u8],
    ) -> Result<BufferHandle, VulkanError> {
        let length = u64::try_from(bytes.len()).map_err(|_| VulkanError::Invalid("too large"))?;
        let (buffer, base) = self.reserve(memory, length)?;
        // SAFETY: the mapping holds at least `length` bytes (reserve grew it) and no GPU work
        // uses the buffer: every transfer waits for its fence before it returns.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), base.as_ptr(), bytes.len()) };
        Ok(buffer)
    }

    /// Makes sure the staging buffer holds `length` bytes and returns it with its mapping.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the staging buffer cannot be made.
    pub fn reserve(
        &mut self,
        memory: &mut MemoryManager,
        length: u64,
    ) -> Result<(BufferHandle, NonNull<u8>), VulkanError> {
        let fits = self
            .staging
            .as_ref()
            .is_some_and(|staging| staging.capacity >= length);
        if !fits {
            self.drop_staging(memory);
            self.staging = Some(self.make_staging(memory, length.max(MIN_STAGING))?);
        }
        let staging = self
            .staging
            .as_ref()
            .ok_or(VulkanError::Invalid("no staging buffer"))?;
        let base = staging
            .allocation
            .mapped()
            .ok_or(VulkanError::Invalid("the staging memory is not mapped"))?;
        Ok((staging.buffer, base))
    }

    /// A host-visible buffer of `capacity` bytes that can be both source and target of a copy.
    fn make_staging(
        &self,
        memory: &mut MemoryManager,
        capacity: u64,
    ) -> Result<Staging, VulkanError> {
        let info = BufferCreateInfo {
            s_type: STYPE_BUFFER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            size: capacity,
            usage: BUFFER_USAGE_TRANSFER_SRC | BUFFER_USAGE_TRANSFER_DST,
            sharing_mode: SHARING_EXCLUSIVE,
            queue_family_index_count: 0,
            queue_family_indices: core::ptr::null(),
        };
        let mut buffer: BufferHandle = 0;
        // SAFETY: `info` is valid for the call; `buffer` is a valid output.
        let result = unsafe {
            (self.fns.create_buffer)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut buffer,
            )
        };
        check_created("vkCreateBuffer", result, buffer == 0)?;
        let mut required = MemoryRequirements::default();
        // SAFETY: the buffer is live; `required` is a valid output.
        unsafe {
            (self.fns.get_buffer_memory_requirements)(self.device, buffer, &raw mut required);
        }
        let allocation = match memory.allocate(&required, Kind::Linear, Placement::Upload) {
            Ok(allocation) => allocation,
            Err(error) => {
                // SAFETY: made above on this device, never bound or used.
                unsafe { (self.fns.destroy_buffer)(self.device, buffer, core::ptr::null()) };
                return Err(error);
            }
        };
        // SAFETY: the buffer and the memory are live and the buffer is not bound yet.
        let bound = ok("vkBindBufferMemory", unsafe {
            (self.fns.bind_buffer_memory)(self.device, buffer, allocation.memory, allocation.offset)
        });
        if let Err(error) = bound {
            // SAFETY: made above on this device, never used.
            unsafe { (self.fns.destroy_buffer)(self.device, buffer, core::ptr::null()) };
            memory.free(allocation);
            return Err(error);
        }
        Ok(Staging {
            buffer,
            allocation,
            capacity,
        })
    }

    fn drop_staging(&mut self, memory: &mut MemoryManager) {
        if let Some(staging) = self.staging.take() {
            // SAFETY: made on this device; no transfer is in flight (each one waits).
            unsafe { (self.fns.destroy_buffer)(self.device, staging.buffer, core::ptr::null()) };
            memory.free(staging.allocation);
        }
    }

    /// Records with `record`, submits to the queue and waits until the work has finished.
    ///
    /// The fence is reset right before the submit, so an error before it leaves the fence as it
    /// was and the next transfer is not affected.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver result.
    pub fn submit(&mut self, record: impl FnOnce(&Recorder<'_>)) -> Result<(), VulkanError> {
        self.finish_pending()?;
        // SAFETY: the last transfer waited for its fence, so the buffer is not in use.
        ok("vkResetCommandPool", unsafe {
            (self.fns.reset_command_pool)(self.device, self.pool, 0)
        })?;
        let begin = CommandBufferBeginInfo {
            s_type: STYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: core::ptr::null(),
            flags: COMMAND_BUFFER_ONE_TIME_SUBMIT,
            inheritance_info: core::ptr::null(),
        };
        // SAFETY: the pool was reset, so the buffer is in the initial state; `begin` is valid.
        ok("vkBeginCommandBuffer", unsafe {
            (self.fns.begin_command_buffer)(self.cmd, &raw const begin)
        })?;
        record(&Recorder {
            fns: &self.fns,
            cmd: self.cmd,
        });
        // SAFETY: the buffer is recording.
        ok("vkEndCommandBuffer", unsafe {
            (self.fns.end_command_buffer)(self.cmd)
        })?;
        let command = CommandBufferSubmitInfo {
            s_type: STYPE_COMMAND_BUFFER_SUBMIT_INFO,
            p_next: core::ptr::null(),
            command_buffer: self.cmd,
            device_mask: 0,
        };
        let submit = SubmitInfo2 {
            s_type: STYPE_SUBMIT_INFO_2,
            p_next: core::ptr::null(),
            flags: 0,
            wait_semaphore_info_count: 0,
            wait_semaphore_infos: core::ptr::null(),
            command_buffer_info_count: 1,
            command_buffer_infos: &raw const command,
            signal_semaphore_info_count: 0,
            signal_semaphore_infos: core::ptr::null(),
        };
        // SAFETY: the fence is valid and no submit uses it now (the last one was waited for).
        ok("vkResetFences", unsafe {
            (self.fns.reset_fences)(self.device, 1, &raw const self.fence)
        })?;
        // SAFETY: the buffer is recorded; `submit` and `command` outlive the call.
        ok("vkQueueSubmit2", unsafe {
            (self.fns.queue_submit2)(self.queue, 1, &raw const submit, self.fence)
        })?;
        self.pending = true;
        self.finish_pending()
    }

    /// Whether work was submitted that has not been seen to finish (the wait for it failed).
    pub fn work_pending(&self) -> bool {
        self.pending
    }

    /// Waits for the last submit, if its wait has not succeeded yet.
    fn finish_pending(&mut self) -> Result<(), VulkanError> {
        if !self.pending {
            return Ok(());
        }
        // SAFETY: the fence was submitted and not reset since.
        ok("vkWaitForFences", unsafe {
            (self.fns.wait_for_fences)(self.device, 1, &raw const self.fence, 1, FOREVER)
        })?;
        self.pending = false;
        Ok(())
    }

    /// Frees the staging buffer and the transfer objects. The device must be idle.
    pub fn destroy(&mut self, memory: &mut MemoryManager) {
        self.drop_staging(memory);
        self.destroy_objects();
    }

    /// Destroys the fence and the command pool (with its buffer), whichever exist.
    fn destroy_objects(&mut self) {
        if self.fence != 0 {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (self.fns.destroy_fence)(self.device, self.fence, core::ptr::null()) };
            self.fence = 0;
        }
        if self.pool != 0 {
            // SAFETY: made on this device and destroyed once, here (with its command buffer).
            unsafe { (self.fns.destroy_command_pool)(self.device, self.pool, core::ptr::null()) };
            self.pool = 0;
        }
    }
}
