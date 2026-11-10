//! One frame in flight: the semaphore the acquire signals, the fence the submit signals, and a
//! command pool with its one command buffer. Also the commands of a clear-and-present frame.
#![expect(
    unsafe_code,
    reason = "the frame objects are a C interface; every unsafe block has a SAFETY comment"
)]

use crate::capture::Capture;
use crate::error::{VulkanError, check_created};
use crate::gear::{Gear, ok};
use crate::swapchain::make_semaphore;
use crate::vk_present::{
    ACCESS_COLOR_ATTACHMENT_WRITE, ACCESS_NONE, ACCESS_TRANSFER_READ, COMMAND_BUFFER_LEVEL_PRIMARY,
    COMMAND_BUFFER_ONE_TIME_SUBMIT, COMMAND_POOL_TRANSIENT, ClearColor, CommandBufferAllocateInfo,
    CommandBufferBeginInfo, CommandBufferHandle, CommandPoolCreateInfo, CommandPoolHandle,
    DependencyInfo, Extent2D, FENCE_CREATE_SIGNALED, FenceHandle, ImageHandle, ImageMemoryBarrier2,
    ImageViewHandle, LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC, LAYOUT_TRANSFER_SRC,
    LAYOUT_UNDEFINED, LOAD_OP_CLEAR, QUEUE_FAMILY_IGNORED, Rect2D, RenderingAttachmentInfo,
    RenderingInfo, STAGE_ALL_TRANSFER, STAGE_COLOR_ATTACHMENT_OUTPUT, STAGE_NONE, STORE_OP_STORE,
    STYPE_COMMAND_BUFFER_ALLOCATE_INFO, STYPE_COMMAND_BUFFER_BEGIN_INFO,
    STYPE_COMMAND_POOL_CREATE_INFO, STYPE_DEPENDENCY_INFO, STYPE_FENCE_CREATE_INFO,
    STYPE_IMAGE_MEMORY_BARRIER_2, STYPE_RENDERING_ATTACHMENT_INFO, STYPE_RENDERING_INFO,
    SemaphoreHandle, SubresourceRange, SyncCreateInfo,
};

/// Frames in flight: while the GPU draws one, the CPU records the next.
pub const FRAMES_IN_FLIGHT: usize = 2;

/// The synchronisation objects and command buffer of one frame. A zero handle is "not made".
#[derive(Clone, Copy)]
pub struct Frame {
    pub acquire: SemaphoreHandle,
    pub fence: FenceHandle,
    pool: CommandPoolHandle,
    pub cmd: CommandBufferHandle,
}

impl Frame {
    /// A frame with nothing made yet.
    pub const EMPTY: Self = Self {
        acquire: 0,
        fence: 0,
        pool: 0,
        cmd: CommandBufferHandle::NULL,
    };

    /// Makes the objects of this frame, in `self`, one at a time, so that [`Frame::destroy`] can
    /// free what exists after an error. The fence starts signalled: the first wait must not block.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result.
    pub fn create(&mut self, gear: &Gear, family: u32) -> Result<(), VulkanError> {
        self.acquire = make_semaphore(gear)?;
        self.fence = make_fence(gear)?;
        self.pool = make_pool(gear, family)?;
        let info = CommandBufferAllocateInfo {
            s_type: STYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            command_pool: self.pool,
            level: COMMAND_BUFFER_LEVEL_PRIMARY,
            command_buffer_count: 1,
        };
        let mut cmd = CommandBufferHandle::NULL;
        // SAFETY: `info` is valid for the call; one buffer is asked for and `cmd` holds one.
        let result = unsafe {
            (gear.fns.allocate_command_buffers)(gear.device, &raw const info, &raw mut cmd)
        };
        check_created("vkAllocateCommandBuffers", result, cmd.0.is_null())?;
        self.cmd = cmd;
        Ok(())
    }

    /// Destroys what was made (the pool takes its command buffer with it). The device must be
    /// idle.
    pub fn destroy(&mut self, gear: &Gear) {
        if self.pool != 0 {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (gear.fns.destroy_command_pool)(gear.device, self.pool, core::ptr::null()) };
        }
        if self.fence != 0 {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (gear.fns.destroy_fence)(gear.device, self.fence, core::ptr::null()) };
        }
        if self.acquire != 0 {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (gear.fns.destroy_semaphore)(gear.device, self.acquire, core::ptr::null()) };
        }
        *self = Self::EMPTY;
    }

    /// Resets the pool (and with it the command buffer) for a new recording.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result.
    pub fn reset(&self, gear: &Gear) -> Result<(), VulkanError> {
        // SAFETY: the fence wait before this call proved the buffer is no longer in use.
        let result = unsafe { (gear.fns.reset_command_pool)(gear.device, self.pool, 0) };
        ok("vkResetCommandPool", result)
    }
}

fn make_fence(gear: &Gear) -> Result<FenceHandle, VulkanError> {
    let info = SyncCreateInfo {
        s_type: STYPE_FENCE_CREATE_INFO,
        p_next: core::ptr::null(),
        flags: FENCE_CREATE_SIGNALED,
    };
    let mut fence: FenceHandle = 0;
    // SAFETY: `info` is valid for the call; `fence` is a valid output.
    let result = unsafe {
        (gear.fns.create_fence)(
            gear.device,
            &raw const info,
            core::ptr::null(),
            &raw mut fence,
        )
    };
    check_created("vkCreateFence", result, fence == 0)?;
    Ok(fence)
}

fn make_pool(gear: &Gear, family: u32) -> Result<CommandPoolHandle, VulkanError> {
    let info = CommandPoolCreateInfo {
        s_type: STYPE_COMMAND_POOL_CREATE_INFO,
        p_next: core::ptr::null(),
        flags: COMMAND_POOL_TRANSIENT,
        queue_family_index: family,
    };
    let mut pool: CommandPoolHandle = 0;
    // SAFETY: `info` is valid for the call; `pool` is a valid output.
    let result = unsafe {
        (gear.fns.create_command_pool)(
            gear.device,
            &raw const info,
            core::ptr::null(),
            &raw mut pool,
        )
    };
    check_created("vkCreateCommandPool", result, pool == 0)?;
    Ok(pool)
}

/// One layout change of a colour image: the layouts and the stage and access on each side.
struct Transition {
    old_layout: u32,
    new_layout: u32,
    src: (u64, u64),
    dst: (u64, u64),
}

/// What a clear-and-present frame draws into.
pub struct Target<'a> {
    pub image: ImageHandle,
    pub view: ImageViewHandle,
    pub extent: Extent2D,
    pub color: [f32; 4],
    pub capture: Option<&'a Capture>,
}

/// Records the frame: image to attachment, clear, (copy the middle pixel out,) image to present.
///
/// # Errors
/// [`VulkanError::Call`] with the driver's result.
pub fn record_clear(
    gear: &Gear,
    cmd: CommandBufferHandle,
    target: &Target,
) -> Result<(), VulkanError> {
    let begin = CommandBufferBeginInfo {
        s_type: STYPE_COMMAND_BUFFER_BEGIN_INFO,
        p_next: core::ptr::null(),
        flags: COMMAND_BUFFER_ONE_TIME_SUBMIT,
        inheritance_info: core::ptr::null(),
    };
    // SAFETY: the pool was reset, so the buffer is in the initial state; `begin` is valid.
    ok("vkBeginCommandBuffer", unsafe {
        (gear.fns.begin_command_buffer)(cmd, &raw const begin)
    })?;
    // The acquire semaphore is waited at the colour-attachment-output stage, so that is the
    // stage the first transition waits for.
    barrier(
        gear,
        cmd,
        target.image,
        &Transition {
            old_layout: LAYOUT_UNDEFINED,
            new_layout: LAYOUT_COLOR_ATTACHMENT,
            src: (STAGE_COLOR_ATTACHMENT_OUTPUT, ACCESS_NONE),
            dst: (STAGE_COLOR_ATTACHMENT_OUTPUT, ACCESS_COLOR_ATTACHMENT_WRITE),
        },
    );
    clear(gear, cmd, target);
    let written = (STAGE_COLOR_ATTACHMENT_OUTPUT, ACCESS_COLOR_ATTACHMENT_WRITE);
    let last = match target.capture {
        Some(capture) => {
            let copy_source = (STAGE_ALL_TRANSFER, ACCESS_TRANSFER_READ);
            barrier(
                gear,
                cmd,
                target.image,
                &Transition {
                    old_layout: LAYOUT_COLOR_ATTACHMENT,
                    new_layout: LAYOUT_TRANSFER_SRC,
                    src: written,
                    dst: copy_source,
                },
            );
            capture.record_copy(cmd, target.image, target.extent);
            copy_source
        }
        None => written,
    };
    barrier(
        gear,
        cmd,
        target.image,
        &Transition {
            old_layout: if target.capture.is_some() {
                LAYOUT_TRANSFER_SRC
            } else {
                LAYOUT_COLOR_ATTACHMENT
            },
            new_layout: LAYOUT_PRESENT_SRC,
            src: last,
            dst: (STAGE_NONE, ACCESS_NONE),
        },
    );
    // SAFETY: the buffer is in the recording state.
    ok("vkEndCommandBuffer", unsafe {
        (gear.fns.end_command_buffer)(cmd)
    })
}

fn barrier(gear: &Gear, cmd: CommandBufferHandle, image: ImageHandle, change: &Transition) {
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
        image,
        subresource_range: SubresourceRange::COLOR,
    };
    let dependency = DependencyInfo {
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
    // SAFETY: the buffer is recording; `dependency` and the barrier it points to live until the
    // call returns.
    unsafe { (gear.fns.cmd_pipeline_barrier2)(cmd, &raw const dependency) };
}

fn clear(gear: &Gear, cmd: CommandBufferHandle, target: &Target) {
    let attachment = RenderingAttachmentInfo {
        s_type: STYPE_RENDERING_ATTACHMENT_INFO,
        p_next: core::ptr::null(),
        image_view: target.view,
        image_layout: LAYOUT_COLOR_ATTACHMENT,
        resolve_mode: 0,
        resolve_image_view: 0,
        resolve_image_layout: 0,
        load_op: LOAD_OP_CLEAR,
        store_op: STORE_OP_STORE,
        clear_value: ClearColor {
            float32: target.color,
        },
    };
    let info = RenderingInfo {
        s_type: STYPE_RENDERING_INFO,
        p_next: core::ptr::null(),
        flags: 0,
        render_area: Rect2D {
            offset: crate::vk_present::Offset2D { x: 0, y: 0 },
            extent: target.extent,
        },
        layer_count: 1,
        view_mask: 0,
        color_attachment_count: 1,
        color_attachments: &raw const attachment,
        depth_attachment: core::ptr::null(),
        stencil_attachment: core::ptr::null(),
    };
    // SAFETY: the buffer is recording; `info` and the attachment it points to live until the
    // calls return; the view belongs to a live swapchain image in the attachment layout.
    unsafe {
        (gear.fns.cmd_begin_rendering)(cmd, &raw const info);
        (gear.fns.cmd_end_rendering)(cmd);
    }
}
