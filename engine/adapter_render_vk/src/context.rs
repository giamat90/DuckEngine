//! The Vulkan context of part 1: loader, instance, one chosen physical device, the logical
//! device and its graphics queue.

use crate::debug::ValidationReport;
use crate::device::Device;
use crate::entry::Entry;
use crate::error::VulkanError;
use crate::instance::Instance;
use crate::select::{Candidate, DeviceType, choose_device};
use crate::version::ApiVersion;
use crate::vk::{PhysicalDeviceHandle, QueueHandle};
use std::rc::Rc;

/// How to start.
#[derive(Clone, Copy, Debug, Default)]
pub struct VulkanOptions {
    /// Turn on the Khronos validation layer (a development aid) if it is installed. Without the
    /// layer the context starts anyway; [`DeviceInfo::validation_active`] says which happened.
    pub validation: bool,
}

/// What the driver says about the device the context chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// `deviceName`, for example `NVIDIA GeForce RTX 3050 Laptop GPU` or `llvmpipe (LLVM 20.1.2, 256 bits)`.
    pub name: String,
    /// The kind of device.
    pub device_type: DeviceType,
    /// The Vulkan version the device supports.
    pub api_version: ApiVersion,
    /// The Vulkan version of the loader (the instance).
    pub instance_version: ApiVersion,
    /// The driver's own version number (vendor-specific encoding).
    pub driver_version: u32,
    /// The PCI vendor id.
    pub vendor_id: u32,
    /// The device id.
    pub device_id: u32,
    /// The queue family of the graphics queue.
    pub queue_family: u32,
    /// Whether the validation layer is on.
    pub validation_active: bool,
    /// Whether the system and the device can present to a window (the surface and swapchain
    /// extensions are there).
    pub can_present: bool,
}

/// What the context and everything made from it share: the objects, owned in the order Vulkan
/// needs them destroyed (the device, then the instance, then the loader).
///
/// A [`crate::Presenter`] holds a reference too, so the device cannot go away under it, whichever
/// of the two is dropped first.
pub(crate) struct Core {
    pub(crate) device: Device,
    pub(crate) instance: Instance,
    pub(crate) _entry: Entry,
    pub(crate) gpu: PhysicalDeviceHandle,
    pub(crate) queue: QueueHandle,
    pub(crate) queue_family: u32,
    pub(crate) can_present: bool,
    pub(crate) max_texture_extent: u32,
}

/// A Vulkan 1.3 instance and device with one graphics queue.
///
/// The objects live in a shared core that is dropped when the context and everything made from it
/// (presenters) are gone: the device first, then the instance, then the loader. That is the order
/// Vulkan requires.
pub struct VulkanContext {
    info: DeviceInfo,
    core: Rc<Core>,
}

impl VulkanContext {
    /// Starts Vulkan: opens the loader, makes the instance, chooses the best physical device
    /// that meets the baseline (Vulkan 1.3, dynamic rendering, synchronization2, sampler
    /// anisotropy, a graphics queue) and makes the device.
    ///
    /// # Errors
    /// [`VulkanError::NoLoader`] without a Vulkan loader; [`VulkanError::InstanceTooOld`] and
    /// [`VulkanError::NoSuitableDevice`] when the system is below the baseline;
    /// [`VulkanError::MissingFunction`] and [`VulkanError::Call`] when the loader or a driver
    /// fails. Whatever was made before the failure is destroyed.
    pub fn create(options: VulkanOptions) -> Result<Self, VulkanError> {
        Self::create_with_entry(Entry::load()?, options)
    }

    /// The same over an entry the caller made (the tests pass a fake driver).
    pub(crate) fn create_with_entry(
        entry: Entry,
        options: VulkanOptions,
    ) -> Result<Self, VulkanError> {
        let instance = Instance::create(&entry, options.validation)?;
        let gpus = instance.physical_devices()?;
        let candidates: Vec<Candidate> = gpus.iter().map(|gpu| gpu.candidate).collect();
        let gpu = choose_device(&candidates)
            .and_then(|index| gpus.get(index))
            .ok_or(VulkanError::NoSuitableDevice)?;
        let queue_family = gpu
            .candidate
            .graphics_family
            .ok_or(VulkanError::NoSuitableDevice)?;
        let (device, queue) = Device::create(&instance, gpu, queue_family)?;
        let can_present = instance.surface_fns().is_some() && gpu.swapchain_extension;
        let info = DeviceInfo {
            name: gpu.name.clone(),
            device_type: gpu.candidate.device_type,
            api_version: gpu.candidate.api_version,
            instance_version: entry.instance_version(),
            driver_version: gpu.driver_version,
            vendor_id: gpu.vendor_id,
            device_id: gpu.device_id,
            queue_family,
            validation_active: instance.validation_active(),
            can_present,
        };
        Ok(Self {
            info,
            core: Rc::new(Core {
                device,
                instance,
                _entry: entry,
                gpu: gpu.handle,
                queue,
                queue_family,
                can_present,
                max_texture_extent: gpu.max_image_dimension_2d,
            }),
        })
    }

    /// The shared objects, for what is made from the context.
    pub(crate) fn core(&self) -> &Rc<Core> {
        &self.core
    }

    /// Makes the buffers, textures and samplers of this device.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] or [`VulkanError::Call`] while setting up.
    pub fn create_resources(&self) -> Result<crate::Resources, VulkanError> {
        crate::Resources::new(&self.core, crate::Resources::DEFAULT_BLOCK)
    }

    /// What the driver said about the chosen device.
    #[must_use]
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// What the validation layer reported so far (empty when it is off).
    #[must_use]
    pub fn validation_report(&self) -> ValidationReport {
        self.core.instance.validation_report()
    }

    /// Waits until the device has finished all submitted work.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result (for example device lost).
    pub fn wait_idle(&self) -> Result<(), VulkanError> {
        self.core.device.wait_idle()
    }
}

#[cfg(test)]
mod tests;
