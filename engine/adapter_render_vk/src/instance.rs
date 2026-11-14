//! The Vulkan instance: made at 1.3, optionally with the validation layer and a debug messenger,
//! and the physical devices it lists.
#![expect(
    unsafe_code,
    reason = "the instance is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::debug::{ValidationLog, ValidationReport, callback};
use crate::entry::{Entry, destroy_found, missing, name_is};
use crate::error::{VulkanError, check_created};
use crate::select::{Candidate, DeviceType};
use crate::surface::{SURFACE_EXTENSIONS, SurfaceFns};
use crate::version::{ApiVersion, BASELINE};
use crate::vk::{
    ApplicationInfo, CORE_FEATURE_COUNT, DebugMessengerCreateInfo, DeviceCreateInfo, DeviceHandle,
    FEATURE_13_DYNAMIC_RENDERING, FEATURE_13_SYNCHRONIZATION2, FEATURE_SAMPLER_ANISOTROPY,
    FEATURES_13_COUNT, INCOMPLETE, InstanceCreateInfo, InstanceHandle, MESSAGE_TYPES_ALL,
    MessengerHandle, PhysicalDeviceFeatures, PhysicalDeviceFeatures2, PhysicalDeviceHandle,
    PhysicalDeviceProperties, QUEUE_GRAPHICS, QueueFamilyProperties, SEVERITY_ERROR,
    SEVERITY_WARNING, STYPE_APPLICATION_INFO, STYPE_DEBUG_MESSENGER_CREATE_INFO, STYPE_FEATURES_2,
    STYPE_INSTANCE_CREATE_INFO, STYPE_VULKAN_1_3_FEATURES, SUCCESS, VkResult, Vulkan13Features,
};
use crate::vk_present::{EXT_SWAPCHAIN, ExtensionProperties, PhysicalDeviceMemoryProperties};
use crate::vk_resource::FormatProperties;
use core::ffi::{CStr, c_char, c_void};
use std::sync::Arc;

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";
const DEBUG_UTILS: &CStr = c"VK_EXT_debug_utils";
const ENGINE_NAME: &CStr = c"DuckEngine";

/// `maxImageDimension2D`: the second 32-bit word of `VkPhysicalDeviceLimits`, which the
/// properties keep as raw 64-bit words; the bytes of a word are the bytes in memory, so bytes 4
/// to 8 are that field on any byte order.
fn max_image_dimension_2d(properties: &PhysicalDeviceProperties) -> u32 {
    let word = properties
        .limits
        .first()
        .copied()
        .unwrap_or(0)
        .to_ne_bytes();
    u32::from_ne_bytes([word[4], word[5], word[6], word[7]])
}

/// More devices or queue families than any system has; a larger count is cut here.
const MAX_ENUMERATED: u32 = 1024;
/// More extensions than any device lists; a larger count is cut here.
const MAX_EXTENSIONS: u32 = 4096;

function_table! {
    /// The instance-level functions the adapter calls.
    struct InstanceFns {
        destroy_instance: unsafe extern "system" fn(InstanceHandle, *const c_void) = c"vkDestroyInstance",
        enumerate_physical_devices: unsafe extern "system" fn(InstanceHandle, *mut u32, *mut PhysicalDeviceHandle) -> VkResult = c"vkEnumeratePhysicalDevices",
        get_properties: unsafe extern "system" fn(PhysicalDeviceHandle, *mut PhysicalDeviceProperties) = c"vkGetPhysicalDeviceProperties",
        get_queue_family_properties: unsafe extern "system" fn(PhysicalDeviceHandle, *mut u32, *mut QueueFamilyProperties) = c"vkGetPhysicalDeviceQueueFamilyProperties",
        get_features2: unsafe extern "system" fn(PhysicalDeviceHandle, *mut PhysicalDeviceFeatures2) = c"vkGetPhysicalDeviceFeatures2",
        create_device: unsafe extern "system" fn(PhysicalDeviceHandle, *const DeviceCreateInfo, *const c_void, *mut DeviceHandle) -> VkResult = c"vkCreateDevice",
        get_device_proc_addr: unsafe extern "system" fn(DeviceHandle, *const c_char) -> *mut c_void = c"vkGetDeviceProcAddr",
        enumerate_device_extensions: unsafe extern "system" fn(PhysicalDeviceHandle, *const c_char, *mut u32, *mut ExtensionProperties) -> VkResult = c"vkEnumerateDeviceExtensionProperties",
        get_memory_properties: unsafe extern "system" fn(PhysicalDeviceHandle, *mut PhysicalDeviceMemoryProperties) = c"vkGetPhysicalDeviceMemoryProperties",
        get_format_properties: unsafe extern "system" fn(PhysicalDeviceHandle, u32, *mut FormatProperties) = c"vkGetPhysicalDeviceFormatProperties",
    }
}

function_table! {
    /// The debug messenger functions (`VK_EXT_debug_utils`).
    struct DebugFns {
        create_messenger: unsafe extern "system" fn(InstanceHandle, *const DebugMessengerCreateInfo, *const c_void, *mut MessengerHandle) -> VkResult = c"vkCreateDebugUtilsMessengerEXT",
        destroy_messenger: unsafe extern "system" fn(InstanceHandle, MessengerHandle, *const c_void) = c"vkDestroyDebugUtilsMessengerEXT",
    }
}

/// A physical device and what the adapter learned about it.
#[derive(Clone, Debug)]
pub struct PhysicalDevice {
    pub handle: PhysicalDeviceHandle,
    pub name: String,
    pub driver_version: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub candidate: Candidate,
    /// Whether the device offers `VK_KHR_swapchain`.
    pub swapchain_extension: bool,
    /// The largest width or height of a 2D image (`maxImageDimension2D`).
    pub max_image_dimension_2d: u32,
}

/// A Vulkan instance.
///
/// Holds function pointers into the loader, so the [`Entry`] it came from must outlive it.
pub struct Instance {
    handle: InstanceHandle,
    fns: InstanceFns,
    messenger: Option<(DebugFns, MessengerHandle)>,
    /// The surface functions, when the instance was made with the surface extensions.
    surface: Option<SurfaceFns>,
    /// Where the validation callback writes. Shared with the C side by pointer, so it must live
    /// until the messenger is destroyed (see `Drop`).
    log: Arc<ValidationLog>,
}

impl Instance {
    /// Makes a Vulkan 1.3 instance; with `validation`, also turns on the Khronos validation layer
    /// and a debug messenger when the layer is installed (otherwise it runs without them).
    ///
    /// # Errors
    /// [`VulkanError::InstanceTooOld`] below 1.3; [`VulkanError::MissingFunction`] and
    /// [`VulkanError::Call`] when the loader or the driver fails.
    pub fn create(entry: &Entry, validation: bool) -> Result<Self, VulkanError> {
        let version = entry.instance_version();
        if !BASELINE.is_met_by(version) {
            return Err(VulkanError::InstanceTooOld(version));
        }
        // Validation is a development aid: if the layers cannot be listed, run without it.
        let validation = validation && entry.has_layer(VALIDATION_LAYER).unwrap_or(false);
        let application = ApplicationInfo {
            s_type: STYPE_APPLICATION_INFO,
            p_next: core::ptr::null(),
            application_name: ENGINE_NAME.as_ptr(),
            application_version: 0,
            engine_name: ENGINE_NAME.as_ptr(),
            engine_version: 0,
            api_version: BASELINE.raw(),
        };
        let layers = [VALIDATION_LAYER.as_ptr()];
        // Surface extensions are optional: without them the context runs, it just cannot present.
        let surfaces = entry.has_extensions(&SURFACE_EXTENSIONS).unwrap_or(false);
        let mut extensions: Vec<*const c_char> = Vec::new();
        if validation {
            extensions.push(DEBUG_UTILS.as_ptr());
        }
        if surfaces {
            extensions.extend(SURFACE_EXTENSIONS.iter().map(|name| name.as_ptr()));
        }
        let enabled = u32::from(validation);
        let info = InstanceCreateInfo {
            s_type: STYPE_INSTANCE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            application_info: &raw const application,
            enabled_layer_count: enabled,
            enabled_layer_names: layers.as_ptr(),
            enabled_extension_count: u32::try_from(extensions.len()).unwrap_or(0),
            enabled_extension_names: extensions.as_ptr(),
        };
        // SAFETY: `info` and everything it points to (`application`, the name arrays and the
        // static strings) live until the call returns; the counts are at most the array sizes.
        let handle = unsafe { entry.create_instance(&info) }?;
        // SAFETY: `handle` is the live instance just made through `entry`.
        let lookup = |name: &CStr| unsafe { entry.proc_addr(handle, name) };
        // SAFETY: the loader returns functions with the declared signatures for these names.
        let fns = match unsafe { InstanceFns::load(lookup) } {
            Ok(fns) => fns,
            Err(name) => {
                // Without the table, destroying needs the one function looked up on its own.
                // SAFETY: the address is `vkDestroyInstance` or null; the instance is live and
                // destroyed once, here.
                unsafe { destroy_found(lookup(c"vkDestroyInstance"), handle) };
                return Err(missing(name));
            }
        };
        // A surface table that does not load (an advertised extension without its functions)
        // only costs the ability to present.
        let surface = if surfaces {
            // SAFETY: the loader returns functions with the declared signatures for these names.
            unsafe { SurfaceFns::load(lookup) }.ok()
        } else {
            None
        };
        let mut instance = Self {
            handle,
            fns,
            messenger: None,
            surface,
            log: Arc::new(ValidationLog::default()),
        };
        if validation {
            // On error `instance` is dropped here, which destroys it.
            instance.attach_messenger(entry)?;
        }
        Ok(instance)
    }

    /// Creates the debug messenger that feeds the validation log.
    fn attach_messenger(&mut self, entry: &Entry) -> Result<(), VulkanError> {
        // SAFETY: `self.handle` is a live instance made through `entry`.
        let lookup = |name: &CStr| unsafe { entry.proc_addr(self.handle, name) };
        // SAFETY: the loader returns functions with the declared signatures for these names.
        let fns = unsafe { DebugFns::load(lookup) }.map_err(missing)?;
        let info = DebugMessengerCreateInfo {
            s_type: STYPE_DEBUG_MESSENGER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            message_severity: SEVERITY_WARNING | SEVERITY_ERROR,
            message_type: MESSAGE_TYPES_ALL,
            user_callback: Some(callback),
            user_data: Arc::as_ptr(&self.log).cast_mut().cast::<c_void>(),
        };
        let mut messenger: MessengerHandle = 0;
        // SAFETY: `info` is valid for the call; the user data points to the log, which `self`
        // keeps alive until the messenger is destroyed in `drop`; `messenger` is a valid output.
        let result = unsafe {
            (fns.create_messenger)(
                self.handle,
                &raw const info,
                core::ptr::null(),
                &raw mut messenger,
            )
        };
        if result != SUCCESS {
            return Err(VulkanError::Call {
                call: "vkCreateDebugUtilsMessengerEXT",
                result,
            });
        }
        self.messenger = Some((fns, messenger));
        Ok(())
    }

    /// The surface functions, when the instance can make surfaces.
    pub fn surface_fns(&self) -> Option<&SurfaceFns> {
        self.surface.as_ref()
    }

    /// The raw instance handle, for the calls that take it.
    pub fn handle(&self) -> InstanceHandle {
        self.handle
    }

    /// Whether the validation layer is on.
    pub fn validation_active(&self) -> bool {
        self.messenger.is_some()
    }

    /// What the validation layer reported so far.
    pub fn validation_report(&self) -> ValidationReport {
        self.log.report()
    }

    /// The physical devices of the instance, with what the adapter needs to choose.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver fails to list them.
    pub fn physical_devices(&self) -> Result<Vec<PhysicalDevice>, VulkanError> {
        let call = "vkEnumeratePhysicalDevices";
        let mut count = 0_u32;
        // SAFETY: a null array asks for the count only.
        let result = unsafe {
            (self.fns.enumerate_physical_devices)(
                self.handle,
                &raw mut count,
                core::ptr::null_mut(),
            )
        };
        if result != SUCCESS {
            return Err(VulkanError::Call { call, result });
        }
        count = count.min(MAX_ENUMERATED);
        let room = usize::try_from(count).unwrap_or(0);
        let mut handles = vec![PhysicalDeviceHandle::NULL; room];
        // SAFETY: `handles` holds `count` entries, which the driver fills (lowering `count` to
        // how many it wrote).
        let result = unsafe {
            (self.fns.enumerate_physical_devices)(self.handle, &raw mut count, handles.as_mut_ptr())
        };
        if result != SUCCESS && result != INCOMPLETE {
            return Err(VulkanError::Call { call, result });
        }
        let written = usize::try_from(count).unwrap_or(0).min(room);
        handles.truncate(written);
        Ok(handles
            .into_iter()
            .map(|handle| self.describe(handle))
            .collect())
    }

    /// Asks the driver about one device.
    fn describe(&self, handle: PhysicalDeviceHandle) -> PhysicalDevice {
        let mut properties = PhysicalDeviceProperties::zeroed();
        // SAFETY: `handle` was listed by this instance; `properties` is a valid output.
        unsafe { (self.fns.get_properties)(handle, &raw mut properties) };
        let name_bytes: Vec<u8> = properties
            .device_name
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| byte.to_ne_bytes()[0])
            .collect();
        let api_version = ApiVersion::from_raw(properties.api_version);
        // The Vulkan 1.3 feature structure only exists for a device that reports 1.3.
        let (dynamic_rendering, synchronization2, sampler_anisotropy) =
            if BASELINE.is_met_by(api_version) {
                self.features(handle)
            } else {
                (false, false, false)
            };
        PhysicalDevice {
            handle,
            name: String::from_utf8_lossy(&name_bytes).into_owned(),
            driver_version: properties.driver_version,
            vendor_id: properties.vendor_id,
            device_id: properties.device_id,
            swapchain_extension: self.device_has_extension(handle, EXT_SWAPCHAIN),
            max_image_dimension_2d: max_image_dimension_2d(&properties),
            candidate: Candidate {
                api_version,
                device_type: DeviceType::from_raw(properties.device_type),
                graphics_family: self.graphics_family(handle),
                dynamic_rendering,
                synchronization2,
                sampler_anisotropy,
            },
        }
    }

    /// Whether the device offers the device extension `name`; false when it cannot be listed.
    fn device_has_extension(&self, handle: PhysicalDeviceHandle, name: &CStr) -> bool {
        let mut count = 0_u32;
        // SAFETY: a null layer name asks for the device's own extensions; a null array asks for
        // the count only.
        let result = unsafe {
            (self.fns.enumerate_device_extensions)(
                handle,
                core::ptr::null(),
                &raw mut count,
                core::ptr::null_mut(),
            )
        };
        if result != SUCCESS {
            return false;
        }
        count = count.min(MAX_EXTENSIONS);
        let room = usize::try_from(count).unwrap_or(0);
        let mut listed = vec![ExtensionProperties::zeroed(); room];
        // SAFETY: `listed` holds `count` records, which the driver fills (and lowers `count` to
        // how many it wrote).
        let result = unsafe {
            (self.fns.enumerate_device_extensions)(
                handle,
                core::ptr::null(),
                &raw mut count,
                listed.as_mut_ptr(),
            )
        };
        let written = usize::try_from(count).unwrap_or(0).min(room);
        result >= SUCCESS
            && listed
                .iter()
                .take(written)
                .any(|extension| name_is(&extension.extension_name, name))
    }

    /// What the device can do with `format` (the features per tiling).
    pub fn format_properties(&self, gpu: PhysicalDeviceHandle, format: u32) -> FormatProperties {
        let mut properties = FormatProperties::default();
        // SAFETY: `gpu` was listed by this instance; `properties` is a valid output.
        unsafe { (self.fns.get_format_properties)(gpu, format, &raw mut properties) };
        properties
    }

    /// The memory types and heaps of the device.
    pub fn memory_properties(&self, gpu: PhysicalDeviceHandle) -> PhysicalDeviceMemoryProperties {
        let mut properties = PhysicalDeviceMemoryProperties::zeroed();
        // SAFETY: `gpu` was listed by this instance; `properties` is a valid output.
        unsafe { (self.fns.get_memory_properties)(gpu, &raw mut properties) };
        properties
    }

    /// The first queue family with graphics, if any.
    fn graphics_family(&self, handle: PhysicalDeviceHandle) -> Option<u32> {
        let mut count = 0_u32;
        // SAFETY: a null array asks for the count only.
        unsafe {
            (self.fns.get_queue_family_properties)(handle, &raw mut count, core::ptr::null_mut());
        };
        count = count.min(MAX_ENUMERATED);
        let room = usize::try_from(count).unwrap_or(0);
        let zero = QueueFamilyProperties {
            queue_flags: 0,
            queue_count: 0,
            timestamp_valid_bits: 0,
            min_image_transfer_granularity: [0; 3],
        };
        let mut families = vec![zero; room];
        // SAFETY: `families` holds `count` entries, which the driver fills.
        unsafe {
            (self.fns.get_queue_family_properties)(handle, &raw mut count, families.as_mut_ptr());
        }
        let written = usize::try_from(count).unwrap_or(0).min(room);
        families
            .iter()
            .take(written)
            .position(|family| family.queue_flags & QUEUE_GRAPHICS != 0 && family.queue_count > 0)
            .and_then(|index| u32::try_from(index).ok())
    }

    /// `(dynamicRendering, synchronization2, samplerAnisotropy)` of the device.
    fn features(&self, handle: PhysicalDeviceHandle) -> (bool, bool, bool) {
        let mut v13 = Vulkan13Features {
            s_type: STYPE_VULKAN_1_3_FEATURES,
            p_next: core::ptr::null_mut(),
            flags: [0; FEATURES_13_COUNT],
        };
        let mut features = PhysicalDeviceFeatures2 {
            s_type: STYPE_FEATURES_2,
            p_next: (&raw mut v13).cast::<c_void>(),
            features: PhysicalDeviceFeatures {
                flags: [0; CORE_FEATURE_COUNT],
            },
        };
        // SAFETY: `handle` was listed by this instance; `features` and the `v13` it chains to are
        // valid outputs that live until the call returns. Vulkan 1.1 core function, and the
        // instance is 1.3.
        unsafe { (self.fns.get_features2)(handle, &raw mut features) };
        (
            v13.flags[FEATURE_13_DYNAMIC_RENDERING] != 0,
            v13.flags[FEATURE_13_SYNCHRONIZATION2] != 0,
            features.features.flags[FEATURE_SAMPLER_ANISOTROPY] != 0,
        )
    }

    /// Makes a logical device on `gpu`.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result.
    ///
    /// # Safety
    /// `gpu` must be a device listed by this instance, and every pointer in `info` (and in the
    /// structures it points to) must be valid for the call.
    pub unsafe fn create_device(
        &self,
        gpu: PhysicalDeviceHandle,
        info: &DeviceCreateInfo,
    ) -> Result<DeviceHandle, VulkanError> {
        let mut device = DeviceHandle::NULL;
        // SAFETY: by the contract above; `device` is a valid output; the allocator is null.
        let result =
            unsafe { (self.fns.create_device)(gpu, info, core::ptr::null(), &raw mut device) };
        check_created("vkCreateDevice", result, device.0.is_null())?;
        Ok(device)
    }

    /// The address of the device-level function `name` for `device`; null when it does not exist.
    ///
    /// # Safety
    /// `device` must be a live device made by this instance.
    #[must_use]
    pub unsafe fn device_proc_addr(&self, device: DeviceHandle, name: &CStr) -> *mut c_void {
        // SAFETY: by the contract above; `name` is NUL-terminated.
        unsafe { (self.fns.get_device_proc_addr)(device, name.as_ptr()) }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some((fns, messenger)) = self.messenger.take() {
            // SAFETY: the messenger was made on this instance and is destroyed once, before the
            // instance (and before `log`, which its callback writes to).
            unsafe { (fns.destroy_messenger)(self.handle, messenger, core::ptr::null()) };
        }
        // SAFETY: the instance is live, and every object made from it (devices) was destroyed
        // first, by the context's field order.
        unsafe { (self.fns.destroy_instance)(self.handle, core::ptr::null()) };
    }
}
