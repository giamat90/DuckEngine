//! Which physical device to use: a pure choice over what the driver said, so it runs without a
//! GPU.

use crate::version::{ApiVersion, BASELINE};

/// The kind of a physical device (`VkPhysicalDeviceType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceType {
    /// Anything the adapter does not name.
    Other,
    /// A GPU that shares memory with the CPU.
    Integrated,
    /// A separate GPU.
    Discrete,
    /// A GPU in a virtual machine.
    Virtual,
    /// A software device (for example lavapipe).
    Cpu,
}

impl DeviceType {
    /// Reads `VkPhysicalDeviceType`; a value the adapter does not know is `Other`.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Integrated,
            2 => Self::Discrete,
            3 => Self::Virtual,
            4 => Self::Cpu,
            _ => Self::Other,
        }
    }

    /// Higher is preferred: discrete, integrated, virtual, software, other.
    const fn rank(self) -> u8 {
        match self {
            Self::Discrete => 4,
            Self::Integrated => 3,
            Self::Virtual => 2,
            Self::Cpu => 1,
            Self::Other => 0,
        }
    }
}

/// What the adapter asks a physical device about.
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    /// The API version the device supports.
    pub api_version: ApiVersion,
    /// The kind of device.
    pub device_type: DeviceType,
    /// The index of a queue family with graphics (which also transfers), if there is one.
    pub graphics_family: Option<u32>,
    /// Feature `dynamicRendering` (Vulkan 1.3).
    pub dynamic_rendering: bool,
    /// Feature `synchronization2` (Vulkan 1.3).
    pub synchronization2: bool,
    /// Feature `samplerAnisotropy`.
    pub sampler_anisotropy: bool,
}

impl Candidate {
    /// Whether the device can run the adapter at all.
    fn qualifies(&self) -> bool {
        BASELINE.is_met_by(self.api_version)
            && self.graphics_family.is_some()
            && self.dynamic_rendering
            && self.synchronization2
            && self.sampler_anisotropy
    }
}

/// The index of the device to use: among those that qualify, the best kind, and the first of
/// equals. `None` when no device qualifies.
#[must_use]
pub fn choose_device(candidates: &[Candidate]) -> Option<usize> {
    let mut best: Option<(usize, u8)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        if !candidate.qualifies() {
            continue;
        }
        let rank = candidate.device_type.rank();
        if best.is_none_or(|(_, best_rank)| rank > best_rank) {
            best = Some((index, rank));
        }
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good(device_type: DeviceType) -> Candidate {
        Candidate {
            api_version: ApiVersion::new(1, 3, 0),
            device_type,
            graphics_family: Some(0),
            dynamic_rendering: true,
            synchronization2: true,
            sampler_anisotropy: true,
        }
    }

    #[test]
    fn no_devices_is_no_choice() {
        assert_eq!(choose_device(&[]), None);
    }

    #[test]
    fn a_device_that_meets_the_baseline_is_chosen() {
        assert_eq!(choose_device(&[good(DeviceType::Cpu)]), Some(0));
    }

    #[test]
    fn a_discrete_gpu_beats_an_integrated_one_whatever_the_order() {
        let both = [good(DeviceType::Integrated), good(DeviceType::Discrete)];
        assert_eq!(choose_device(&both), Some(1));
        let both = [good(DeviceType::Discrete), good(DeviceType::Integrated)];
        assert_eq!(choose_device(&both), Some(0));
    }

    #[test]
    fn the_order_of_kinds_is_discrete_integrated_virtual_cpu_other() {
        let all = [
            good(DeviceType::Other),
            good(DeviceType::Cpu),
            good(DeviceType::Virtual),
            good(DeviceType::Integrated),
            good(DeviceType::Discrete),
        ];
        for skip in 0..all.len() - 1 {
            // Without the best ones, the next kind wins: remove from the end.
            let rest = &all[..all.len() - skip];
            assert_eq!(choose_device(rest), Some(rest.len() - 1));
        }
    }

    #[test]
    fn equal_devices_keep_the_first_one() {
        let two = [good(DeviceType::Discrete), good(DeviceType::Discrete)];
        assert_eq!(choose_device(&two), Some(0));
    }

    #[test]
    fn a_device_below_vulkan_1_3_is_refused() {
        let mut old = good(DeviceType::Discrete);
        old.api_version = ApiVersion::new(1, 2, 198);
        assert_eq!(choose_device(&[old]), None);
        assert_eq!(choose_device(&[old, good(DeviceType::Cpu)]), Some(1));
    }

    #[test]
    fn each_missing_requirement_refuses_the_device() {
        let mut no_queue = good(DeviceType::Discrete);
        no_queue.graphics_family = None;
        let mut no_dynamic = good(DeviceType::Discrete);
        no_dynamic.dynamic_rendering = false;
        let mut no_sync2 = good(DeviceType::Discrete);
        no_sync2.synchronization2 = false;
        let mut no_aniso = good(DeviceType::Discrete);
        no_aniso.sampler_anisotropy = false;
        for bad in [no_queue, no_dynamic, no_sync2, no_aniso] {
            assert_eq!(choose_device(&[bad]), None, "{bad:?}");
            // A weaker device that qualifies still wins over a stronger one that does not.
            assert_eq!(choose_device(&[bad, good(DeviceType::Cpu)]), Some(1));
        }
    }

    #[test]
    fn raw_device_types_map_to_kinds_and_unknown_values_are_other() {
        assert_eq!(DeviceType::from_raw(0), DeviceType::Other);
        assert_eq!(DeviceType::from_raw(1), DeviceType::Integrated);
        assert_eq!(DeviceType::from_raw(2), DeviceType::Discrete);
        assert_eq!(DeviceType::from_raw(3), DeviceType::Virtual);
        assert_eq!(DeviceType::from_raw(4), DeviceType::Cpu);
        assert_eq!(DeviceType::from_raw(99), DeviceType::Other);
        assert_eq!(DeviceType::from_raw(u32::MAX), DeviceType::Other);
    }
}
