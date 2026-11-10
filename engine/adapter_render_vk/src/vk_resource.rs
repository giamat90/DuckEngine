//! Vulkan declarations for images, samplers, copies and memory barriers (card 6-8).
//!
//! Layouts and numbers follow Vulkan-Headers 1.3.296 for 64-bit targets; the tests pin them to
//! numbers copied from `vulkan_core.h`, not from these constants.

use crate::vk_present::SubresourceRange;
use core::ffi::c_void;

/// `VkSampler` (a non-dispatchable handle).
pub type SamplerHandle = u64;

/// `VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO`.
pub const STYPE_IMAGE_CREATE_INFO: u32 = 14;
/// `VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO`.
pub const STYPE_SAMPLER_CREATE_INFO: u32 = 31;
/// `VK_STRUCTURE_TYPE_MEMORY_BARRIER_2`.
pub const STYPE_MEMORY_BARRIER_2: u32 = 1_000_314_000;

/// `VK_FORMAT_R8G8B8A8_UNORM`.
pub const FORMAT_R8G8B8A8_UNORM: u32 = 37;
/// `VK_FORMAT_D32_SFLOAT`.
pub const FORMAT_D32_SFLOAT: u32 = 126;

/// `VK_IMAGE_TYPE_2D`.
pub const IMAGE_TYPE_2D: u32 = 1;
/// `VK_IMAGE_TILING_OPTIMAL`.
pub const TILING_OPTIMAL: u32 = 0;
/// `VK_SAMPLE_COUNT_1_BIT`.
pub const SAMPLES_1: u32 = 1;

/// `VK_IMAGE_USAGE_TRANSFER_SRC_BIT`.
pub const IMAGE_USAGE_TRANSFER_SRC: u32 = 0x1;
/// `VK_IMAGE_USAGE_TRANSFER_DST_BIT`.
pub const IMAGE_USAGE_TRANSFER_DST: u32 = 0x2;
/// `VK_IMAGE_USAGE_SAMPLED_BIT`.
pub const IMAGE_USAGE_SAMPLED: u32 = 0x4;
/// `VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT`.
pub const IMAGE_USAGE_DEPTH_ATTACHMENT: u32 = 0x20;

/// `VK_BUFFER_USAGE_TRANSFER_SRC_BIT`.
pub const BUFFER_USAGE_TRANSFER_SRC: u32 = 0x1;
/// `VK_BUFFER_USAGE_TRANSFER_DST_BIT`.
pub const BUFFER_USAGE_TRANSFER_DST: u32 = 0x2;
/// `VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT`.
pub const BUFFER_USAGE_UNIFORM: u32 = 0x10;
/// `VK_BUFFER_USAGE_INDEX_BUFFER_BIT`.
pub const BUFFER_USAGE_INDEX: u32 = 0x40;
/// `VK_BUFFER_USAGE_VERTEX_BUFFER_BIT`.
pub const BUFFER_USAGE_VERTEX: u32 = 0x80;

/// `VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL`.
pub const LAYOUT_DEPTH_ATTACHMENT: u32 = 3;
/// `VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL`.
pub const LAYOUT_SHADER_READ: u32 = 5;
/// `VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL`.
pub const LAYOUT_TRANSFER_DST: u32 = 7;

/// `VK_IMAGE_ASPECT_DEPTH_BIT`.
pub const ASPECT_DEPTH: u32 = 0x2;

/// `VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT`.
pub const MEMORY_DEVICE_LOCAL: u32 = 0x1;

/// `VK_FORMAT_FEATURE_DEPTH_STENCIL_ATTACHMENT_BIT`.
pub const FEATURE_DEPTH_ATTACHMENT: u32 = 0x200;

/// `VK_FORMAT_FEATURE_TRANSFER_SRC_BIT`.
pub const FEATURE_TRANSFER_SRC: u32 = 0x4000;

/// `VK_FILTER_NEAREST`.
pub const FILTER_NEAREST: u32 = 0;
/// `VK_FILTER_LINEAR`.
pub const FILTER_LINEAR: u32 = 1;
/// `VK_SAMPLER_MIPMAP_MODE_NEAREST`.
pub const MIPMAP_NEAREST: u32 = 0;
/// `VK_SAMPLER_MIPMAP_MODE_LINEAR`.
pub const MIPMAP_LINEAR: u32 = 1;
/// `VK_SAMPLER_ADDRESS_MODE_REPEAT`.
pub const ADDRESS_REPEAT: u32 = 0;
/// `VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE`.
pub const ADDRESS_CLAMP_TO_EDGE: u32 = 2;
/// `VK_LOD_CLAMP_NONE`: the sampler may use every mip level.
pub const LOD_CLAMP_NONE: f32 = 1000.0;

/// `VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT`, as a source or destination of a global barrier.
pub const STAGE_ALL_COMMANDS: u64 = 0x1_0000;
/// `VK_PIPELINE_STAGE_2_ALL_TRANSFER_BIT`.
pub const STAGE_ALL_TRANSFER: u64 = 0x1000;
/// `VK_ACCESS_2_TRANSFER_WRITE_BIT`.
pub const ACCESS_TRANSFER_WRITE: u64 = 0x1000;
/// `VK_ACCESS_2_TRANSFER_READ_BIT`.
pub const ACCESS_TRANSFER_READ: u64 = 0x800;

/// `VK_PIPELINE_STAGE_2_HOST_BIT`.
pub const STAGE_HOST: u64 = 0x4000;

/// `VK_ACCESS_2_HOST_READ_BIT`.
pub const ACCESS_HOST_READ: u64 = 0x2000;
/// `VK_ACCESS_2_MEMORY_READ_BIT`.
pub const ACCESS_MEMORY_READ: u64 = 0x8000;
/// `VK_ACCESS_2_MEMORY_WRITE_BIT`.
pub const ACCESS_MEMORY_WRITE: u64 = 0x1_0000;

/// `VkImageCreateInfo`.
#[repr(C)]
pub struct ImageCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub image_type: u32,
    pub format: u32,
    pub extent: [u32; 3],
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: u32,
    pub tiling: u32,
    pub usage: u32,
    pub sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub queue_family_indices: *const u32,
    pub initial_layout: u32,
}

/// `VkSamplerCreateInfo`.
#[repr(C)]
pub struct SamplerCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub mag_filter: u32,
    pub min_filter: u32,
    pub mipmap_mode: u32,
    pub address_mode_u: u32,
    pub address_mode_v: u32,
    pub address_mode_w: u32,
    pub mip_lod_bias: f32,
    pub anisotropy_enable: u32,
    pub max_anisotropy: f32,
    pub compare_enable: u32,
    pub compare_op: u32,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: u32,
    pub unnormalized_coordinates: u32,
}

/// `VkBufferCopy`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BufferCopy {
    pub src_offset: u64,
    pub dst_offset: u64,
    pub size: u64,
}

/// `VkMemoryBarrier2`: a barrier on all memory, not one resource.
#[repr(C)]
pub struct MemoryBarrier2 {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub src_stage_mask: u64,
    pub src_access_mask: u64,
    pub dst_stage_mask: u64,
    pub dst_access_mask: u64,
}

/// `VkFormatProperties`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FormatProperties {
    pub linear: u32,
    pub optimal: u32,
    pub buffer: u32,
}

impl SubresourceRange {
    /// Every mip level of a colour or depth image of `levels` levels, one layer.
    #[must_use]
    pub const fn all_levels(aspect_mask: u32, levels: u32) -> Self {
        Self {
            aspect_mask,
            base_mip_level: 0,
            level_count: levels,
            base_array_layer: 0,
            layer_count: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    /// The numbers are copied from `vulkan_core.h` (v1.3.296), not from the constants.
    #[test]
    fn the_numbers_are_the_ones_of_the_header() {
        assert_eq!(STYPE_IMAGE_CREATE_INFO, 14);
        assert_eq!(STYPE_SAMPLER_CREATE_INFO, 31);
        assert_eq!(STYPE_MEMORY_BARRIER_2, 1_000_314_000);
        assert_eq!(FORMAT_R8G8B8A8_UNORM, 37);
        assert_eq!(FORMAT_D32_SFLOAT, 126);
        assert_eq!(IMAGE_TYPE_2D, 1);
        assert_eq!(TILING_OPTIMAL, 0);
        assert_eq!(SAMPLES_1, 1);
        assert_eq!(IMAGE_USAGE_TRANSFER_SRC, 0x1);
        assert_eq!(IMAGE_USAGE_TRANSFER_DST, 0x2);
        assert_eq!(IMAGE_USAGE_SAMPLED, 0x4);
        assert_eq!(IMAGE_USAGE_DEPTH_ATTACHMENT, 0x20);
        assert_eq!(BUFFER_USAGE_TRANSFER_SRC, 0x1);
        assert_eq!(BUFFER_USAGE_TRANSFER_DST, 0x2);
        assert_eq!(BUFFER_USAGE_UNIFORM, 0x10);
        assert_eq!(BUFFER_USAGE_INDEX, 0x40);
        assert_eq!(BUFFER_USAGE_VERTEX, 0x80);
    }

    #[test]
    fn the_layouts_aspects_and_features_are_the_ones_of_the_header() {
        assert_eq!(LAYOUT_DEPTH_ATTACHMENT, 3);
        assert_eq!(LAYOUT_SHADER_READ, 5);
        assert_eq!(LAYOUT_TRANSFER_DST, 7);
        assert_eq!(ASPECT_DEPTH, 0x2);
        assert_eq!(MEMORY_DEVICE_LOCAL, 0x1);
        assert_eq!(FEATURE_TRANSFER_SRC, 0x4000);
        assert_eq!(FEATURE_DEPTH_ATTACHMENT, 0x200);
    }

    #[test]
    fn the_sampler_and_sync_numbers_are_the_ones_of_the_header() {
        assert_eq!((FILTER_NEAREST, FILTER_LINEAR), (0, 1));
        assert_eq!((MIPMAP_NEAREST, MIPMAP_LINEAR), (0, 1));
        assert_eq!((ADDRESS_REPEAT, ADDRESS_CLAMP_TO_EDGE), (0, 2));
        assert!((LOD_CLAMP_NONE - 1000.0).abs() < f32::EPSILON);
        assert_eq!(STAGE_ALL_COMMANDS, 0x1_0000);
        assert_eq!(STAGE_ALL_TRANSFER, 0x1000);
        assert_eq!(ACCESS_TRANSFER_WRITE, 0x1000);
        assert_eq!(ACCESS_TRANSFER_READ, 0x800);
        assert_eq!(STAGE_HOST, 0x4000);
        assert_eq!(ACCESS_HOST_READ, 0x2000);
        assert_eq!(ACCESS_MEMORY_READ, 0x8000);
        assert_eq!(ACCESS_MEMORY_WRITE, 0x1_0000);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_layouts_are_the_ones_of_the_header() {
        assert_eq!(size_of::<ImageCreateInfo>(), 88);
        assert_eq!(offset_of!(ImageCreateInfo, extent), 28);
        assert_eq!(offset_of!(ImageCreateInfo, mip_levels), 40);
        assert_eq!(offset_of!(ImageCreateInfo, usage), 56);
        assert_eq!(offset_of!(ImageCreateInfo, queue_family_indices), 72);
        assert_eq!(offset_of!(ImageCreateInfo, initial_layout), 80);
        assert_eq!(size_of::<SamplerCreateInfo>(), 80);
        assert_eq!(offset_of!(SamplerCreateInfo, address_mode_u), 32);
        assert_eq!(offset_of!(SamplerCreateInfo, min_lod), 64);
        assert_eq!(offset_of!(SamplerCreateInfo, unnormalized_coordinates), 76);
        assert_eq!(size_of::<BufferCopy>(), 24);
        assert_eq!(size_of::<MemoryBarrier2>(), 48);
        assert_eq!(offset_of!(MemoryBarrier2, dst_access_mask), 40);
        assert_eq!(size_of::<FormatProperties>(), 12);
    }
}
