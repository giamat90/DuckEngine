//! The Vulkan context of part 1: loader, instance, one chosen physical device, the logical
//! device and its graphics queue.

use crate::debug::ValidationReport;
use crate::device::Device;
use crate::entry::Entry;
use crate::error::VulkanError;
use crate::instance::Instance;
use crate::select::{Candidate, DeviceType, choose_device};
use crate::version::ApiVersion;
use crate::vk::QueueHandle;

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
}

/// A Vulkan 1.3 instance and device with one graphics queue.
///
/// Fields are dropped in order: the device first, then the instance, then the loader. That is
/// the order Vulkan requires.
pub struct VulkanContext {
    info: DeviceInfo,
    #[expect(
        dead_code,
        reason = "the swapchain and submit code of card 6-7 uses the queue"
    )]
    queue: QueueHandle,
    device: Device,
    instance: Instance,
    _entry: Entry,
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
        };
        Ok(Self {
            info,
            queue,
            device,
            instance,
            _entry: entry,
        })
    }

    /// What the driver said about the chosen device.
    #[must_use]
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// What the validation layer reported so far (empty when it is off).
    #[must_use]
    pub fn validation_report(&self) -> ValidationReport {
        self.instance.validation_report()
    }

    /// Waits until the device has finished all submitted work.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver's result (for example device lost).
    pub fn wait_idle(&self) -> Result<(), VulkanError> {
        self.device.wait_idle()
    }
}

#[cfg(test)]
mod tests;
