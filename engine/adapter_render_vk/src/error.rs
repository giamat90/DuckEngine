//! Errors of the Vulkan adapter. Values, never panics.

use crate::version::ApiVersion;
use crate::vk::ERROR_INITIALIZATION_FAILED;
use core::fmt;

/// Why the adapter could not start, or a Vulkan call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VulkanError {
    /// The system has no Vulkan loader (`vulkan-1.dll`, `libvulkan.so.1`).
    NoLoader,
    /// The loader or a driver lacks a function the adapter needs.
    MissingFunction(&'static str),
    /// The loader supports less than Vulkan 1.3.
    InstanceTooOld(ApiVersion),
    /// No physical device has Vulkan 1.3 with dynamic rendering, synchronization2, sampler
    /// anisotropy and a graphics queue.
    NoSuitableDevice,
    /// A Vulkan call returned an error code.
    Call {
        /// The Vulkan function.
        call: &'static str,
        /// The `VkResult`.
        result: i32,
    },
}

/// The name of a `VkResult` the adapter handles; others are "unknown result".
#[must_use]
pub fn result_name(result: i32) -> &'static str {
    match result {
        0 => "VK_SUCCESS",
        -1 => "VK_ERROR_OUT_OF_HOST_MEMORY",
        -2 => "VK_ERROR_OUT_OF_DEVICE_MEMORY",
        -3 => "VK_ERROR_INITIALIZATION_FAILED",
        -4 => "VK_ERROR_DEVICE_LOST",
        -6 => "VK_ERROR_LAYER_NOT_PRESENT",
        -7 => "VK_ERROR_EXTENSION_NOT_PRESENT",
        -8 => "VK_ERROR_FEATURE_NOT_PRESENT",
        -9 => "VK_ERROR_INCOMPATIBLE_DRIVER",
        _ => "unknown result",
    }
}

/// The outcome of a call that makes an object: the driver's result, and whether the handle it
/// wrote is null. A success with a null handle is a driver fault, reported as
/// `VK_ERROR_INITIALIZATION_FAILED`.
pub fn check_created(
    call: &'static str,
    result: i32,
    handle_is_null: bool,
) -> Result<(), VulkanError> {
    match (result, handle_is_null) {
        (0, false) => Ok(()),
        (0, true) => Err(VulkanError::Call {
            call,
            result: ERROR_INITIALIZATION_FAILED,
        }),
        _ => Err(VulkanError::Call { call, result }),
    }
}

impl fmt::Display for VulkanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLoader => f.write_str("no Vulkan loader on this system"),
            Self::MissingFunction(name) => write!(f, "the Vulkan loader lacks {name}"),
            Self::InstanceTooOld(version) => {
                write!(f, "Vulkan {version} is older than the required 1.3")
            }
            Self::NoSuitableDevice => f.write_str(
                "no Vulkan device with 1.3, dynamic rendering, synchronization2 and a graphics queue",
            ),
            Self::Call { call, result } => {
                write!(f, "{call} failed: {} ({result})", result_name(*result))
            }
        }
    }
}

impl std::error::Error for VulkanError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::ApiVersion;

    #[test]
    fn every_error_says_what_went_wrong() {
        let all = [
            VulkanError::NoLoader,
            VulkanError::MissingFunction("vkCreateDevice"),
            VulkanError::InstanceTooOld(ApiVersion::new(1, 1, 0)),
            VulkanError::NoSuitableDevice,
            VulkanError::Call {
                call: "vkCreateInstance",
                result: -9,
            },
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        for (i, text) in texts.iter().enumerate() {
            assert!(!text.is_empty());
            assert!(!texts[..i].contains(text), "{text} repeats");
        }
        assert!(texts[1].contains("vkCreateDevice"));
        assert!(texts[2].contains("1.1.0"));
        assert!(texts[4].contains("vkCreateInstance") && texts[4].contains("-9"));
    }

    #[test]
    fn a_result_code_has_a_name_where_the_adapter_knows_it() {
        assert_eq!(result_name(0), "VK_SUCCESS");
        assert_eq!(result_name(-1), "VK_ERROR_OUT_OF_HOST_MEMORY");
        assert_eq!(result_name(-2), "VK_ERROR_OUT_OF_DEVICE_MEMORY");
        assert_eq!(result_name(-3), "VK_ERROR_INITIALIZATION_FAILED");
        assert_eq!(result_name(-4), "VK_ERROR_DEVICE_LOST");
        assert_eq!(result_name(-6), "VK_ERROR_LAYER_NOT_PRESENT");
        assert_eq!(result_name(-7), "VK_ERROR_EXTENSION_NOT_PRESENT");
        assert_eq!(result_name(-8), "VK_ERROR_FEATURE_NOT_PRESENT");
        assert_eq!(result_name(-9), "VK_ERROR_INCOMPATIBLE_DRIVER");
        assert_eq!(result_name(-1_000_000_000), "unknown result");
    }

    #[test]
    fn a_created_object_needs_a_success_and_a_handle() {
        assert_eq!(check_created("vkCreateX", 0, false), Ok(()));
        assert_eq!(
            check_created("vkCreateX", -2, true),
            Err(VulkanError::Call {
                call: "vkCreateX",
                result: -2
            })
        );
        assert_eq!(
            check_created("vkCreateX", 0, true),
            Err(VulkanError::Call {
                call: "vkCreateX",
                result: -3
            })
        );
    }

    #[test]
    fn a_call_error_shows_the_name_and_the_number() {
        let e = VulkanError::Call {
            call: "vkCreateDevice",
            result: -8,
        };
        let text = e.to_string();
        assert!(text.contains("VK_ERROR_FEATURE_NOT_PRESENT"), "{text}");
        assert!(text.contains("-8"), "{text}");
    }
}
