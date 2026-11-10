//! The presenting half of the fake driver: surfaces, swapchains, frames, commands and the
//! readback buffer. Every object is registered when it is made and removed when it is destroyed,
//! so a test can ask what is left.

use super::{note, with};
use crate::vk::{self, Bool32, DeviceHandle, InstanceHandle, PhysicalDeviceHandle, QueueHandle};
use crate::vk_present::{
    self as vp, CommandBufferAllocateInfo, CommandBufferBeginInfo, CommandBufferHandle,
    CommandPoolCreateInfo, DependencyInfo, ExtensionProperties, PhysicalDeviceMemoryProperties,
    PresentInfo, RenderingInfo, SubmitInfo2, SurfaceCapabilities, SurfaceFormat,
    SwapchainCreateInfo, SyncCreateInfo,
};
use core::ffi::{c_char, c_void};

/// What `vkCreateSwapchainKHR` was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapchainRequest {
    pub format: u32,
    pub color_space: u32,
    pub width: u32,
    pub height: u32,
    pub min_image_count: u32,
    pub usage: u32,
    pub present_mode: u32,
    pub old_swapchain: u64,
}

/// What one `vkQueueSubmit2` carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitRecord {
    pub wait_semaphore: u64,
    pub wait_stage: u64,
    pub signal_semaphore: u64,
    pub fence: u64,
}

/// What one `vkQueuePresentKHR` carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentRecord {
    pub image: u32,
    pub wait_semaphore: u64,
}

/// `VK_ERROR_OUT_OF_HOST_MEMORY`.
const OUT_OF_HOST_MEMORY: i32 = -1;

/// Whether the script makes `call` fail.
fn refused(call: &str) -> bool {
    with(|script| script.fail_call == Some(call))
}

pub fn alloc(kind: &'static str) -> u64 {
    with(|script| {
        script.next_handle += 1;
        let handle = 0x10_000 + script.next_handle;
        script.live.push((kind, handle));
        handle
    })
}

pub fn release(kind: &'static str, handle: u64) {
    with(|script| {
        let position = script
            .live
            .iter()
            .position(|object| *object == (kind, handle));
        let Some(position) = position else {
            panic!(
                "{kind} {handle:#x} destroyed, but it is not alive (destroyed twice or never made)"
            );
        };
        script.live.remove(position);
    });
}

/// The address of the presenting function `name`, or null.
pub fn lookup(name: &str) -> *mut c_void {
    macro_rules! address {
        ($f:expr) => {
            ($f as *const ()).cast_mut().cast::<c_void>()
        };
    }
    match name {
        "vkEnumerateInstanceExtensionProperties" => {
            address!(enumerate_instance_extensions as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkEnumerateDeviceExtensionProperties" => {
            address!(enumerate_device_extensions as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkGetPhysicalDeviceMemoryProperties" => {
            address!(memory_properties as unsafe extern "system" fn(_, _))
        }
        "vkCreateWin32SurfaceKHR" | "vkCreateXlibSurfaceKHR" => {
            address!(create_surface as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroySurfaceKHR" => address!(destroy_surface as unsafe extern "system" fn(_, _, _)),
        "vkGetPhysicalDeviceSurfaceSupportKHR" => {
            address!(surface_support as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkGetPhysicalDeviceSurfaceCapabilitiesKHR" => {
            address!(surface_capabilities as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkGetPhysicalDeviceSurfaceFormatsKHR" => {
            address!(surface_formats as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkCreateSwapchainKHR" => {
            address!(create_swapchain as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroySwapchainKHR" => {
            address!(destroy_swapchain as unsafe extern "system" fn(_, _, _))
        }
        "vkGetSwapchainImagesKHR" => {
            address!(swapchain_images as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkAcquireNextImageKHR" => {
            address!(acquire_next_image as unsafe extern "system" fn(_, _, _, _, _, _) -> _)
        }
        "vkQueuePresentKHR" => address!(queue_present as unsafe extern "system" fn(_, _) -> _),
        _ => lookup_commands(name),
    }
}

/// The semaphore, fence, command and buffer functions.
fn lookup_commands(name: &str) -> *mut c_void {
    macro_rules! address {
        ($f:expr) => {
            ($f as *const ()).cast_mut().cast::<c_void>()
        };
    }
    match name {
        "vkCreateSemaphore" => {
            address!(create_semaphore as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroySemaphore" => address!(destroy_semaphore as unsafe extern "system" fn(_, _, _)),
        "vkCreateFence" => address!(create_fence as unsafe extern "system" fn(_, _, _, _) -> _),
        "vkDestroyFence" => address!(destroy_fence as unsafe extern "system" fn(_, _, _)),
        "vkWaitForFences" => {
            address!(wait_for_fences as unsafe extern "system" fn(_, _, _, _, _) -> _)
        }
        "vkResetFences" => address!(reset_fences as unsafe extern "system" fn(_, _, _) -> _),
        "vkCreateCommandPool" => {
            address!(create_command_pool as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyCommandPool" => {
            address!(destroy_command_pool as unsafe extern "system" fn(_, _, _))
        }
        "vkResetCommandPool" => {
            address!(reset_command_pool as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkAllocateCommandBuffers" => {
            address!(allocate_command_buffers as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkBeginCommandBuffer" => {
            address!(begin_command_buffer as unsafe extern "system" fn(_, _) -> _)
        }
        "vkEndCommandBuffer" => address!(end_command_buffer as unsafe extern "system" fn(_) -> _),
        "vkCreateImageView" => {
            address!(create_image_view as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyImageView" => address!(destroy_image_view as unsafe extern "system" fn(_, _, _)),
        "vkCmdPipelineBarrier2" => address!(cmd_barrier as unsafe extern "system" fn(_, _)),
        "vkCmdBeginRendering" => address!(cmd_begin_rendering as unsafe extern "system" fn(_, _)),
        "vkCmdEndRendering" => address!(cmd_end_rendering as unsafe extern "system" fn(_)),
        "vkQueueSubmit2" => address!(queue_submit2 as unsafe extern "system" fn(_, _, _, _) -> _),
        _ => super::resource::lookup(name),
    }
}

unsafe fn fill_extensions(names: &[&str], count: *mut u32, out: *mut ExtensionProperties) -> i32 {
    unsafe {
        if out.is_null() {
            *count = u32::try_from(names.len()).unwrap_or(0);
            return vk::SUCCESS;
        }
        let room = usize::try_from(*count).unwrap_or(0);
        let given = room.min(names.len());
        for (i, name) in names.iter().take(given).enumerate() {
            let mut entry = ExtensionProperties::zeroed();
            for (slot, byte) in entry.extension_name.iter_mut().zip(name.bytes()) {
                *slot = c_char::try_from(byte).unwrap_or(0);
            }
            out.add(i).write(entry);
        }
        *count = u32::try_from(given).unwrap_or(0);
    }
    vk::SUCCESS
}

unsafe extern "system" fn enumerate_instance_extensions(
    _layer: *const c_char,
    count: *mut u32,
    out: *mut ExtensionProperties,
) -> i32 {
    note("vkEnumerateInstanceExtensionProperties");
    let names = with(|script| script.instance_extensions.clone());
    unsafe { fill_extensions(&names, count, out) }
}

unsafe extern "system" fn enumerate_device_extensions(
    gpu: PhysicalDeviceHandle,
    _layer: *const c_char,
    count: *mut u32,
    out: *mut ExtensionProperties,
) -> i32 {
    note("vkEnumerateDeviceExtensionProperties");
    let has = with(|script| script.gpus[super::gpu_index(gpu)].swapchain_extension);
    let names: &[&str] = if has { &["VK_KHR_swapchain"] } else { &[] };
    unsafe { fill_extensions(names, count, out) }
}

unsafe extern "system" fn memory_properties(
    _gpu: PhysicalDeviceHandle,
    out: *mut PhysicalDeviceMemoryProperties,
) {
    note("vkGetPhysicalDeviceMemoryProperties");
    unsafe { out.write(super::resource::memory_properties()) };
}

unsafe extern "system" fn create_surface(
    _instance: InstanceHandle,
    info: *const c_void,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateSurfaceKHR");
    unsafe {
        #[cfg(windows)]
        let window = (*info.cast::<vp::Win32SurfaceCreateInfo>()).hwnd.addr();
        #[cfg(target_os = "linux")]
        let window =
            usize::try_from((*info.cast::<vp::XlibSurfaceCreateInfo>()).window).unwrap_or(0);
        with(|script| script.surface_window = Some(window));
        *out = alloc("surface");
    }
    vk::SUCCESS
}

unsafe extern "system" fn destroy_surface(
    _instance: InstanceHandle,
    surface: u64,
    _allocator: *const c_void,
) {
    note("vkDestroySurfaceKHR");
    release("surface", surface);
}

unsafe extern "system" fn surface_support(
    _gpu: PhysicalDeviceHandle,
    family: u32,
    _surface: u64,
    out: *mut Bool32,
) -> i32 {
    note("vkGetPhysicalDeviceSurfaceSupportKHR");
    with(|script| script.support_queries.push(family));
    unsafe { *out = u32::from(with(|script| script.present_support)) };
    vk::SUCCESS
}

unsafe extern "system" fn surface_capabilities(
    _gpu: PhysicalDeviceHandle,
    _surface: u64,
    out: *mut SurfaceCapabilities,
) -> i32 {
    note("vkGetPhysicalDeviceSurfaceCapabilitiesKHR");
    unsafe { out.write(with(|script| script.surface_caps)) };
    vk::SUCCESS
}

unsafe extern "system" fn surface_formats(
    _gpu: PhysicalDeviceHandle,
    _surface: u64,
    count: *mut u32,
    out: *mut SurfaceFormat,
) -> i32 {
    note("vkGetPhysicalDeviceSurfaceFormatsKHR");
    let formats = with(|script| script.surface_formats.clone());
    unsafe { super::fill(count, out, &formats) }
}

unsafe extern "system" fn create_swapchain(
    _device: DeviceHandle,
    info: *const SwapchainCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateSwapchainKHR");
    let result = with(|script| script.create_swapchain_result);
    if result != vk::SUCCESS {
        return result;
    }
    let info = unsafe { &*info };
    let request = SwapchainRequest {
        format: info.image_format,
        color_space: info.image_color_space,
        width: info.image_extent.width,
        height: info.image_extent.height,
        min_image_count: info.min_image_count,
        usage: info.image_usage,
        present_mode: info.present_mode,
        old_swapchain: info.old_swapchain,
    };
    let handle = alloc("swapchain");
    let images: Vec<u64> = (0..info.min_image_count)
        .map(|n| 0x90_000 + handle * 16 + u64::from(n))
        .collect();
    with(|script| {
        script.swapchain_request = Some(request);
        script.swapchains_created += 1;
        script.swapchain_images.push((handle, images));
    });
    unsafe { *out = handle };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_swapchain(
    _device: DeviceHandle,
    swapchain: u64,
    _allocator: *const c_void,
) {
    note("vkDestroySwapchainKHR");
    release("swapchain", swapchain);
}

unsafe extern "system" fn swapchain_images(
    _device: DeviceHandle,
    swapchain: u64,
    count: *mut u32,
    out: *mut u64,
) -> i32 {
    note("vkGetSwapchainImagesKHR");
    if refused("vkGetSwapchainImagesKHR") {
        return OUT_OF_HOST_MEMORY;
    }
    let images = with(|script| {
        script
            .swapchain_images
            .iter()
            .find(|(handle, _)| *handle == swapchain)
            .map(|(_, images)| images.clone())
    })
    .unwrap_or_default();
    unsafe { super::fill(count, out, &images) }
}

unsafe extern "system" fn acquire_next_image(
    _device: DeviceHandle,
    swapchain: u64,
    _timeout: u64,
    _semaphore: u64,
    _fence: u64,
    out: *mut u32,
) -> i32 {
    note("vkAcquireNextImageKHR");
    let result = with(|script| script.acquire_results.pop_front()).unwrap_or(vk::SUCCESS);
    if result != vk::SUCCESS && result != vp::SUBOPTIMAL {
        return result;
    }
    let image = with(|script| {
        let total = script
            .swapchain_images
            .iter()
            .find(|(handle, _)| *handle == swapchain)
            .map_or(1, |(_, images)| u32::try_from(images.len()).unwrap_or(1));
        let image = script.next_image % total;
        script.next_image += 1;
        image
    });
    unsafe { *out = image };
    result
}

unsafe extern "system" fn queue_present(_queue: QueueHandle, info: *const PresentInfo) -> i32 {
    note("vkQueuePresentKHR");
    let info = unsafe { &*info };
    let record = unsafe {
        PresentRecord {
            image: *info.image_indices,
            wait_semaphore: *info.wait_semaphores,
        }
    };
    with(|script| script.presents.push(record));
    with(|script| script.present_results.pop_front()).unwrap_or(vk::SUCCESS)
}

unsafe extern "system" fn create_semaphore(
    _device: DeviceHandle,
    _info: *const SyncCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateSemaphore");
    if refused("vkCreateSemaphore") {
        return OUT_OF_HOST_MEMORY;
    }
    unsafe { *out = alloc("semaphore") };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_semaphore(
    _device: DeviceHandle,
    semaphore: u64,
    _allocator: *const c_void,
) {
    note("vkDestroySemaphore");
    release("semaphore", semaphore);
}

unsafe extern "system" fn create_fence(
    _device: DeviceHandle,
    info: *const SyncCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateFence");
    if refused("vkCreateFence") {
        return OUT_OF_HOST_MEMORY;
    }
    assert_eq!(
        unsafe { (*info).flags },
        vp::FENCE_CREATE_SIGNALED,
        "a frame fence starts signalled"
    );
    unsafe { *out = alloc("fence") };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_fence(
    _device: DeviceHandle,
    fence: u64,
    _allocator: *const c_void,
) {
    note("vkDestroyFence");
    release("fence", fence);
}

unsafe extern "system" fn wait_for_fences(
    _device: DeviceHandle,
    _count: u32,
    _fences: *const u64,
    _wait_all: Bool32,
    _timeout: u64,
) -> i32 {
    note("vkWaitForFences");
    if refused("vkWaitForFences") {
        return -1;
    }
    vk::SUCCESS
}

unsafe extern "system" fn reset_fences(
    _device: DeviceHandle,
    _count: u32,
    _fences: *const u64,
) -> i32 {
    note("vkResetFences");
    vk::SUCCESS
}

unsafe extern "system" fn create_command_pool(
    _device: DeviceHandle,
    _info: *const CommandPoolCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateCommandPool");
    if refused("vkCreateCommandPool") {
        return OUT_OF_HOST_MEMORY;
    }
    unsafe { *out = alloc("pool") };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_command_pool(
    _device: DeviceHandle,
    pool: u64,
    _allocator: *const c_void,
) {
    note("vkDestroyCommandPool");
    release("pool", pool);
}

unsafe extern "system" fn reset_command_pool(
    _device: DeviceHandle,
    _pool: u64,
    _flags: u32,
) -> i32 {
    note("vkResetCommandPool");
    vk::SUCCESS
}

unsafe extern "system" fn allocate_command_buffers(
    _device: DeviceHandle,
    info: *const CommandBufferAllocateInfo,
    out: *mut CommandBufferHandle,
) -> i32 {
    note("vkAllocateCommandBuffers");
    if refused("vkAllocateCommandBuffers") {
        return OUT_OF_HOST_MEMORY;
    }
    let pool = unsafe { (*info).command_pool };
    let address = usize::try_from(pool).unwrap_or(1) + 0x1000;
    unsafe { *out = CommandBufferHandle(core::ptr::without_provenance_mut(address)) };
    vk::SUCCESS
}

unsafe extern "system" fn begin_command_buffer(
    _cmd: CommandBufferHandle,
    info: *const CommandBufferBeginInfo,
) -> i32 {
    note("vkBeginCommandBuffer");
    if refused("vkBeginCommandBuffer") {
        return OUT_OF_HOST_MEMORY;
    }
    assert_eq!(
        unsafe { (*info).s_type },
        vp::STYPE_COMMAND_BUFFER_BEGIN_INFO
    );
    vk::SUCCESS
}

unsafe extern "system" fn end_command_buffer(_cmd: CommandBufferHandle) -> i32 {
    note("vkEndCommandBuffer");
    vk::SUCCESS
}

unsafe extern "system" fn create_image_view(
    _device: DeviceHandle,
    _info: *const vp::ImageViewCreateInfo,
    _allocator: *const c_void,
    out: *mut u64,
) -> i32 {
    note("vkCreateImageView");
    if refused("vkCreateImageView") {
        return OUT_OF_HOST_MEMORY;
    }
    unsafe { *out = alloc("view") };
    vk::SUCCESS
}

unsafe extern "system" fn destroy_image_view(
    _device: DeviceHandle,
    view: u64,
    _allocator: *const c_void,
) {
    note("vkDestroyImageView");
    release("view", view);
}

unsafe extern "system" fn cmd_barrier(_cmd: CommandBufferHandle, info: *const DependencyInfo) {
    note("vkCmdPipelineBarrier2");
    unsafe { super::resource::record_barrier(info) };
}

unsafe extern "system" fn cmd_begin_rendering(
    _cmd: CommandBufferHandle,
    info: *const RenderingInfo,
) {
    note("vkCmdBeginRendering");
    unsafe {
        let attachment = &*(*info).color_attachments;
        assert_eq!(attachment.load_op, vp::LOAD_OP_CLEAR);
        with(|script| script.clears.push(attachment.clear_value.float32));
    }
}

unsafe extern "system" fn cmd_end_rendering(_cmd: CommandBufferHandle) {
    note("vkCmdEndRendering");
}

unsafe extern "system" fn queue_submit2(
    _queue: QueueHandle,
    count: u32,
    info: *const SubmitInfo2,
    fence: u64,
) -> i32 {
    note("vkQueueSubmit2");
    assert_eq!(count, 1);
    let record = unsafe {
        let info = &*info;
        let (wait_semaphore, wait_stage) = if info.wait_semaphore_info_count > 0 {
            let wait = &*info.wait_semaphore_infos;
            (wait.semaphore, wait.stage_mask)
        } else {
            (0, 0)
        };
        let signal_semaphore = if info.signal_semaphore_info_count > 0 {
            (*info.signal_semaphore_infos).semaphore
        } else {
            0
        };
        SubmitRecord {
            wait_semaphore,
            wait_stage,
            signal_semaphore,
            fence,
        }
    };
    with(|script| script.submits.push(record));
    with(|script| script.submit_result)
}
