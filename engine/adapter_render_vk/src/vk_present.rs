//! Hand-written declarations for presenting (card 6-7): surfaces, swapchains, synchronization,
//! command buffers, dynamic rendering and the one-off readback buffer. Layouts follow
//! `vulkan_core.h` (Vulkan-Headers 1.3.296) for 64-bit targets, checked by hand; the tests pin
//! the sizes and offsets that matter, not the header itself.

use crate::vk::Bool32;
use core::ffi::{c_char, c_void};

/// Non-dispatchable handles are 64-bit integers on 64-bit targets.
pub type SurfaceHandle = u64;
/// `VkSwapchainKHR`.
pub type SwapchainHandle = u64;
/// `VkSemaphore`.
pub type SemaphoreHandle = u64;
/// `VkFence`.
pub type FenceHandle = u64;
/// `VkCommandPool`.
pub type CommandPoolHandle = u64;
/// `VkImage`.
pub type ImageHandle = u64;
/// `VkImageView`.
pub type ImageViewHandle = u64;
/// `VkBuffer`.
pub type BufferHandle = u64;
/// `VkDeviceMemory`.
pub type MemoryHandle = u64;

/// `VkCommandBuffer`, a dispatchable handle.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandBufferHandle(pub *mut c_void);

impl CommandBufferHandle {
    /// `VK_NULL_HANDLE`.
    pub const NULL: Self = Self(core::ptr::null_mut());
}

// Results.
/// `VK_SUBOPTIMAL_KHR`: the swapchain still works but no longer matches the surface.
pub const SUBOPTIMAL: i32 = 1_000_001_003;
/// `VK_ERROR_OUT_OF_DATE_KHR`: the swapchain must be recreated.
pub const ERROR_OUT_OF_DATE: i32 = -1_000_001_004;
/// `VK_ERROR_SURFACE_LOST_KHR`.
pub const ERROR_SURFACE_LOST: i32 = -1_000_000_000;

// Structure types.
/// `VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO`.
pub const STYPE_MEMORY_ALLOCATE_INFO: u32 = 5;
/// `VK_STRUCTURE_TYPE_FENCE_CREATE_INFO`.
pub const STYPE_FENCE_CREATE_INFO: u32 = 8;
/// `VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO`.
pub const STYPE_SEMAPHORE_CREATE_INFO: u32 = 9;
/// `VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO`.
pub const STYPE_BUFFER_CREATE_INFO: u32 = 12;
/// `VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO`.
pub const STYPE_IMAGE_VIEW_CREATE_INFO: u32 = 15;
/// `VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO`.
pub const STYPE_COMMAND_POOL_CREATE_INFO: u32 = 39;
/// `VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO`.
pub const STYPE_COMMAND_BUFFER_ALLOCATE_INFO: u32 = 40;
/// `VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO`.
pub const STYPE_COMMAND_BUFFER_BEGIN_INFO: u32 = 42;
/// `VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR`.
pub const STYPE_SWAPCHAIN_CREATE_INFO: u32 = 1_000_001_000;
/// `VK_STRUCTURE_TYPE_PRESENT_INFO_KHR`.
pub const STYPE_PRESENT_INFO: u32 = 1_000_001_001;
/// `VK_STRUCTURE_TYPE_XLIB_SURFACE_CREATE_INFO_KHR`.
#[cfg(target_os = "linux")]
pub const STYPE_XLIB_SURFACE_CREATE_INFO: u32 = 1_000_004_000;
/// `VK_STRUCTURE_TYPE_WIN32_SURFACE_CREATE_INFO_KHR`.
#[cfg(windows)]
pub const STYPE_WIN32_SURFACE_CREATE_INFO: u32 = 1_000_009_000;
/// `VK_STRUCTURE_TYPE_RENDERING_INFO`.
pub const STYPE_RENDERING_INFO: u32 = 1_000_044_000;
/// `VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO`.
pub const STYPE_RENDERING_ATTACHMENT_INFO: u32 = 1_000_044_001;
/// `VK_STRUCTURE_TYPE_DEPENDENCY_INFO`.
pub const STYPE_DEPENDENCY_INFO: u32 = 1_000_314_003;
/// `VK_STRUCTURE_TYPE_SUBMIT_INFO_2`.
pub const STYPE_SUBMIT_INFO_2: u32 = 1_000_314_004;
/// `VK_STRUCTURE_TYPE_SEMAPHORE_SUBMIT_INFO`.
pub const STYPE_SEMAPHORE_SUBMIT_INFO: u32 = 1_000_314_005;
/// `VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO`.
pub const STYPE_COMMAND_BUFFER_SUBMIT_INFO: u32 = 1_000_314_006;
/// `VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2`.
pub const STYPE_IMAGE_MEMORY_BARRIER_2: u32 = 1_000_314_002;

// Formats, colour spaces, present modes.
/// `VK_FORMAT_B8G8R8A8_UNORM`: the swapchain format of the engine (design note section 3).
pub const FORMAT_B8G8R8A8_UNORM: u32 = 44;
/// `VK_COLOR_SPACE_SRGB_NONLINEAR_KHR`.
pub const COLOR_SPACE_SRGB_NONLINEAR: u32 = 0;
/// `VK_PRESENT_MODE_FIFO_KHR`: always available, waits for the display.
pub const PRESENT_MODE_FIFO: u32 = 2;

// Usage, sharing, alpha, aspect.
/// `VK_IMAGE_USAGE_TRANSFER_SRC_BIT`.
pub const IMAGE_USAGE_TRANSFER_SRC: u32 = 0x1;
/// `VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT`.
pub const IMAGE_USAGE_COLOR_ATTACHMENT: u32 = 0x10;
/// `VK_SHARING_MODE_EXCLUSIVE`.
pub const SHARING_EXCLUSIVE: u32 = 0;
/// `VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR`.
pub const COMPOSITE_ALPHA_OPAQUE: u32 = 0x1;
/// `VK_IMAGE_ASPECT_COLOR_BIT`.
pub const ASPECT_COLOR: u32 = 0x1;
/// `VK_IMAGE_VIEW_TYPE_2D`.
pub const IMAGE_VIEW_TYPE_2D: u32 = 1;

// Layouts.
/// `VK_IMAGE_LAYOUT_UNDEFINED`.
pub const LAYOUT_UNDEFINED: u32 = 0;
/// `VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL`.
pub const LAYOUT_COLOR_ATTACHMENT: u32 = 2;
/// `VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL`.
pub const LAYOUT_TRANSFER_SRC: u32 = 6;
/// `VK_IMAGE_LAYOUT_PRESENT_SRC_KHR`.
pub const LAYOUT_PRESENT_SRC: u32 = 1_000_001_002;

// Pipeline stages and accesses (synchronization2, 64-bit masks).
/// `VK_PIPELINE_STAGE_2_NONE`.
pub const STAGE_NONE: u64 = 0;
/// `VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT`.
pub const STAGE_COLOR_ATTACHMENT_OUTPUT: u64 = 0x400;
/// `VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT`.
pub const STAGE_ALL_COMMANDS: u64 = 0x1_0000;
/// `VK_PIPELINE_STAGE_2_ALL_TRANSFER_BIT`.
pub const STAGE_ALL_TRANSFER: u64 = 0x1000;
/// `VK_ACCESS_2_NONE`.
pub const ACCESS_NONE: u64 = 0;
/// `VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT`.
pub const ACCESS_COLOR_ATTACHMENT_WRITE: u64 = 0x100;
/// `VK_ACCESS_2_TRANSFER_READ_BIT`.
pub const ACCESS_TRANSFER_READ: u64 = 0x800;

// Rendering, commands, fences.
/// `VK_ATTACHMENT_LOAD_OP_CLEAR`.
pub const LOAD_OP_CLEAR: u32 = 1;
/// `VK_ATTACHMENT_STORE_OP_STORE`.
pub const STORE_OP_STORE: u32 = 0;
/// `VK_FENCE_CREATE_SIGNALED_BIT`.
pub const FENCE_CREATE_SIGNALED: u32 = 1;
/// `VK_COMMAND_POOL_CREATE_TRANSIENT_BIT`.
pub const COMMAND_POOL_TRANSIENT: u32 = 1;
/// `VK_COMMAND_BUFFER_LEVEL_PRIMARY`.
pub const COMMAND_BUFFER_LEVEL_PRIMARY: u32 = 0;
/// `VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT`.
pub const COMMAND_BUFFER_ONE_TIME_SUBMIT: u32 = 1;
/// `VK_BUFFER_USAGE_TRANSFER_DST_BIT`.
pub const BUFFER_USAGE_TRANSFER_DST: u32 = 0x2;
/// `VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT`.
pub const MEMORY_HOST_VISIBLE: u32 = 0x2;
/// `VK_MEMORY_PROPERTY_HOST_COHERENT_BIT`.
pub const MEMORY_HOST_COHERENT: u32 = 0x4;
/// The "wait for ever" timeout of the wait and acquire calls.
pub const FOREVER: u64 = u64::MAX;
/// `VK_QUEUE_FAMILY_IGNORED`.
pub const QUEUE_FAMILY_IGNORED: u32 = u32::MAX;
/// The `currentExtent` width/height that means "the surface follows the swapchain".
pub const EXTENT_FOLLOWS_SWAPCHAIN: u32 = u32::MAX;

/// Instance extension names.
pub const EXT_SURFACE: &core::ffi::CStr = c"VK_KHR_surface";
/// The Win32 surface extension.
#[cfg(windows)]
pub const EXT_WIN32_SURFACE: &core::ffi::CStr = c"VK_KHR_win32_surface";
/// The Xlib surface extension.
#[cfg(target_os = "linux")]
pub const EXT_XLIB_SURFACE: &core::ffi::CStr = c"VK_KHR_xlib_surface";
/// The device extension that makes swapchains.
pub const EXT_SWAPCHAIN: &core::ffi::CStr = c"VK_KHR_swapchain";

/// `VkExtent2D`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

/// `VkOffset2D`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offset2D {
    pub x: i32,
    pub y: i32,
}

/// `VkRect2D`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect2D {
    pub offset: Offset2D,
    pub extent: Extent2D,
}

/// `VkExtensionProperties`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExtensionProperties {
    pub extension_name: [c_char; 256],
    pub spec_version: u32,
}

impl ExtensionProperties {
    /// All zero, to be filled by the driver.
    pub const fn zeroed() -> Self {
        Self {
            extension_name: [0; 256],
            spec_version: 0,
        }
    }
}

/// `VkWin32SurfaceCreateInfoKHR`.
#[cfg(windows)]
#[repr(C)]
pub struct Win32SurfaceCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub hinstance: *const c_void,
    pub hwnd: *const c_void,
}

/// `VkXlibSurfaceCreateInfoKHR`.
#[cfg(target_os = "linux")]
#[repr(C)]
pub struct XlibSurfaceCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub display: *mut c_void,
    pub window: core::ffi::c_ulong,
}

/// `VkSurfaceCapabilitiesKHR`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfaceCapabilities {
    pub min_image_count: u32,
    pub max_image_count: u32,
    pub current_extent: Extent2D,
    pub min_image_extent: Extent2D,
    pub max_image_extent: Extent2D,
    pub max_image_array_layers: u32,
    pub supported_transforms: u32,
    pub current_transform: u32,
    pub supported_composite_alpha: u32,
    pub supported_usage_flags: u32,
}

/// `VkSurfaceFormatKHR`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfaceFormat {
    pub format: u32,
    pub color_space: u32,
}

/// `VkSwapchainCreateInfoKHR`.
#[repr(C)]
pub struct SwapchainCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub surface: SurfaceHandle,
    pub min_image_count: u32,
    pub image_format: u32,
    pub image_color_space: u32,
    pub image_extent: Extent2D,
    pub image_array_layers: u32,
    pub image_usage: u32,
    pub image_sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub queue_family_indices: *const u32,
    pub pre_transform: u32,
    pub composite_alpha: u32,
    pub present_mode: u32,
    pub clipped: Bool32,
    pub old_swapchain: SwapchainHandle,
}

/// `VkSemaphoreCreateInfo` and `VkFenceCreateInfo` share this shape.
#[repr(C)]
pub struct SyncCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
}

/// `VkCommandPoolCreateInfo`.
#[repr(C)]
pub struct CommandPoolCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_family_index: u32,
}

/// `VkCommandBufferAllocateInfo`.
#[repr(C)]
pub struct CommandBufferAllocateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub command_pool: CommandPoolHandle,
    pub level: u32,
    pub command_buffer_count: u32,
}

/// `VkCommandBufferBeginInfo`.
#[repr(C)]
pub struct CommandBufferBeginInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub inheritance_info: *const c_void,
}

/// `VkImageSubresourceRange`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubresourceRange {
    pub aspect_mask: u32,
    pub base_mip_level: u32,
    pub level_count: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

impl SubresourceRange {
    /// The one colour level and layer of a swapchain image.
    pub const COLOR: Self = Self {
        aspect_mask: ASPECT_COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
}

/// `VkImageViewCreateInfo` (identity component mapping).
#[repr(C)]
pub struct ImageViewCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub image: ImageHandle,
    pub view_type: u32,
    pub format: u32,
    pub components: [u32; 4],
    pub subresource_range: SubresourceRange,
}

/// `VkImageMemoryBarrier2`.
#[repr(C)]
pub struct ImageMemoryBarrier2 {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub src_stage_mask: u64,
    pub src_access_mask: u64,
    pub dst_stage_mask: u64,
    pub dst_access_mask: u64,
    pub old_layout: u32,
    pub new_layout: u32,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub image: ImageHandle,
    pub subresource_range: SubresourceRange,
}

/// `VkDependencyInfo`.
#[repr(C)]
pub struct DependencyInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub dependency_flags: u32,
    pub memory_barrier_count: u32,
    pub memory_barriers: *const c_void,
    pub buffer_memory_barrier_count: u32,
    pub buffer_memory_barriers: *const c_void,
    pub image_memory_barrier_count: u32,
    pub image_memory_barriers: *const ImageMemoryBarrier2,
}

/// `VkClearValue` as a colour of four floats (the union's other members are not used).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClearColor {
    pub float32: [f32; 4],
}

/// `VkRenderingAttachmentInfo`.
#[repr(C)]
pub struct RenderingAttachmentInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub image_view: ImageViewHandle,
    pub image_layout: u32,
    pub resolve_mode: u32,
    pub resolve_image_view: ImageViewHandle,
    pub resolve_image_layout: u32,
    pub load_op: u32,
    pub store_op: u32,
    pub clear_value: ClearColor,
}

/// `VkRenderingInfo`.
#[repr(C)]
pub struct RenderingInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub render_area: Rect2D,
    pub layer_count: u32,
    pub view_mask: u32,
    pub color_attachment_count: u32,
    pub color_attachments: *const RenderingAttachmentInfo,
    pub depth_attachment: *const RenderingAttachmentInfo,
    pub stencil_attachment: *const RenderingAttachmentInfo,
}

/// `VkSemaphoreSubmitInfo`.
#[repr(C)]
pub struct SemaphoreSubmitInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub semaphore: SemaphoreHandle,
    pub value: u64,
    pub stage_mask: u64,
    pub device_index: u32,
}

/// `VkCommandBufferSubmitInfo`.
#[repr(C)]
pub struct CommandBufferSubmitInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub command_buffer: CommandBufferHandle,
    pub device_mask: u32,
}

/// `VkSubmitInfo2`.
#[repr(C)]
pub struct SubmitInfo2 {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub wait_semaphore_info_count: u32,
    pub wait_semaphore_infos: *const SemaphoreSubmitInfo,
    pub command_buffer_info_count: u32,
    pub command_buffer_infos: *const CommandBufferSubmitInfo,
    pub signal_semaphore_info_count: u32,
    pub signal_semaphore_infos: *const SemaphoreSubmitInfo,
}

/// `VkPresentInfoKHR`.
#[repr(C)]
pub struct PresentInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub wait_semaphores: *const SemaphoreHandle,
    pub swapchain_count: u32,
    pub swapchains: *const SwapchainHandle,
    pub image_indices: *const u32,
    pub results: *mut i32,
}

/// `VkBufferCreateInfo`.
#[repr(C)]
pub struct BufferCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub size: u64,
    pub usage: u32,
    pub sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub queue_family_indices: *const u32,
}

/// `VkMemoryRequirements`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemoryRequirements {
    pub size: u64,
    pub alignment: u64,
    pub memory_type_bits: u32,
}

/// `VkMemoryAllocateInfo`.
#[repr(C)]
pub struct MemoryAllocateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub allocation_size: u64,
    pub memory_type_index: u32,
}

/// `VkMemoryType`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemoryType {
    pub property_flags: u32,
    pub heap_index: u32,
}

/// `VkMemoryHeap`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemoryHeap {
    pub size: u64,
    pub flags: u32,
}

/// The most memory types and heaps Vulkan allows.
pub const MAX_MEMORY_TYPES: usize = 32;
/// The most memory heaps Vulkan allows.
pub const MAX_MEMORY_HEAPS: usize = 16;

/// `VkPhysicalDeviceMemoryProperties`.
#[expect(
    clippy::struct_field_names,
    reason = "the field names are the Vulkan ones"
)]
#[repr(C)]
pub struct PhysicalDeviceMemoryProperties {
    pub memory_type_count: u32,
    pub memory_types: [MemoryType; MAX_MEMORY_TYPES],
    pub memory_heap_count: u32,
    pub memory_heaps: [MemoryHeap; MAX_MEMORY_HEAPS],
}

impl PhysicalDeviceMemoryProperties {
    /// All zero, to be filled by the driver.
    pub const fn zeroed() -> Self {
        Self {
            memory_type_count: 0,
            memory_types: [MemoryType {
                property_flags: 0,
                heap_index: 0,
            }; MAX_MEMORY_TYPES],
            memory_heap_count: 0,
            memory_heaps: [MemoryHeap { size: 0, flags: 0 }; MAX_MEMORY_HEAPS],
        }
    }
}

/// `VkImageSubresourceLayers`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubresourceLayers {
    pub aspect_mask: u32,
    pub mip_level: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

/// `VkBufferImageCopy`.
#[repr(C)]
pub struct BufferImageCopy {
    pub buffer_offset: u64,
    pub buffer_row_length: u32,
    pub buffer_image_height: u32,
    pub image_subresource: SubresourceLayers,
    pub image_offset: [i32; 3],
    pub image_extent: [u32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    /// The numbers are copied from `vulkan_core.h`, not from the constants, so a wrong constant
    /// fails here (the fake driver and the code share the constants and cannot catch it).
    #[test]
    fn the_structure_types_are_the_ones_of_the_header() {
        let header = [
            (STYPE_MEMORY_ALLOCATE_INFO, 5),
            (STYPE_FENCE_CREATE_INFO, 8),
            (STYPE_SEMAPHORE_CREATE_INFO, 9),
            (STYPE_BUFFER_CREATE_INFO, 12),
            (STYPE_IMAGE_VIEW_CREATE_INFO, 15),
            (STYPE_COMMAND_POOL_CREATE_INFO, 39),
            (STYPE_COMMAND_BUFFER_ALLOCATE_INFO, 40),
            (STYPE_COMMAND_BUFFER_BEGIN_INFO, 42),
            (STYPE_SWAPCHAIN_CREATE_INFO, 1_000_001_000),
            (STYPE_PRESENT_INFO, 1_000_001_001),
            (STYPE_RENDERING_INFO, 1_000_044_000),
            (STYPE_RENDERING_ATTACHMENT_INFO, 1_000_044_001),
            (STYPE_DEPENDENCY_INFO, 1_000_314_003),
            (STYPE_IMAGE_MEMORY_BARRIER_2, 1_000_314_002),
            (STYPE_SUBMIT_INFO_2, 1_000_314_004),
            (STYPE_SEMAPHORE_SUBMIT_INFO, 1_000_314_005),
            (STYPE_COMMAND_BUFFER_SUBMIT_INFO, 1_000_314_006),
        ];
        for (constant, expected) in header {
            assert_eq!(constant, expected);
        }
    }

    #[test]
    fn the_surface_layouts_match_the_headers_on_64_bit() {
        assert_eq!(size_of::<Extent2D>(), 8);
        assert_eq!(size_of::<Rect2D>(), 16);
        assert_eq!(size_of::<ExtensionProperties>(), 260);
        #[cfg(windows)]
        assert_eq!(size_of::<Win32SurfaceCreateInfo>(), 40);
        #[cfg(target_os = "linux")]
        assert_eq!(size_of::<XlibSurfaceCreateInfo>(), 40);
        assert_eq!(size_of::<SurfaceCapabilities>(), 52);
        assert_eq!(size_of::<SurfaceFormat>(), 8);
    }

    #[test]
    fn the_swapchain_and_sync_layouts_match_the_headers_on_64_bit() {
        assert_eq!(size_of::<SwapchainCreateInfo>(), 104);
        assert_eq!(offset_of!(SwapchainCreateInfo, image_extent), 44);
        assert_eq!(offset_of!(SwapchainCreateInfo, queue_family_indices), 72);
        assert_eq!(offset_of!(SwapchainCreateInfo, old_swapchain), 96);
        assert_eq!(size_of::<SyncCreateInfo>(), 24);
        assert_eq!(size_of::<CommandPoolCreateInfo>(), 24);
        assert_eq!(size_of::<CommandBufferAllocateInfo>(), 32);
        assert_eq!(size_of::<CommandBufferBeginInfo>(), 32);
        assert_eq!(size_of::<SubresourceRange>(), 20);
        assert_eq!(size_of::<ImageViewCreateInfo>(), 80);
    }

    #[test]
    fn the_recording_layouts_match_the_headers_on_64_bit() {
        assert_eq!(size_of::<ImageMemoryBarrier2>(), 96);
        assert_eq!(offset_of!(ImageMemoryBarrier2, old_layout), 48);
        assert_eq!(offset_of!(ImageMemoryBarrier2, image), 64);
        assert_eq!(size_of::<DependencyInfo>(), 64);
        assert_eq!(offset_of!(DependencyInfo, image_memory_barriers), 56);
        assert_eq!(size_of::<RenderingAttachmentInfo>(), 72);
        // The clear value is a union of 4-byte members: 4-byte aligned, so no padding before it.
        assert_eq!(offset_of!(RenderingAttachmentInfo, clear_value), 52);
        assert_eq!(size_of::<RenderingInfo>(), 72);
        assert_eq!(offset_of!(RenderingInfo, color_attachments), 48);
    }

    #[test]
    fn the_submit_and_memory_layouts_match_the_headers_on_64_bit() {
        assert_eq!(size_of::<SemaphoreSubmitInfo>(), 48);
        // 24 bytes of members, padded to the 8-byte alignment of the handle.
        assert_eq!(size_of::<CommandBufferSubmitInfo>(), 32);
        assert_eq!(size_of::<SubmitInfo2>(), 64);
        assert_eq!(offset_of!(SubmitInfo2, signal_semaphore_infos), 56);
        assert_eq!(size_of::<PresentInfo>(), 64);
        assert_eq!(size_of::<BufferCreateInfo>(), 56);
        assert_eq!(size_of::<MemoryRequirements>(), 24);
        assert_eq!(size_of::<MemoryAllocateInfo>(), 32);
        assert_eq!(size_of::<PhysicalDeviceMemoryProperties>(), 520);
        assert_eq!(
            offset_of!(PhysicalDeviceMemoryProperties, memory_heap_count),
            260
        );
        assert_eq!(size_of::<BufferImageCopy>(), 56);
    }
}
