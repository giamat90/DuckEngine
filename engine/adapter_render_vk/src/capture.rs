//! A one-pixel readback of the swapchain image, for tests that must see what was drawn.
//!
//! The buffer and its memory are made directly with `vkAllocateMemory`: the allocator of card 6-8
//! replaces this. Only the middle pixel is copied, so the buffer is a few bytes.
#![expect(
    unsafe_code,
    reason = "the buffer is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::error::{VulkanError, check_created};
use crate::gear::{CaptureFns, Gear, ok};
use crate::plan::pick_memory_type;
use crate::vk_present::{
    ASPECT_COLOR, BUFFER_USAGE_TRANSFER_DST, BufferCreateInfo, BufferHandle, BufferImageCopy,
    CommandBufferHandle, Extent2D, ImageHandle, LAYOUT_TRANSFER_SRC, MEMORY_HOST_COHERENT,
    MEMORY_HOST_VISIBLE, MemoryAllocateInfo, MemoryHandle, MemoryRequirements, SHARING_EXCLUSIVE,
    STYPE_BUFFER_CREATE_INFO, STYPE_MEMORY_ALLOCATE_INFO, SubresourceLayers,
};
use core::ffi::c_void;
use core::ptr::NonNull;

/// The bytes of one Bgra8 pixel.
const PIXEL_BYTES: u64 = 4;

/// The readback buffer, its memory and its mapping.
pub struct Capture {
    fns: CaptureFns,
    device: crate::vk::DeviceHandle,
    buffer: BufferHandle,
    memory: MemoryHandle,
    mapped: Option<NonNull<u8>>,
}

impl Capture {
    /// Makes a host-visible buffer for one pixel and maps it.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] when the device has no host-visible coherent memory for it;
    /// [`VulkanError::Call`] when the driver refuses. Whatever was made first is destroyed.
    pub fn create(gear: &Gear) -> Result<Self, VulkanError> {
        let mut capture = Self {
            fns: gear.capture_fns()?,
            device: gear.device,
            buffer: 0,
            memory: 0,
            mapped: None,
        };
        if let Err(error) = capture.make(gear) {
            capture.destroy();
            return Err(error);
        }
        Ok(capture)
    }

    fn make(&mut self, gear: &Gear) -> Result<(), VulkanError> {
        let info = BufferCreateInfo {
            s_type: STYPE_BUFFER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            size: PIXEL_BYTES,
            usage: BUFFER_USAGE_TRANSFER_DST,
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
        self.buffer = buffer;
        let mut required = MemoryRequirements::default();
        // SAFETY: the buffer is live; `required` is a valid output.
        unsafe {
            (self.fns.get_buffer_memory_requirements)(self.device, buffer, &raw mut required);
        }
        let properties = gear.core.instance.memory_properties(gear.core.gpu);
        let wanted = MEMORY_HOST_VISIBLE | MEMORY_HOST_COHERENT;
        let type_index = pick_memory_type(&properties, required.memory_type_bits, wanted).ok_or(
            VulkanError::Unsupported("no host-visible coherent memory for the readback buffer"),
        )?;
        self.allocate_and_map(required.size, type_index)
    }

    fn allocate_and_map(&mut self, size: u64, type_index: u32) -> Result<(), VulkanError> {
        let info = MemoryAllocateInfo {
            s_type: STYPE_MEMORY_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            allocation_size: size,
            memory_type_index: type_index,
        };
        let mut memory: MemoryHandle = 0;
        // SAFETY: `info` is valid for the call; `memory` is a valid output.
        let result = unsafe {
            (self.fns.allocate_memory)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut memory,
            )
        };
        check_created("vkAllocateMemory", result, memory == 0)?;
        self.memory = memory;
        // SAFETY: the buffer and the memory are live and the memory is not bound yet.
        ok("vkBindBufferMemory", unsafe {
            (self.fns.bind_buffer_memory)(self.device, self.buffer, memory, 0)
        })?;
        let mut address: *mut c_void = core::ptr::null_mut();
        // SAFETY: the memory is host-visible and not mapped; `address` is a valid output.
        let result = unsafe {
            (self.fns.map_memory)(self.device, memory, 0, PIXEL_BYTES, 0, &raw mut address)
        };
        ok("vkMapMemory", result)?;
        self.mapped = NonNull::new(address.cast::<u8>());
        if self.mapped.is_none() {
            return Err(VulkanError::Call {
                call: "vkMapMemory",
                result: crate::vk::ERROR_INITIALIZATION_FAILED,
            });
        }
        Ok(())
    }

    /// Records the copy of the middle pixel of `image` (in the transfer-source layout) into the
    /// buffer.
    pub fn record_copy(&self, cmd: CommandBufferHandle, image: ImageHandle, extent: Extent2D) {
        let region = BufferImageCopy {
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image_subresource: SubresourceLayers {
                aspect_mask: ASPECT_COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            },
            image_offset: [
                i32::try_from(extent.width / 2).unwrap_or(0),
                i32::try_from(extent.height / 2).unwrap_or(0),
                0,
            ],
            image_extent: [1, 1, 1],
        };
        // SAFETY: the buffer is recording; the image is in the transfer-source layout; the
        // region lies inside the image (the middle of a non-empty extent) and the buffer.
        unsafe {
            (self.fns.cmd_copy_image_to_buffer)(
                cmd,
                image,
                LAYOUT_TRANSFER_SRC,
                self.buffer,
                1,
                &raw const region,
            );
        }
    }

    /// The four bytes the last finished frame copied. The device must be idle.
    pub fn read(&self) -> [u8; 4] {
        let Some(mapped) = self.mapped else {
            return [0; 4];
        };
        let mut pixel = [0_u8; 4];
        // SAFETY: the mapping is `PIXEL_BYTES` (4) long and coherent; no GPU work is running, so
        // nothing writes it while it is read.
        unsafe { core::ptr::copy_nonoverlapping(mapped.as_ptr(), pixel.as_mut_ptr(), pixel.len()) };
        pixel
    }

    /// Unmaps and frees the memory and the buffer (whatever exists). The device must be idle.
    pub fn destroy(&mut self) {
        if self.mapped.take().is_some() {
            // SAFETY: the memory was mapped by `make` and is unmapped once.
            unsafe { (self.fns.unmap_memory)(self.device, self.memory) };
        }
        if self.buffer != 0 {
            // SAFETY: made on this device and destroyed once, here, before its memory.
            unsafe { (self.fns.destroy_buffer)(self.device, self.buffer, core::ptr::null()) };
            self.buffer = 0;
        }
        if self.memory != 0 {
            // SAFETY: allocated on this device and freed once, here.
            unsafe { (self.fns.free_memory)(self.device, self.memory, core::ptr::null()) };
            self.memory = 0;
        }
    }
}
