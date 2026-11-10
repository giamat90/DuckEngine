//! The resource half of the fake driver: device memory, buffers, images, samplers and copies.
//!
//! Memory is real bytes, so a test can write through a mapping, run an upload and read the texels
//! back. The fake checks what the validation layer would (a memory type the resource allows, an
//! aligned offset inside the allocation, no two resources sharing bytes, the layout an image is in
//! when it is copied) and panics on a violation, so a wrong bind or a missing barrier fails the
//! test that ran it. Copies run when they are recorded, not when the queue is submitted.

use super::{note, with};
use crate::vk::{self, DeviceHandle, PhysicalDeviceHandle};
use crate::vk_present::{
    self as vp, BufferCreateInfo, BufferImageCopy, CommandBufferHandle, DependencyInfo,
    ImageMemoryBarrier2, MemoryAllocateInfo, MemoryRequirements, PhysicalDeviceMemoryProperties,
};
use crate::vk_resource::{
    self as vr, BufferCopy, FormatProperties, ImageCreateInfo, MemoryBarrier2, SamplerCreateInfo,
};
use core::ffi::c_void;
use std::collections::HashMap;

/// `VK_ERROR_OUT_OF_HOST_MEMORY`.
const OUT_OF_HOST_MEMORY: i32 = -1;
/// `VK_ERROR_OUT_OF_DEVICE_MEMORY`.
const OUT_OF_DEVICE_MEMORY: i32 = -2;

/// One `vkAllocateMemory` block.
pub struct FakeMemory {
    pub type_index: u32,
    pub bytes: Vec<u8>,
    pub mapped: bool,
}

/// A buffer and where it is bound.
pub struct FakeBuffer {
    pub size: u64,
    pub usage: u32,
    pub bound: Option<(u64, u64)>,
}

/// An image, its level-0 texels and where it is bound.
pub struct FakeImage {
    pub format: u32,
    pub extent: [u32; 3],
    pub usage: u32,
    pub layout: u32,
    pub bound: Option<(u64, u64)>,
    pub texels: Vec<u8>,
    pub size: u64,
}

/// What `vkCreateSampler` was asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct SamplerRecord {
    pub min_filter: u32,
    pub mag_filter: u32,
    pub mipmap_mode: u32,
    pub address: u32,
    pub max_lod: f32,
}

/// A command the fake recorded, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    GlobalBarrier,
    ImageBarrier { image: u64, old: u32, new: u32 },
    CopyBuffer { src: u64, dst: u64, size: u64 },
    BufferToImage { image: u64, width: u32, height: u32 },
    ImageToBuffer { image: u64, width: u32, height: u32 },
}

/// One image barrier as the driver saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageBarrierRecord {
    pub image: u64,
    pub aspect: u32,
    pub old: u32,
    pub new: u32,
    pub src_stage: u64,
    pub dst_stage: u64,
    pub dst_access: u64,
}

/// The state of the resource half.
pub struct World {
    pub memories: HashMap<u64, FakeMemory>,
    pub buffers: HashMap<u64, FakeBuffer>,
    pub images: HashMap<u64, FakeImage>,
    pub samplers: HashMap<u64, SamplerRecord>,
    /// Each `vkAllocateMemory`: memory type and size.
    pub allocations: Vec<(u32, u64)>,
    pub ops: Vec<Op>,
    /// The (source stage, source access, destination stage, destination access) of each global
    /// barrier, in order.
    pub global_masks: Vec<(u64, u64, u64, u64)>,
    pub buffer_alignment: u64,
    pub image_alignment: u64,
    pub buffer_type_bits: u32,
    pub image_type_bits: u32,
    /// `vkAllocateMemory` fails once this many allocations were made.
    pub allocation_limit: Option<usize>,
    /// The `optimalTilingFeatures` of the depth format.
    pub depth_features: u32,
    /// The `maxImageDimension2D` the device reports.
    pub max_extent: u32,
}

impl World {
    /// Memory type 0 is device local, type 1 host visible and coherent; buffers may use both,
    /// images the first.
    pub fn new() -> Self {
        Self {
            memories: HashMap::new(),
            buffers: HashMap::new(),
            images: HashMap::new(),
            samplers: HashMap::new(),
            allocations: Vec::new(),
            ops: Vec::new(),
            global_masks: Vec::new(),
            buffer_alignment: 16,
            image_alignment: 64,
            buffer_type_bits: 0b11,
            image_type_bits: 0b01,
            allocation_limit: None,
            depth_features: vr::FEATURE_TRANSFER_SRC | vr::FEATURE_DEPTH_ATTACHMENT,
            max_extent: 16384,
        }
    }
}

fn refused(call: &str) -> bool {
    with(|script| script.fail_call == Some(call))
}

fn bytes_per_texel(format: u32) -> u64 {
    match format {
        vr::FORMAT_R8G8B8A8_UNORM | vp::FORMAT_B8G8R8A8_UNORM | vr::FORMAT_D32_SFLOAT => 4,
        other => panic!("the fake does not know format {other}"),
    }
}

fn round_up(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

/// The address of the resource function `name`, or null.
pub fn lookup(name: &str) -> *mut c_void {
    macro_rules! address {
        ($f:expr) => {
            ($f as *const ()).cast_mut().cast::<c_void>()
        };
    }
    match name {
        "vkCreateBuffer" => address!(create_buffer as unsafe extern "system" fn(_, _, _, _) -> _),
        "vkDestroyBuffer" => address!(destroy_buffer as unsafe extern "system" fn(_, _, _)),
        "vkGetBufferMemoryRequirements" => {
            address!(buffer_requirements as unsafe extern "system" fn(_, _, _))
        }
        "vkBindBufferMemory" => {
            address!(bind_buffer_memory as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkCreateImage" => address!(create_image as unsafe extern "system" fn(_, _, _, _) -> _),
        "vkDestroyImage" => address!(destroy_image as unsafe extern "system" fn(_, _, _)),
        "vkGetImageMemoryRequirements" => {
            address!(image_requirements as unsafe extern "system" fn(_, _, _))
        }
        "vkBindImageMemory" => {
            address!(bind_image_memory as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkCreateSampler" => {
            address!(create_sampler as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroySampler" => address!(destroy_sampler as unsafe extern "system" fn(_, _, _)),
        "vkAllocateMemory" => {
            address!(allocate_memory as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkFreeMemory" => address!(free_memory as unsafe extern "system" fn(_, _, _)),
        "vkMapMemory" => address!(map_memory as unsafe extern "system" fn(_, _, _, _, _, _) -> _),
        "vkUnmapMemory" => address!(unmap_memory as unsafe extern "system" fn(_, _)),
        "vkCmdCopyBuffer" => address!(cmd_copy_buffer as unsafe extern "system" fn(_, _, _, _, _)),
        "vkCmdCopyBufferToImage" => {
            address!(cmd_copy_buffer_to_image as unsafe extern "system" fn(_, _, _, _, _, _))
        }
        "vkCmdCopyImageToBuffer" => {
            address!(cmd_copy_image_to_buffer as unsafe extern "system" fn(_, _, _, _, _, _))
        }
        "vkGetPhysicalDeviceFormatProperties" => {
            address!(format_properties as unsafe extern "system" fn(_, _, _))
        }
        _ => core::ptr::null_mut(),
    }
}

/// Registers a new object under `kind` and returns its handle.
fn new_handle(kind: &'static str) -> u64 {
    super::present::alloc(kind)
}

unsafe extern "system" fn create_buffer(
    _device: DeviceHandle,
    info: *const BufferCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateBuffer");
    if refused("vkCreateBuffer") {
        return OUT_OF_HOST_MEMORY;
    }
    let (size, usage) = unsafe { ((*info).size, (*info).usage) };
    assert!(size > 0, "a zero-sized buffer is invalid usage");
    assert_eq!(unsafe { (*info).s_type }, vp::STYPE_BUFFER_CREATE_INFO);
    let handle = new_handle("buffer");
    with(|script| {
        script.world.buffers.insert(
            handle,
            FakeBuffer {
                size,
                usage,
                bound: None,
            },
        );
    });
    unsafe { *out = handle };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_buffer(
    _device: DeviceHandle,
    buffer: u64,
    _allocator: *const c_void,
) {
    note("vkDestroyBuffer");
    super::present::release("buffer", buffer);
    with(|script| script.world.buffers.remove(&buffer));
}

unsafe extern "system" fn buffer_requirements(
    _device: DeviceHandle,
    buffer: u64,
    out: *mut MemoryRequirements,
) {
    note("vkGetBufferMemoryRequirements");
    let requirements = with(|script| {
        let alignment = script.world.buffer_alignment;
        let size = script.world.buffers[&buffer].size;
        MemoryRequirements {
            size: round_up(size, alignment),
            alignment,
            memory_type_bits: script.world.buffer_type_bits,
        }
    });
    unsafe { out.write(requirements) };
}

unsafe extern "system" fn image_requirements(
    _device: DeviceHandle,
    image: u64,
    out: *mut MemoryRequirements,
) {
    note("vkGetImageMemoryRequirements");
    let requirements = with(|script| {
        let alignment = script.world.image_alignment;
        let size = script.world.images[&image].size;
        MemoryRequirements {
            size: round_up(size, alignment),
            alignment,
            memory_type_bits: script.world.image_type_bits,
        }
    });
    unsafe { out.write(requirements) };
}

/// Binds `resource` of `size` bytes at `offset` in `memory`, checking what the layer checks.
fn check_bind(memory: u64, offset: u64, requirements: &MemoryRequirements, resource: u64) {
    with(|script| {
        let world = &script.world;
        let block = world
            .memories
            .get(&memory)
            .unwrap_or_else(|| panic!("binding to memory {memory:#x}, which is not allocated"));
        assert_eq!(
            offset % requirements.alignment,
            0,
            "offset {offset} is not a multiple of the alignment {}",
            requirements.alignment
        );
        assert!(
            requirements.memory_type_bits & (1 << block.type_index) != 0,
            "memory type {} is not allowed for this resource (bits {:#b})",
            block.type_index,
            requirements.memory_type_bits
        );
        let end = offset + requirements.size;
        assert!(
            end <= u64::try_from(block.bytes.len()).unwrap_or(0),
            "the binding {offset}..{end} leaves the allocation"
        );
        let others = world
            .buffers
            .iter()
            .filter(|(handle, _)| **handle != resource)
            .filter_map(|(_, buffer)| buffer.bound.map(|at| (at, buffer.size)))
            .chain(
                world
                    .images
                    .iter()
                    .filter(|(handle, _)| **handle != resource)
                    .filter_map(|(_, image)| image.bound.map(|at| (at, image.size))),
            );
        for ((other_memory, other_offset), other_size) in others {
            let other_end = other_offset + other_size;
            assert!(
                other_memory != memory || end <= other_offset || other_end <= offset,
                "{resource:#x} at {offset}..{end} overlaps another resource at {other_offset}..{other_end}"
            );
        }
    });
}

unsafe extern "system" fn bind_buffer_memory(
    _device: DeviceHandle,
    buffer: u64,
    memory: u64,
    offset: u64,
) -> i32 {
    note("vkBindBufferMemory");
    if refused("vkBindBufferMemory") {
        return OUT_OF_HOST_MEMORY;
    }
    let size = with(|script| script.world.buffers[&buffer].size);
    let requirements = with(|script| MemoryRequirements {
        size,
        alignment: script.world.buffer_alignment,
        memory_type_bits: script.world.buffer_type_bits,
    });
    check_bind(memory, offset, &requirements, buffer);
    with(|script| {
        if let Some(entry) = script.world.buffers.get_mut(&buffer) {
            assert!(entry.bound.is_none(), "the buffer is bound twice");
            entry.bound = Some((memory, offset));
        }
    });
    vk::SUCCESS
}

unsafe extern "system" fn bind_image_memory(
    _device: DeviceHandle,
    image: u64,
    memory: u64,
    offset: u64,
) -> i32 {
    note("vkBindImageMemory");
    if refused("vkBindImageMemory") {
        return OUT_OF_HOST_MEMORY;
    }
    let (size, alignment, bits) = with(|script| {
        (
            script.world.images[&image].size,
            script.world.image_alignment,
            script.world.image_type_bits,
        )
    });
    let requirements = MemoryRequirements {
        size,
        alignment,
        memory_type_bits: bits,
    };
    check_bind(memory, offset, &requirements, image);
    with(|script| {
        if let Some(entry) = script.world.images.get_mut(&image) {
            assert!(entry.bound.is_none(), "the image is bound twice");
            entry.bound = Some((memory, offset));
        }
    });
    vk::SUCCESS
}

unsafe extern "system" fn create_image(
    _device: DeviceHandle,
    info: *const ImageCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateImage");
    if refused("vkCreateImage") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vr::STYPE_IMAGE_CREATE_INFO);
    assert_eq!(info.image_type, vr::IMAGE_TYPE_2D);
    assert_eq!(info.tiling, vr::TILING_OPTIMAL);
    assert_eq!(info.samples, vr::SAMPLES_1);
    assert_eq!(info.initial_layout, vp::LAYOUT_UNDEFINED);
    assert!(info.extent[0] > 0 && info.extent[1] > 0 && info.mip_levels > 0);
    let max = with(|script| script.world.max_extent);
    assert!(
        info.extent[0] <= max && info.extent[1] <= max,
        "the image is larger than maxImageDimension2D"
    );
    let texel = bytes_per_texel(info.format);
    let level0 = u64::from(info.extent[0]) * u64::from(info.extent[1]) * texel;
    let handle = new_handle("image");
    with(|script| {
        script.world.images.insert(
            handle,
            FakeImage {
                format: info.format,
                extent: info.extent,
                usage: info.usage,
                layout: vp::LAYOUT_UNDEFINED,
                bound: None,
                texels: vec![0; usize::try_from(level0).unwrap_or(0)],
                // The whole chain is a third more than level 0.
                size: level0 + level0 / 3,
            },
        );
    });
    unsafe { *out = handle };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_image(
    _device: DeviceHandle,
    image: u64,
    _allocator: *const c_void,
) {
    note("vkDestroyImage");
    super::present::release("image", image);
    with(|script| script.world.images.remove(&image));
}

unsafe extern "system" fn create_sampler(
    _device: DeviceHandle,
    info: *const SamplerCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateSampler");
    if refused("vkCreateSampler") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vr::STYPE_SAMPLER_CREATE_INFO);
    assert_eq!(info.address_mode_u, info.address_mode_v);
    assert_eq!(info.address_mode_u, info.address_mode_w);
    let handle = new_handle("sampler");
    with(|script| {
        script.world.samplers.insert(
            handle,
            SamplerRecord {
                min_filter: info.min_filter,
                mag_filter: info.mag_filter,
                mipmap_mode: info.mipmap_mode,
                address: info.address_mode_u,
                max_lod: info.max_lod,
            },
        );
    });
    unsafe { *out = handle };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_sampler(
    _device: DeviceHandle,
    sampler: u64,
    _allocator: *const c_void,
) {
    note("vkDestroySampler");
    super::present::release("sampler", sampler);
    with(|script| script.world.samplers.remove(&sampler));
}

unsafe extern "system" fn allocate_memory(
    _device: DeviceHandle,
    info: *const MemoryAllocateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkAllocateMemory");
    if refused("vkAllocateMemory") {
        return OUT_OF_HOST_MEMORY;
    }
    let (size, type_index) = unsafe { ((*info).allocation_size, (*info).memory_type_index) };
    assert!(size > 0, "a zero-sized allocation is invalid usage");
    let over_limit = with(|script| {
        script
            .world
            .allocation_limit
            .is_some_and(|limit| script.world.allocations.len() >= limit)
    });
    if over_limit {
        return OUT_OF_DEVICE_MEMORY;
    }
    let handle = new_handle("memory");
    with(|script| {
        script.world.allocations.push((type_index, size));
        script.world.memories.insert(
            handle,
            FakeMemory {
                type_index,
                bytes: vec![0; usize::try_from(size).unwrap_or(0)],
                mapped: false,
            },
        );
    });
    unsafe { *out = handle };
    vk::SUCCESS
}

unsafe extern "system" fn free_memory(
    _device: DeviceHandle,
    memory: u64,
    _allocator: *const c_void,
) {
    note("vkFreeMemory");
    super::present::release("memory", memory);
    with(|script| {
        let world = &mut script.world;
        let in_use = world
            .buffers
            .values()
            .any(|buffer| buffer.bound.is_some_and(|(held, _)| held == memory))
            || world
                .images
                .values()
                .any(|image| image.bound.is_some_and(|(held, _)| held == memory));
        assert!(
            !in_use,
            "memory {memory:#x} freed while a resource is bound to it"
        );
        world.memories.remove(&memory);
    });
}

unsafe extern "system" fn map_memory(
    _device: DeviceHandle,
    memory: u64,
    offset: u64,
    _size: u64,
    _flags: u32,
    out: *mut *mut c_void,
) -> i32 {
    note("vkMapMemory");
    if refused("vkMapMemory") {
        return OUT_OF_HOST_MEMORY;
    }
    assert_eq!(offset, 0, "the adapter maps whole blocks");
    let address = with(|script| {
        let block = script
            .world
            .memories
            .get_mut(&memory)
            .unwrap_or_else(|| panic!("mapping memory {memory:#x}, which is not allocated"));
        assert!(!block.mapped, "memory {memory:#x} is mapped twice");
        assert_eq!(
            block.type_index, 1,
            "only the host-visible type may be mapped"
        );
        block.mapped = true;
        block.bytes.as_mut_ptr()
    });
    unsafe { *out = address.cast::<c_void>() };
    vk::SUCCESS
}

unsafe extern "system" fn unmap_memory(_device: DeviceHandle, memory: u64) {
    note("vkUnmapMemory");
    with(|script| {
        if let Some(block) = script.world.memories.get_mut(&memory) {
            assert!(block.mapped, "memory {memory:#x} is not mapped");
            block.mapped = false;
        }
    });
}

/// The bytes of `buffer` from `offset`, as the bound memory holds them.
fn buffer_range(buffer: u64, offset: u64, length: u64) -> (u64, usize, usize) {
    with(|script| {
        let entry = &script.world.buffers[&buffer];
        let (memory, base) = entry
            .bound
            .unwrap_or_else(|| panic!("the buffer is used before it is bound"));
        assert!(
            offset + length <= entry.size,
            "the range {offset}+{length} leaves the buffer of {}",
            entry.size
        );
        let start = usize::try_from(base + offset).unwrap_or(0);
        (memory, start, usize::try_from(length).unwrap_or(0))
    })
}

unsafe extern "system" fn cmd_copy_buffer(
    _cmd: CommandBufferHandle,
    source: u64,
    target: u64,
    count: u32,
    regions: *const BufferCopy,
) {
    note("vkCmdCopyBuffer");
    assert_eq!(count, 1);
    let region = unsafe { *regions };
    with(|script| {
        let world = &script.world;
        assert!(world.buffers[&source].usage & vr::BUFFER_USAGE_TRANSFER_SRC != 0);
        assert!(world.buffers[&target].usage & vr::BUFFER_USAGE_TRANSFER_DST != 0);
    });
    let (from_memory, from, length) = buffer_range(source, region.src_offset, region.size);
    let (to_memory, to, _) = buffer_range(target, region.dst_offset, region.size);
    with(|script| {
        let data = script.world.memories[&from_memory].bytes[from..from + length].to_vec();
        if let Some(block) = script.world.memories.get_mut(&to_memory) {
            block.bytes[to..to + length].copy_from_slice(&data);
        }
        script.world.ops.push(Op::CopyBuffer {
            src: source,
            dst: target,
            size: region.size,
        });
    });
}

/// Checks the region of a buffer-image copy and returns (row bytes, rows, x, y).
fn region_shape(image: u64, region: &BufferImageCopy) -> (usize, usize, usize, usize) {
    with(|script| {
        let entry = &script.world.images[&image];
        assert_eq!(region.image_subresource.mip_level, 0, "level 0 only");
        assert_eq!(region.image_extent[2], 1);
        assert_eq!(region.buffer_row_length, 0, "tightly packed");
        assert_eq!(region.buffer_image_height, 0, "tightly packed");
        let (x, y) = (region.image_offset[0], region.image_offset[1]);
        assert!(x >= 0 && y >= 0, "the region starts before the image");
        let (x, y) = (u32::try_from(x).unwrap_or(0), u32::try_from(y).unwrap_or(0));
        assert!(
            x + region.image_extent[0] <= entry.extent[0]
                && y + region.image_extent[1] <= entry.extent[1],
            "the region leaves the image"
        );
        let texel = usize::try_from(bytes_per_texel(entry.format)).unwrap_or(4);
        (
            usize::try_from(region.image_extent[0]).unwrap_or(0) * texel,
            usize::try_from(region.image_extent[1]).unwrap_or(0),
            usize::try_from(x).unwrap_or(0) * texel,
            usize::try_from(y).unwrap_or(0),
        )
    })
}

unsafe extern "system" fn cmd_copy_buffer_to_image(
    _cmd: CommandBufferHandle,
    buffer: u64,
    image: u64,
    layout: u32,
    count: u32,
    regions: *const BufferImageCopy,
) {
    note("vkCmdCopyBufferToImage");
    assert_eq!(count, 1);
    let region = unsafe { &*regions };
    with(|script| {
        let entry = &script.world.images[&image];
        assert!(
            entry.bound.is_some(),
            "the image is used before it is bound"
        );
        assert_eq!(layout, vr::LAYOUT_TRANSFER_DST);
        assert_eq!(
            entry.layout,
            vr::LAYOUT_TRANSFER_DST,
            "the image is not in the layout it is copied in"
        );
        assert!(entry.usage & vr::IMAGE_USAGE_TRANSFER_DST != 0);
        assert!(script.world.buffers[&buffer].usage & vr::BUFFER_USAGE_TRANSFER_SRC != 0);
    });
    let (row, rows, x, y) = region_shape(image, region);
    let (memory, start, _) = buffer_range(buffer, region.buffer_offset, (row * rows) as u64);
    with(|script| {
        let source = script.world.memories[&memory].bytes[start..start + row * rows].to_vec();
        if let Some(entry) = script.world.images.get_mut(&image) {
            let pitch = usize::try_from(entry.extent[0]).unwrap_or(0)
                * usize::try_from(bytes_per_texel(entry.format)).unwrap_or(4);
            copy_rows(
                &source,
                &mut entry.texels,
                (rows, row),
                |line| line * row,
                |line| (y + line) * pitch + x,
            );
        }
        script.world.ops.push(Op::BufferToImage {
            image,
            width: region.image_extent[0],
            height: region.image_extent[1],
        });
    });
}

/// The swapchain-image readback of the presenter's capture: the pixel of the last clear.
fn copy_capture_pixel(buffer: u64, offset: u64) {
    let clear = with(|script| script.clears.last().copied()).unwrap_or([0.0; 4]);
    // Bgra8: blue, green, red, alpha.
    let bgra = [
        byte(clear[2]),
        byte(clear[1]),
        byte(clear[0]),
        byte(clear[3]),
    ];
    let (memory, start, _) = buffer_range(buffer, offset, 4);
    with(|script| {
        if let Some(block) = script.world.memories.get_mut(&memory) {
            block.bytes[start..start + 4].copy_from_slice(&bgra);
        }
    });
}

unsafe extern "system" fn cmd_copy_image_to_buffer(
    _cmd: CommandBufferHandle,
    image: u64,
    layout: u32,
    buffer: u64,
    count: u32,
    regions: *const BufferImageCopy,
) {
    note("vkCmdCopyImageToBuffer");
    assert_eq!(count, 1);
    let region = unsafe { &*regions };
    let known = with(|script| script.world.images.contains_key(&image));
    if !known {
        // A swapchain image: the presenter's one-pixel capture.
        assert_eq!(region.image_extent, [1, 1, 1], "one pixel");
        copy_capture_pixel(buffer, region.buffer_offset);
        return;
    }
    with(|script| {
        let entry = &script.world.images[&image];
        assert!(
            entry.bound.is_some(),
            "the image is used before it is bound"
        );
        assert_eq!(layout, vp::LAYOUT_TRANSFER_SRC);
        assert_eq!(
            entry.layout,
            vp::LAYOUT_TRANSFER_SRC,
            "the image is not in the layout it is copied in"
        );
        assert!(entry.usage & vr::IMAGE_USAGE_TRANSFER_SRC != 0);
        assert!(script.world.buffers[&buffer].usage & vr::BUFFER_USAGE_TRANSFER_DST != 0);
    });
    let (row, rows, x, y) = region_shape(image, region);
    let (memory, start, _) = buffer_range(buffer, region.buffer_offset, (row * rows) as u64);
    with(|script| {
        let (texels, pitch) = {
            let entry = &script.world.images[&image];
            let pitch = usize::try_from(entry.extent[0]).unwrap_or(0)
                * usize::try_from(bytes_per_texel(entry.format)).unwrap_or(4);
            (entry.texels.clone(), pitch)
        };
        if let Some(block) = script.world.memories.get_mut(&memory) {
            copy_rows(
                &texels,
                &mut block.bytes,
                (rows, row),
                |line| (y + line) * pitch + x,
                |line| start + line * row,
            );
        }
        script.world.ops.push(Op::ImageToBuffer {
            image,
            width: region.image_extent[0],
            height: region.image_extent[1],
        });
    });
}

/// A colour channel from 0 to 1 as a byte.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=255 first"
)]
fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

unsafe extern "system" fn format_properties(
    _gpu: PhysicalDeviceHandle,
    format: u32,
    out: *mut FormatProperties,
) {
    note("vkGetPhysicalDeviceFormatProperties");
    let optimal = if format == vr::FORMAT_D32_SFLOAT {
        with(|script| script.world.depth_features)
    } else {
        u32::MAX
    };
    unsafe {
        out.write(FormatProperties {
            linear: 0,
            optimal,
            buffer: 0,
        });
    }
}

/// Records the layout changes and the global barriers of one `vkCmdPipelineBarrier2` and checks
/// that each image barrier starts from the layout the image is in.
pub unsafe fn record_barrier(info: *const DependencyInfo) {
    let info = unsafe { &*info };
    if info.memory_barrier_count > 0 {
        let barrier = unsafe { &*info.memory_barriers.cast::<MemoryBarrier2>() };
        assert_eq!(barrier.s_type, vr::STYPE_MEMORY_BARRIER_2);
        with(|script| {
            script.world.ops.push(Op::GlobalBarrier);
            script.world.global_masks.push((
                barrier.src_stage_mask,
                barrier.src_access_mask,
                barrier.dst_stage_mask,
                barrier.dst_access_mask,
            ));
        });
    }
    for index in 0..usize::try_from(info.image_memory_barrier_count).unwrap_or(0) {
        let barrier: &ImageMemoryBarrier2 = unsafe { &*info.image_memory_barriers.add(index) };
        assert_eq!(barrier.s_type, vp::STYPE_IMAGE_MEMORY_BARRIER_2);
        with(|script| apply_image_barrier(script, barrier));
    }
}

/// Checks that the barrier starts from the layout the image is in, then moves the image to the
/// new one.
fn apply_image_barrier(script: &mut super::Script, barrier: &ImageMemoryBarrier2) {
    script
        .barriers
        .push((barrier.old_layout, barrier.new_layout));
    script.image_barriers.push(ImageBarrierRecord {
        image: barrier.image,
        aspect: barrier.subresource_range.aspect_mask,
        old: barrier.old_layout,
        new: barrier.new_layout,
        src_stage: barrier.src_stage_mask,
        dst_stage: barrier.dst_stage_mask,
        dst_access: barrier.dst_access_mask,
    });
    let Some(entry) = script.world.images.get_mut(&barrier.image) else {
        return;
    };
    assert!(
        entry.bound.is_some(),
        "a barrier on an image with no memory"
    );
    assert!(
        barrier.old_layout == vp::LAYOUT_UNDEFINED || barrier.old_layout == entry.layout,
        "the barrier says the image is in layout {}, but it is in {}",
        barrier.old_layout,
        entry.layout
    );
    entry.layout = barrier.new_layout;
    script.world.ops.push(Op::ImageBarrier {
        image: barrier.image,
        old: barrier.old_layout,
        new: barrier.new_layout,
    });
}

/// Copies `rows` rows of `row` bytes; row `line` is read at `from(line)` and written at
/// `to(line)`.
fn copy_rows(
    source: &[u8],
    target: &mut [u8],
    (rows, row): (usize, usize),
    from: impl Fn(usize) -> usize,
    to: impl Fn(usize) -> usize,
) {
    for line in 0..rows {
        let (read, write) = (from(line), to(line));
        target[write..write + row].copy_from_slice(&source[read..read + row]);
    }
}

/// Memory types for the fake: type 0 device local, type 1 host visible and coherent.
pub fn memory_properties() -> PhysicalDeviceMemoryProperties {
    let mut properties = PhysicalDeviceMemoryProperties::zeroed();
    properties.memory_type_count = 2;
    properties.memory_types[0].property_flags = vr::MEMORY_DEVICE_LOCAL;
    properties.memory_types[1].property_flags = vp::MEMORY_HOST_VISIBLE | vp::MEMORY_HOST_COHERENT;
    properties
}
