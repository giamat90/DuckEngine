//! Hand-written declarations of the Vulkan types and constants the adapter uses (choice O1 of the
//! phase 6 design note: no binding crate). Layouts follow `vulkan_core.h` (Vulkan-Headers
//! 1.3.296) for 64-bit targets; the tests check the sizes that matter.

use core::ffi::{c_char, c_void};

/// `VkBool32`.
pub type Bool32 = u32;
/// `VkResult`.
pub type VkResult = i32;
/// `VkDebugUtilsMessengerEXT`, a non-dispatchable handle (a 64-bit integer on 64-bit targets).
pub type MessengerHandle = u64;

/// `VK_SUCCESS`.
pub const SUCCESS: VkResult = 0;
/// `VK_INCOMPLETE`: an enumeration had more entries than the caller's array held.
pub const INCOMPLETE: VkResult = 5;
/// `VK_ERROR_INITIALIZATION_FAILED`.
pub const ERROR_INITIALIZATION_FAILED: VkResult = -3;

/// `VK_STRUCTURE_TYPE_APPLICATION_INFO`.
pub const STYPE_APPLICATION_INFO: u32 = 0;
/// `VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO`.
pub const STYPE_INSTANCE_CREATE_INFO: u32 = 1;
/// `VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO`.
pub const STYPE_DEVICE_QUEUE_CREATE_INFO: u32 = 2;
/// `VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO`.
pub const STYPE_DEVICE_CREATE_INFO: u32 = 3;
/// `VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2`.
pub const STYPE_FEATURES_2: u32 = 1_000_059_000;
/// `VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES`.
pub const STYPE_VULKAN_1_3_FEATURES: u32 = 53;
/// `VK_STRUCTURE_TYPE_DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT`.
pub const STYPE_DEBUG_MESSENGER_CREATE_INFO: u32 = 1_000_128_004;

/// `VK_QUEUE_GRAPHICS_BIT`.
pub const QUEUE_GRAPHICS: u32 = 1;

/// `VK_DEBUG_UTILS_MESSAGE_SEVERITY_WARNING_BIT_EXT`.
pub const SEVERITY_WARNING: u32 = 0x100;
/// `VK_DEBUG_UTILS_MESSAGE_SEVERITY_ERROR_BIT_EXT`.
pub const SEVERITY_ERROR: u32 = 0x1000;
/// `VK_DEBUG_UTILS_MESSAGE_TYPE_GENERAL_BIT_EXT | VALIDATION | PERFORMANCE`.
pub const MESSAGE_TYPES_ALL: u32 = 1 | 2 | 4;

/// The number of `VkBool32` members of `VkPhysicalDeviceFeatures`.
pub const CORE_FEATURE_COUNT: usize = 55;
/// The position of `samplerAnisotropy` in `VkPhysicalDeviceFeatures`.
pub const FEATURE_SAMPLER_ANISOTROPY: usize = 19;
/// The number of `VkBool32` members of `VkPhysicalDeviceVulkan13Features`.
pub const FEATURES_13_COUNT: usize = 15;
/// The position of `synchronization2` in `VkPhysicalDeviceVulkan13Features`.
pub const FEATURE_13_SYNCHRONIZATION2: usize = 9;
/// The position of `dynamicRendering` in `VkPhysicalDeviceVulkan13Features`.
pub const FEATURE_13_DYNAMIC_RENDERING: usize = 12;

macro_rules! dispatchable {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub *mut c_void);

        impl $name {
            /// `VK_NULL_HANDLE`.
            pub const NULL: Self = Self(core::ptr::null_mut());
        }
    )+};
}

dispatchable! {
    /// `VkInstance`.
    InstanceHandle,
    /// `VkPhysicalDevice`.
    PhysicalDeviceHandle,
    /// `VkDevice`.
    DeviceHandle,
    /// `VkQueue`.
    QueueHandle,
}

/// `PFN_vkGetInstanceProcAddr`, as the loader exports it.
pub type GetInstanceProcAddr =
    unsafe extern "system" fn(InstanceHandle, *const c_char) -> *mut c_void;

/// `VkApplicationInfo`.
#[repr(C)]
pub struct ApplicationInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub application_name: *const c_char,
    pub application_version: u32,
    pub engine_name: *const c_char,
    pub engine_version: u32,
    pub api_version: u32,
}

/// `VkInstanceCreateInfo`.
#[repr(C)]
pub struct InstanceCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub application_info: *const ApplicationInfo,
    pub enabled_layer_count: u32,
    pub enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub enabled_extension_names: *const *const c_char,
}

/// `VkLayerProperties`.
#[repr(C)]
pub struct LayerProperties {
    pub layer_name: [c_char; 256],
    pub spec_version: u32,
    pub implementation_version: u32,
    pub description: [c_char; 256],
}

impl LayerProperties {
    /// All zero, to be filled by the driver.
    pub const fn zeroed() -> Self {
        Self {
            layer_name: [0; 256],
            spec_version: 0,
            implementation_version: 0,
            description: [0; 256],
        }
    }
}

/// `VkPhysicalDeviceProperties`. The `limits` member (`VkPhysicalDeviceLimits`, 504 bytes) is kept
/// as raw words: part 1 reads none of it.
#[repr(C)]
pub struct PhysicalDeviceProperties {
    pub api_version: u32,
    pub driver_version: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: u32,
    pub device_name: [c_char; 256],
    pub pipeline_cache_uuid: [u8; 16],
    pub limits: [u64; 63],
    pub sparse_properties: [u32; 5],
}

impl PhysicalDeviceProperties {
    /// All zero, to be filled by the driver.
    pub const fn zeroed() -> Self {
        Self {
            api_version: 0,
            driver_version: 0,
            vendor_id: 0,
            device_id: 0,
            device_type: 0,
            device_name: [0; 256],
            pipeline_cache_uuid: [0; 16],
            limits: [0; 63],
            sparse_properties: [0; 5],
        }
    }
}

/// `VkQueueFamilyProperties`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueueFamilyProperties {
    pub queue_flags: u32,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: [u32; 3],
}

/// `VkPhysicalDeviceFeatures`: 55 `VkBool32` members, addressed by position.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceFeatures {
    pub flags: [Bool32; CORE_FEATURE_COUNT],
}

/// `VkPhysicalDeviceFeatures2`.
#[repr(C)]
pub struct PhysicalDeviceFeatures2 {
    pub s_type: u32,
    pub p_next: *mut c_void,
    pub features: PhysicalDeviceFeatures,
}

/// `VkPhysicalDeviceVulkan13Features`: 15 `VkBool32` members, addressed by position.
#[repr(C)]
pub struct Vulkan13Features {
    pub s_type: u32,
    pub p_next: *mut c_void,
    pub flags: [Bool32; FEATURES_13_COUNT],
}

/// `VkDeviceQueueCreateInfo`.
#[repr(C)]
pub struct DeviceQueueCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_family_index: u32,
    pub queue_count: u32,
    pub queue_priorities: *const f32,
}

/// `VkDeviceCreateInfo`.
#[repr(C)]
pub struct DeviceCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_create_info_count: u32,
    pub queue_create_infos: *const DeviceQueueCreateInfo,
    pub enabled_layer_count: u32,
    pub enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub enabled_extension_names: *const *const c_char,
    pub enabled_features: *const PhysicalDeviceFeatures,
}

/// `PFN_vkDebugUtilsMessengerCallbackEXT`.
pub type DebugCallback =
    unsafe extern "system" fn(u32, u32, *const DebugCallbackData, *mut c_void) -> Bool32;

/// `VkDebugUtilsMessengerCreateInfoEXT`.
#[repr(C)]
pub struct DebugMessengerCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub message_severity: u32,
    pub message_type: u32,
    pub user_callback: Option<DebugCallback>,
    pub user_data: *mut c_void,
}

/// `VkDebugUtilsMessengerCallbackDataEXT`.
#[repr(C)]
pub struct DebugCallbackData {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub message_id_name: *const c_char,
    pub message_id_number: i32,
    pub message: *const c_char,
    pub queue_label_count: u32,
    pub queue_labels: *const c_void,
    pub cmd_buf_label_count: u32,
    pub cmd_buf_labels: *const c_void,
    pub object_count: u32,
    pub objects: *const c_void,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    // The adapter targets 64-bit Windows and Linux; these are the sizes `vulkan_core.h` gives.
    #[test]
    fn the_layouts_match_the_headers_on_64_bit() {
        assert_eq!(size_of::<ApplicationInfo>(), 48);
        assert_eq!(size_of::<InstanceCreateInfo>(), 64);
        assert_eq!(size_of::<LayerProperties>(), 520);
        assert_eq!(size_of::<QueueFamilyProperties>(), 24);
        assert_eq!(size_of::<PhysicalDeviceFeatures>(), 220);
        // 16 bytes of header + 220 of features = 236, padded to the 8-byte alignment of the pointer.
        assert_eq!(size_of::<PhysicalDeviceFeatures2>(), 240);
        // 16 + 60 = 76, padded to 80.
        assert_eq!(size_of::<Vulkan13Features>(), 80);
        assert_eq!(size_of::<DeviceQueueCreateInfo>(), 40);
        assert_eq!(size_of::<DeviceCreateInfo>(), 72);
        assert_eq!(size_of::<DebugMessengerCreateInfo>(), 48);
        assert_eq!(size_of::<DebugCallbackData>(), 96);
    }

    #[test]
    fn the_physical_device_properties_are_824_bytes_with_the_limits_at_296() {
        assert_eq!(size_of::<PhysicalDeviceProperties>(), 824);
        assert_eq!(offset_of!(PhysicalDeviceProperties, device_name), 20);
        assert_eq!(offset_of!(PhysicalDeviceProperties, limits), 296);
        assert_eq!(offset_of!(PhysicalDeviceProperties, sparse_properties), 800);
    }

    #[test]
    fn the_feature_positions_stay_inside_their_structs() {
        const {
            assert!(FEATURE_SAMPLER_ANISOTROPY < CORE_FEATURE_COUNT);
            assert!(FEATURE_13_SYNCHRONIZATION2 < FEATURES_13_COUNT);
            assert!(FEATURE_13_DYNAMIC_RENDERING < FEATURES_13_COUNT);
            assert!(FEATURE_13_SYNCHRONIZATION2 != FEATURE_13_DYNAMIC_RENDERING);
        }
    }

    #[test]
    fn a_dispatchable_handle_is_one_pointer() {
        assert_eq!(size_of::<InstanceHandle>(), size_of::<*mut c_void>());
        assert!(InstanceHandle::NULL.0.is_null());
    }
}
