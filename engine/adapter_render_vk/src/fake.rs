//! A fake Vulkan driver for tests: `vkGetInstanceProcAddr` and the few functions the adapter calls,
//! driven by a script, logging every call. It lets the whole start-up path (loader, instance,
//! device choice, device, teardown order) run without a GPU or a loader.
//!
//! State is per thread, so tests that run in parallel do not see each other. The stubs follow the
//! Vulkan conventions the adapter relies on: two-call enumerations, out parameters, `VkResult`.
#![expect(
    unsafe_code,
    reason = "test-only fake of a C interface: the stubs read and write through the pointers the adapter passes"
)]
#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "test-only fake: each stub trusts the pointers of the adapter under test, which the tests then check"
)]

use crate::vk::{
    self, DebugMessengerCreateInfo, DeviceCreateInfo, DeviceHandle, InstanceCreateInfo,
    InstanceHandle, LayerProperties, PhysicalDeviceFeatures2, PhysicalDeviceHandle,
    PhysicalDeviceProperties, QueueFamilyProperties, QueueHandle, VkResult, Vulkan13Features,
};
use core::ffi::{CStr, c_char, c_void};
use core::ptr::without_provenance_mut;
use std::cell::RefCell;

/// A fake physical device.
#[derive(Clone, Debug)]
pub struct FakeGpu {
    pub name: &'static str,
    pub api: u32,
    pub device_type: u32,
    /// The `queueFlags` of each queue family.
    pub families: Vec<u32>,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
    pub sampler_anisotropy: bool,
}

impl FakeGpu {
    /// A device that meets the baseline: Vulkan 1.3, one graphics family, all features.
    pub fn good(name: &'static str, device_type: u32) -> Self {
        Self {
            name,
            api: crate::ApiVersion::new(1, 3, 280).raw(),
            device_type,
            families: vec![vk::QUEUE_GRAPHICS],
            dynamic_rendering: true,
            synchronization2: true,
            sampler_anisotropy: true,
        }
    }
}

/// What `vkCreateDevice` was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRequest {
    pub gpu: usize,
    pub queue_family: u32,
    pub queue_count: u32,
    pub anisotropy: bool,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
}

/// The script of the fake driver and the log of what the adapter called.
pub struct Script {
    pub instance_version: u32,
    pub validation_layer: bool,
    pub gpus: Vec<FakeGpu>,
    /// Function names the fake does not export (the lookup returns null).
    pub missing: Vec<&'static str>,
    pub create_instance_result: VkResult,
    pub create_device_result: VkResult,
    pub wait_idle_result: VkResult,
    pub null_queue: bool,
    pub log: Vec<&'static str>,
    pub layers_requested: u32,
    pub extensions_requested: u32,
    pub device_request: Option<DeviceRequest>,
    pub queue_request: Option<(u32, u32)>,
}

impl Script {
    /// A system with Vulkan 1.4 instance support and the given devices.
    pub fn with_gpus(gpus: Vec<FakeGpu>) -> Self {
        Self {
            instance_version: crate::ApiVersion::new(1, 4, 300).raw(),
            validation_layer: false,
            gpus,
            missing: Vec::new(),
            create_instance_result: vk::SUCCESS,
            create_device_result: vk::SUCCESS,
            wait_idle_result: vk::SUCCESS,
            null_queue: false,
            log: Vec::new(),
            layers_requested: 0,
            extensions_requested: 0,
            device_request: None,
            queue_request: None,
        }
    }
}

thread_local! {
    static SCRIPT: RefCell<Script> = RefCell::new(Script::with_gpus(Vec::new()));
}

/// Installs `script` for this thread.
pub fn install(script: Script) {
    SCRIPT.with(|cell| *cell.borrow_mut() = script);
}

/// Reads or changes the script of this thread.
pub fn with<R>(f: impl FnOnce(&mut Script) -> R) -> R {
    SCRIPT.with(|cell| f(&mut cell.borrow_mut()))
}

/// The calls so far, in order.
pub fn log() -> Vec<&'static str> {
    with(|script| script.log.clone())
}

/// Forgets the calls so far (to look at what a later step does).
pub fn clear_log() {
    with(|script| script.log.clear());
}

fn note(call: &'static str) {
    with(|script| script.log.push(call));
}

fn fake_handle(value: usize) -> *mut c_void {
    without_provenance_mut::<c_void>(value)
}

const INSTANCE: usize = 0x10;
const DEVICE: usize = 0x20;
const QUEUE: usize = 0x30;
const FIRST_GPU: usize = 0x1000;

fn gpu_index(handle: PhysicalDeviceHandle) -> usize {
    handle.0.addr() - FIRST_GPU
}

/// The two-call enumeration pattern: with no output array only the count is written.
unsafe fn fill<T: Copy>(count: *mut u32, out: *mut T, items: &[T]) -> VkResult {
    unsafe {
        let total = u32::try_from(items.len()).unwrap_or(u32::MAX);
        if out.is_null() {
            *count = total;
            return vk::SUCCESS;
        }
        let room = usize::try_from(*count).unwrap_or(0);
        let given = room.min(items.len());
        for (i, item) in items.iter().take(given).enumerate() {
            *out.add(i) = *item;
        }
        *count = u32::try_from(given).unwrap_or(0);
        if given < items.len() {
            vk::INCOMPLETE
        } else {
            vk::SUCCESS
        }
    }
}

macro_rules! address {
    ($f:expr) => {
        ($f as *const ()).cast_mut().cast::<c_void>()
    };
}

/// The fake `vkGetInstanceProcAddr`.
pub unsafe extern "system" fn get_instance_proc_addr(
    _instance: InstanceHandle,
    name: *const c_char,
) -> *mut c_void {
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
    if with(|script| script.missing.contains(&name)) {
        return core::ptr::null_mut();
    }
    match name {
        "vkCreateInstance" => address!(create_instance as unsafe extern "system" fn(_, _, _) -> _),
        "vkEnumerateInstanceVersion" => {
            address!(enumerate_instance_version as unsafe extern "system" fn(_) -> _)
        }
        "vkEnumerateInstanceLayerProperties" => {
            address!(enumerate_layers as unsafe extern "system" fn(_, _) -> _)
        }
        "vkDestroyInstance" => address!(destroy_instance as unsafe extern "system" fn(_, _)),
        "vkEnumeratePhysicalDevices" => {
            address!(enumerate_physical_devices as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkGetPhysicalDeviceProperties" => {
            address!(get_properties as unsafe extern "system" fn(_, _))
        }
        "vkGetPhysicalDeviceQueueFamilyProperties" => {
            address!(get_queue_families as unsafe extern "system" fn(_, _, _))
        }
        "vkGetPhysicalDeviceFeatures2" => {
            address!(get_features2 as unsafe extern "system" fn(_, _))
        }
        "vkCreateDevice" => address!(create_device as unsafe extern "system" fn(_, _, _, _) -> _),
        "vkGetDeviceProcAddr" => {
            address!(get_device_proc_addr as unsafe extern "system" fn(_, _) -> _)
        }
        "vkDestroyDevice" => address!(destroy_device as unsafe extern "system" fn(_, _)),
        "vkGetDeviceQueue" => address!(get_device_queue as unsafe extern "system" fn(_, _, _, _)),
        "vkDeviceWaitIdle" => address!(device_wait_idle as unsafe extern "system" fn(_) -> _),
        "vkCreateDebugUtilsMessengerEXT" => {
            address!(create_messenger as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyDebugUtilsMessengerEXT" => {
            address!(destroy_messenger as unsafe extern "system" fn(_, _, _))
        }
        _ => core::ptr::null_mut(),
    }
}

unsafe extern "system" fn get_device_proc_addr(
    _device: DeviceHandle,
    name: *const c_char,
) -> *mut c_void {
    unsafe { get_instance_proc_addr(InstanceHandle::NULL, name) }
}

unsafe extern "system" fn create_instance(
    info: *const InstanceCreateInfo,
    _allocator: *const c_void,
    out: *mut InstanceHandle,
) -> VkResult {
    note("vkCreateInstance");
    let result = with(|script| script.create_instance_result);
    if result != vk::SUCCESS {
        return result;
    }
    unsafe {
        let layers = (*info).enabled_layer_count;
        let extensions = (*info).enabled_extension_count;
        with(|script| {
            script.layers_requested = layers;
            script.extensions_requested = extensions;
        });
        *out = InstanceHandle(fake_handle(INSTANCE));
    }
    vk::SUCCESS
}

unsafe extern "system" fn enumerate_instance_version(out: *mut u32) -> VkResult {
    note("vkEnumerateInstanceVersion");
    unsafe { *out = with(|script| script.instance_version) };
    vk::SUCCESS
}

unsafe extern "system" fn enumerate_layers(count: *mut u32, out: *mut LayerProperties) -> VkResult {
    note("vkEnumerateInstanceLayerProperties");
    let mut layers = Vec::new();
    if with(|script| script.validation_layer) {
        let mut layer = LayerProperties::zeroed();
        for (slot, byte) in layer
            .layer_name
            .iter_mut()
            .zip(b"VK_LAYER_KHRONOS_validation")
        {
            *slot = c_char::try_from(*byte).unwrap_or(0);
        }
        layers.push(layer);
    }
    unsafe {
        let total = u32::try_from(layers.len()).unwrap_or(0);
        if out.is_null() {
            *count = total;
            return vk::SUCCESS;
        }
        let room = usize::try_from(*count).unwrap_or(0);
        let given = room.min(layers.len());
        for (i, layer) in layers.into_iter().take(given).enumerate() {
            out.add(i).write(layer);
        }
        *count = u32::try_from(given).unwrap_or(0);
    }
    vk::SUCCESS
}

unsafe extern "system" fn destroy_instance(_instance: InstanceHandle, _allocator: *const c_void) {
    note("vkDestroyInstance");
}

unsafe extern "system" fn enumerate_physical_devices(
    _instance: InstanceHandle,
    count: *mut u32,
    out: *mut PhysicalDeviceHandle,
) -> VkResult {
    note("vkEnumeratePhysicalDevices");
    let handles: Vec<PhysicalDeviceHandle> = (0..with(|script| script.gpus.len()))
        .map(|i| PhysicalDeviceHandle(fake_handle(FIRST_GPU + i)))
        .collect();
    unsafe { fill(count, out, &handles) }
}

unsafe extern "system" fn get_properties(
    gpu: PhysicalDeviceHandle,
    out: *mut PhysicalDeviceProperties,
) {
    note("vkGetPhysicalDeviceProperties");
    let gpu = with(|script| script.gpus[gpu_index(gpu)].clone());
    let mut props = PhysicalDeviceProperties::zeroed();
    props.api_version = gpu.api;
    props.driver_version = 7;
    props.vendor_id = 0x10DE;
    props.device_id = 0x2500;
    props.device_type = gpu.device_type;
    for (slot, byte) in props.device_name.iter_mut().zip(gpu.name.bytes()) {
        *slot = c_char::try_from(byte).unwrap_or(0);
    }
    unsafe { out.write(props) };
}

unsafe extern "system" fn get_queue_families(
    gpu: PhysicalDeviceHandle,
    count: *mut u32,
    out: *mut QueueFamilyProperties,
) {
    note("vkGetPhysicalDeviceQueueFamilyProperties");
    let families: Vec<QueueFamilyProperties> =
        with(|script| script.gpus[gpu_index(gpu)].families.clone())
            .into_iter()
            .map(|queue_flags| QueueFamilyProperties {
                queue_flags,
                queue_count: 1,
                timestamp_valid_bits: 64,
                min_image_transfer_granularity: [1, 1, 1],
            })
            .collect();
    let _ = unsafe { fill(count, out, &families) };
}

unsafe extern "system" fn get_features2(
    gpu: PhysicalDeviceHandle,
    out: *mut PhysicalDeviceFeatures2,
) {
    note("vkGetPhysicalDeviceFeatures2");
    let gpu = with(|script| script.gpus[gpu_index(gpu)].clone());
    unsafe {
        (*out).features.flags[vk::FEATURE_SAMPLER_ANISOTROPY] = u32::from(gpu.sampler_anisotropy);
        let mut next = (*out).p_next.cast::<Vulkan13Features>();
        while !next.is_null() {
            if (*next).s_type == vk::STYPE_VULKAN_1_3_FEATURES {
                (*next).flags[vk::FEATURE_13_DYNAMIC_RENDERING] = u32::from(gpu.dynamic_rendering);
                (*next).flags[vk::FEATURE_13_SYNCHRONIZATION2] = u32::from(gpu.synchronization2);
            }
            next = (*next).p_next.cast::<Vulkan13Features>();
        }
    }
}

unsafe extern "system" fn create_device(
    gpu: PhysicalDeviceHandle,
    info: *const DeviceCreateInfo,
    _allocator: *const c_void,
    out: *mut DeviceHandle,
) -> VkResult {
    note("vkCreateDevice");
    let result = with(|script| script.create_device_result);
    if result != vk::SUCCESS {
        return result;
    }
    unsafe {
        let queue = &*(*info).queue_create_infos;
        let features = (*info).p_next.cast::<PhysicalDeviceFeatures2>();
        let v13 = (*features).p_next.cast::<Vulkan13Features>();
        let request = DeviceRequest {
            gpu: gpu_index(gpu),
            queue_family: queue.queue_family_index,
            queue_count: queue.queue_count,
            anisotropy: (*features).features.flags[vk::FEATURE_SAMPLER_ANISOTROPY] == 1,
            dynamic_rendering: (*v13).flags[vk::FEATURE_13_DYNAMIC_RENDERING] == 1,
            synchronization2: (*v13).flags[vk::FEATURE_13_SYNCHRONIZATION2] == 1,
        };
        with(|script| script.device_request = Some(request));
        *out = DeviceHandle(fake_handle(DEVICE));
    }
    vk::SUCCESS
}

unsafe extern "system" fn destroy_device(_device: DeviceHandle, _allocator: *const c_void) {
    note("vkDestroyDevice");
}

unsafe extern "system" fn get_device_queue(
    _device: DeviceHandle,
    family: u32,
    index: u32,
    out: *mut QueueHandle,
) {
    note("vkGetDeviceQueue");
    with(|script| script.queue_request = Some((family, index)));
    let handle = if with(|script| script.null_queue) {
        0
    } else {
        QUEUE
    };
    unsafe { *out = QueueHandle(fake_handle(handle)) };
}

unsafe extern "system" fn device_wait_idle(_device: DeviceHandle) -> VkResult {
    note("vkDeviceWaitIdle");
    with(|script| script.wait_idle_result)
}

unsafe extern "system" fn create_messenger(
    _instance: InstanceHandle,
    info: *const DebugMessengerCreateInfo,
    _allocator: *const c_void,
    out: *mut vk::MessengerHandle,
) -> VkResult {
    note("vkCreateDebugUtilsMessengerEXT");
    unsafe {
        assert!(
            (*info).user_callback.is_some(),
            "the adapter must pass a callback"
        );
        *out = 7;
    }
    vk::SUCCESS
}

unsafe extern "system" fn destroy_messenger(
    _instance: InstanceHandle,
    _messenger: vk::MessengerHandle,
    _allocator: *const c_void,
) {
    note("vkDestroyDebugUtilsMessengerEXT");
}
