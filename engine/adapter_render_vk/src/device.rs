//! The logical device and its graphics queue.
#![expect(
    unsafe_code,
    reason = "the device is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::entry::{destroy_found, missing};
use crate::error::VulkanError;
use crate::instance::{Instance, PhysicalDevice};
use crate::vk::{
    CORE_FEATURE_COUNT, DeviceCreateInfo, DeviceHandle, DeviceQueueCreateInfo,
    ERROR_INITIALIZATION_FAILED, FEATURE_13_DYNAMIC_RENDERING, FEATURE_13_SYNCHRONIZATION2,
    FEATURE_SAMPLER_ANISOTROPY, FEATURES_13_COUNT, PhysicalDeviceFeatures, PhysicalDeviceFeatures2,
    QueueHandle, STYPE_DEVICE_CREATE_INFO, STYPE_DEVICE_QUEUE_CREATE_INFO, STYPE_FEATURES_2,
    STYPE_VULKAN_1_3_FEATURES, SUCCESS, VkResult, Vulkan13Features,
};
use crate::vk_present::EXT_SWAPCHAIN;
use core::ffi::{CStr, c_char, c_void};

function_table! {
    /// The device-level functions the adapter calls.
    struct DeviceFns {
        destroy_device: unsafe extern "system" fn(DeviceHandle, *const c_void) = c"vkDestroyDevice",
        get_device_queue: unsafe extern "system" fn(DeviceHandle, u32, u32, *mut QueueHandle) = c"vkGetDeviceQueue",
        device_wait_idle: unsafe extern "system" fn(DeviceHandle) -> VkResult = c"vkDeviceWaitIdle",
    }
}

/// A logical device. Dropping it waits for the device to be idle and destroys it.
///
/// Holds function pointers into the driver: the [`Instance`] it came from must outlive it.
pub struct Device {
    handle: DeviceHandle,
    fns: DeviceFns,
}

impl Device {
    /// Makes a device on `gpu` with one queue from `family` and the features the baseline needs
    /// (dynamic rendering, synchronization2, sampler anisotropy), and fetches the queue.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver refuses; [`VulkanError::MissingFunction`] when it
    /// lacks a device function (the device is destroyed first).
    pub fn create(
        instance: &Instance,
        gpu: &PhysicalDevice,
        family: u32,
    ) -> Result<(Self, QueueHandle), VulkanError> {
        let priority = 1.0_f32;
        let queue_info = DeviceQueueCreateInfo {
            s_type: STYPE_DEVICE_QUEUE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            queue_family_index: family,
            queue_count: 1,
            queue_priorities: &raw const priority,
        };
        let mut v13 = Vulkan13Features {
            s_type: STYPE_VULKAN_1_3_FEATURES,
            p_next: core::ptr::null_mut(),
            flags: [0; FEATURES_13_COUNT],
        };
        v13.flags[FEATURE_13_DYNAMIC_RENDERING] = 1;
        v13.flags[FEATURE_13_SYNCHRONIZATION2] = 1;
        let mut core_features = PhysicalDeviceFeatures {
            flags: [0; CORE_FEATURE_COUNT],
        };
        core_features.flags[FEATURE_SAMPLER_ANISOTROPY] = 1;
        let features = PhysicalDeviceFeatures2 {
            s_type: STYPE_FEATURES_2,
            p_next: (&raw mut v13).cast::<c_void>(),
            features: core_features,
        };
        let extensions = enabled_extensions(gpu);
        let info = DeviceCreateInfo {
            s_type: STYPE_DEVICE_CREATE_INFO,
            // The features go in the chain; `enabled_features` stays null (Vulkan forbids both).
            p_next: (&raw const features).cast::<c_void>(),
            flags: 0,
            queue_create_info_count: 1,
            queue_create_infos: &raw const queue_info,
            enabled_layer_count: 0,
            enabled_layer_names: core::ptr::null(),
            enabled_extension_count: u32::try_from(extensions.len()).unwrap_or(0),
            enabled_extension_names: extensions.as_ptr(),
            enabled_features: core::ptr::null(),
        };
        // SAFETY: `gpu` was listed by `instance`; `info` and the structures it points to live
        // until the call returns.
        let handle = unsafe { instance.create_device(gpu.handle, &info) }?;
        // SAFETY: `handle` is the live device just made by `instance`.
        let lookup = |name: &core::ffi::CStr| unsafe { instance.device_proc_addr(handle, name) };
        // SAFETY: the driver returns functions with the declared signatures for these names.
        let fns = match unsafe { DeviceFns::load(lookup) } {
            Ok(fns) => fns,
            Err(name) => {
                // SAFETY: the address is `vkDestroyDevice` or null; the device is live and
                // destroyed once, here.
                unsafe { destroy_found(lookup(c"vkDestroyDevice"), handle) };
                return Err(missing(name));
            }
        };
        let device = Self { handle, fns };
        let mut queue = QueueHandle::NULL;
        // SAFETY: `family` was requested with one queue, so index 0 exists; `queue` is a valid
        // output.
        unsafe { (device.fns.get_device_queue)(device.handle, family, 0, &raw mut queue) };
        if queue.0.is_null() {
            // `device` is dropped here, which destroys it.
            return Err(VulkanError::Call {
                call: "vkGetDeviceQueue",
                result: ERROR_INITIALIZATION_FAILED,
            });
        }
        Ok((device, queue))
    }

    /// The raw device handle, for the calls that take it.
    pub fn handle(&self) -> DeviceHandle {
        self.handle
    }

    /// Waits until the device has finished all submitted work.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result (for example device lost).
    pub fn wait_idle(&self) -> Result<(), VulkanError> {
        // SAFETY: the device is live.
        let result = unsafe { (self.fns.device_wait_idle)(self.handle) };
        if result == SUCCESS {
            Ok(())
        } else {
            Err(VulkanError::Call {
                call: "vkDeviceWaitIdle",
                result,
            })
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Work still running on the device would use what is destroyed next; the result does not
        // change what happens: the device is destroyed either way.
        let _ = self.wait_idle();
        // SAFETY: the device is live and destroyed once, here; everything made from it was
        // destroyed first.
        unsafe { (self.fns.destroy_device)(self.handle, core::ptr::null()) };
    }
}

/// The device extensions to enable. Presenting needs the swapchain extension; a device without
/// it still works headless.
fn enabled_extensions(gpu: &PhysicalDevice) -> Vec<*const c_char> {
    let wanted: &[&CStr] = if gpu.swapchain_extension {
        &[EXT_SWAPCHAIN]
    } else {
        &[]
    };
    wanted.iter().map(|name| name.as_ptr()).collect()
}
