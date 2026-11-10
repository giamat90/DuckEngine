//! The swapchain choices as pure functions: format, image count, extent, usage, alpha.
//! Nothing here calls Vulkan, so the rules are tested without a device.

use crate::error::VulkanError;
use crate::vk_present::{
    COLOR_SPACE_SRGB_NONLINEAR, COMPOSITE_ALPHA_OPAQUE, EXTENT_FOLLOWS_SWAPCHAIN, Extent2D,
    FORMAT_B8G8R8A8_UNORM, IMAGE_USAGE_COLOR_ATTACHMENT, IMAGE_USAGE_TRANSFER_SRC,
    PhysicalDeviceMemoryProperties, SurfaceCapabilities, SurfaceFormat,
};

/// The fewest swapchain images the adapter asks for (double buffering).
const MIN_IMAGES: u32 = 2;

/// `Bgra8` UNORM in the sRGB non-linear colour space, the one format of the engine (design note
/// section 3), if the surface offers it.
#[must_use]
pub fn choose_format(offered: &[SurfaceFormat]) -> Option<SurfaceFormat> {
    offered.iter().copied().find(|candidate| {
        candidate.format == FORMAT_B8G8R8A8_UNORM
            && candidate.color_space == COLOR_SPACE_SRGB_NONLINEAR
    })
}

/// One image more than the minimum (so the driver is never starved), at least two, at most the
/// surface's maximum (zero means no maximum).
#[must_use]
pub fn choose_image_count(caps: &SurfaceCapabilities) -> u32 {
    let wanted = caps.min_image_count.saturating_add(1).max(MIN_IMAGES);
    if caps.max_image_count == 0 {
        wanted
    } else {
        wanted.min(caps.max_image_count)
    }
}

/// The surface's own size when it has one, else the requested size within the limits.
#[must_use]
pub fn choose_extent(caps: &SurfaceCapabilities, width: u32, height: u32) -> Extent2D {
    let current = caps.current_extent;
    if current.width != EXTENT_FOLLOWS_SWAPCHAIN && current.height != EXTENT_FOLLOWS_SWAPCHAIN {
        return current;
    }
    let within = |value: u32, low: u32, high: u32| value.max(low).min(high);
    Extent2D {
        width: within(
            width,
            caps.min_image_extent.width,
            caps.max_image_extent.width,
        ),
        height: within(
            height,
            caps.min_image_extent.height,
            caps.max_image_extent.height,
        ),
    }
}

/// The image usage: a colour attachment (required), and a copy source when the surface allows
/// it (the tests read pixels back). `None` when the images cannot be drawn on.
#[must_use]
pub fn choose_usage(caps: &SurfaceCapabilities) -> Option<u32> {
    let supported = caps.supported_usage_flags;
    if supported & IMAGE_USAGE_COLOR_ATTACHMENT == 0 {
        return None;
    }
    Some(IMAGE_USAGE_COLOR_ATTACHMENT | (supported & IMAGE_USAGE_TRANSFER_SRC))
}

/// Opaque if supported, else the lowest supported bit; `None` when the surface lists none.
#[must_use]
pub fn choose_composite_alpha(caps: &SurfaceCapabilities) -> Option<u32> {
    let supported = caps.supported_composite_alpha;
    if supported & COMPOSITE_ALPHA_OPAQUE != 0 {
        return Some(COMPOSITE_ALPHA_OPAQUE);
    }
    (supported != 0).then(|| 1 << supported.trailing_zeros())
}

/// Whether a swapchain of this size can exist (a minimised window has none).
#[must_use]
pub fn has_area(extent: Extent2D) -> bool {
    extent.width > 0 && extent.height > 0
}

/// Everything a swapchain is made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// How many images to ask for.
    pub image_count: u32,
    /// The format and colour space.
    pub format: SurfaceFormat,
    /// The size in pixels.
    pub extent: Extent2D,
    /// The image usage flags.
    pub usage: u32,
    /// The composite alpha bit.
    pub alpha: u32,
    /// The surface's current transform, passed through.
    pub transform: u32,
}

/// The swapchain to make for a surface: `Ok(None)` when the window has no area right now (it is
/// minimised), an error when the surface cannot be used at all.
///
/// # Errors
/// [`VulkanError::Unsupported`] without the engine format, drawable images or a composite alpha.
pub fn make_plan(
    caps: &SurfaceCapabilities,
    formats: &[SurfaceFormat],
    width: u32,
    height: u32,
) -> Result<Option<Plan>, VulkanError> {
    let extent = choose_extent(caps, width, height);
    if !has_area(extent) {
        return Ok(None);
    }
    let format = choose_format(formats).ok_or(VulkanError::Unsupported(
        "the surface offers no Bgra8 UNORM format",
    ))?;
    let usage = choose_usage(caps).ok_or(VulkanError::Unsupported(
        "the surface images cannot be drawn on",
    ))?;
    let alpha = choose_composite_alpha(caps).ok_or(VulkanError::Unsupported(
        "the surface lists no composite alpha mode",
    ))?;
    Ok(Some(Plan {
        image_count: choose_image_count(caps),
        format,
        extent,
        usage,
        alpha,
        transform: caps.current_transform,
    }))
}

/// The first memory type that `type_bits` allows and that has every property in `wanted`.
#[must_use]
pub fn pick_memory_type(
    properties: &PhysicalDeviceMemoryProperties,
    type_bits: u32,
    wanted: u32,
) -> Option<u32> {
    let count = usize::try_from(properties.memory_type_count).unwrap_or(0);
    properties
        .memory_types
        .iter()
        .take(count)
        .enumerate()
        .find(|(index, memory)| {
            let allowed = u32::try_from(*index)
                .ok()
                .and_then(|shift| type_bits.checked_shr(shift))
                .is_some_and(|bits| bits & 1 != 0);
            allowed && memory.property_flags & wanted == wanted
        })
        .and_then(|(index, _)| u32::try_from(index).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VulkanError;
    use crate::vk_present::{
        COLOR_SPACE_SRGB_NONLINEAR, COMPOSITE_ALPHA_OPAQUE, EXTENT_FOLLOWS_SWAPCHAIN, Extent2D,
        FORMAT_B8G8R8A8_UNORM, IMAGE_USAGE_COLOR_ATTACHMENT, IMAGE_USAGE_TRANSFER_SRC,
        MEMORY_HOST_COHERENT, MEMORY_HOST_VISIBLE, PhysicalDeviceMemoryProperties,
        SurfaceCapabilities, SurfaceFormat,
    };

    const OTHER_FORMAT: u32 = 37; // VK_FORMAT_R8G8B8A8_UNORM
    const SRGB_FORMAT: u32 = 50; // VK_FORMAT_B8G8R8A8_SRGB
    const HDR_SPACE: u32 = 1_000_104_001;

    fn caps(min: u32, max: u32) -> SurfaceCapabilities {
        SurfaceCapabilities {
            min_image_count: min,
            max_image_count: max,
            current_extent: Extent2D {
                width: 800,
                height: 600,
            },
            min_image_extent: Extent2D {
                width: 1,
                height: 1,
            },
            max_image_extent: Extent2D {
                width: 4096,
                height: 4096,
            },
            max_image_array_layers: 1,
            supported_transforms: 1,
            current_transform: 1,
            supported_composite_alpha: COMPOSITE_ALPHA_OPAQUE,
            supported_usage_flags: IMAGE_USAGE_COLOR_ATTACHMENT | IMAGE_USAGE_TRANSFER_SRC,
        }
    }

    fn format(format: u32, color_space: u32) -> SurfaceFormat {
        SurfaceFormat {
            format,
            color_space,
        }
    }

    #[test]
    fn the_engine_format_is_bgra8_unorm_in_srgb_nonlinear() {
        let offered = [
            format(OTHER_FORMAT, COLOR_SPACE_SRGB_NONLINEAR),
            format(FORMAT_B8G8R8A8_UNORM, COLOR_SPACE_SRGB_NONLINEAR),
        ];
        assert_eq!(choose_format(&offered), Some(offered[1]));
    }

    #[test]
    fn a_surface_without_bgra8_unorm_has_no_format() {
        let offered = [
            format(OTHER_FORMAT, COLOR_SPACE_SRGB_NONLINEAR),
            format(SRGB_FORMAT, COLOR_SPACE_SRGB_NONLINEAR),
            format(FORMAT_B8G8R8A8_UNORM, HDR_SPACE),
        ];
        assert_eq!(choose_format(&offered), None);
        assert_eq!(choose_format(&[]), None);
    }

    #[test]
    fn one_image_more_than_the_minimum_is_asked_for() {
        assert_eq!(choose_image_count(&caps(2, 8)), 3);
        assert_eq!(choose_image_count(&caps(3, 0)), 4);
    }

    #[test]
    fn the_image_count_stays_within_the_maximum_and_is_at_least_two() {
        assert_eq!(choose_image_count(&caps(3, 3)), 3);
        assert_eq!(choose_image_count(&caps(1, 1)), 1);
        assert_eq!(choose_image_count(&caps(0, 0)), 2);
    }

    #[test]
    fn a_fixed_surface_size_wins_over_the_request() {
        assert_eq!(
            choose_extent(&caps(2, 8), 320, 200),
            Extent2D {
                width: 800,
                height: 600
            }
        );
    }

    #[test]
    fn a_free_surface_size_takes_the_request_within_the_limits() {
        let mut free = caps(2, 8);
        free.current_extent = Extent2D {
            width: EXTENT_FOLLOWS_SWAPCHAIN,
            height: EXTENT_FOLLOWS_SWAPCHAIN,
        };
        free.min_image_extent = Extent2D {
            width: 16,
            height: 16,
        };
        assert_eq!(
            choose_extent(&free, 320, 200),
            Extent2D {
                width: 320,
                height: 200
            }
        );
        assert_eq!(
            choose_extent(&free, 1, 99_999),
            Extent2D {
                width: 16,
                height: 4096
            }
        );
    }

    #[test]
    fn the_images_are_colour_targets_and_copy_sources_when_the_surface_allows() {
        assert_eq!(
            choose_usage(&caps(2, 8)),
            Some(IMAGE_USAGE_COLOR_ATTACHMENT | IMAGE_USAGE_TRANSFER_SRC)
        );
        let mut no_copy = caps(2, 8);
        no_copy.supported_usage_flags = IMAGE_USAGE_COLOR_ATTACHMENT;
        assert_eq!(choose_usage(&no_copy), Some(IMAGE_USAGE_COLOR_ATTACHMENT));
    }

    #[test]
    fn a_surface_that_cannot_be_drawn_on_has_no_usage() {
        let mut none = caps(2, 8);
        none.supported_usage_flags = IMAGE_USAGE_TRANSFER_SRC;
        assert_eq!(choose_usage(&none), None);
    }

    #[test]
    fn opaque_alpha_is_preferred_and_any_other_supported_bit_is_the_fallback() {
        assert_eq!(
            choose_composite_alpha(&caps(2, 8)),
            Some(COMPOSITE_ALPHA_OPAQUE)
        );
        let mut pre = caps(2, 8);
        pre.supported_composite_alpha = 0x2 | 0x4;
        assert_eq!(choose_composite_alpha(&pre), Some(0x2));
        pre.supported_composite_alpha = 0;
        assert_eq!(choose_composite_alpha(&pre), None);
    }

    #[test]
    fn a_window_with_no_area_cannot_have_a_swapchain() {
        assert!(!has_area(Extent2D {
            width: 0,
            height: 600
        }));
        assert!(!has_area(Extent2D {
            width: 800,
            height: 0
        }));
        assert!(has_area(Extent2D {
            width: 1,
            height: 1
        }));
    }

    fn plan_of(
        caps: &SurfaceCapabilities,
        formats: &[SurfaceFormat],
    ) -> Result<Option<Plan>, VulkanError> {
        make_plan(caps, formats, 800, 600)
    }

    fn good_formats() -> [SurfaceFormat; 1] {
        [format(FORMAT_B8G8R8A8_UNORM, COLOR_SPACE_SRGB_NONLINEAR)]
    }

    #[test]
    fn a_plan_gathers_every_choice() {
        let Ok(Some(plan)) = plan_of(&caps(2, 8), &good_formats()) else {
            panic!("a plan was expected");
        };
        assert_eq!(plan.image_count, 3);
        assert_eq!(plan.format, good_formats()[0]);
        assert_eq!(
            plan.extent,
            Extent2D {
                width: 800,
                height: 600
            }
        );
        assert_eq!(
            plan.usage,
            IMAGE_USAGE_COLOR_ATTACHMENT | IMAGE_USAGE_TRANSFER_SRC
        );
        assert_eq!(plan.alpha, COMPOSITE_ALPHA_OPAQUE);
        assert_eq!(plan.transform, 1);
    }

    #[test]
    fn a_surface_with_no_area_gives_no_plan_but_no_error() {
        let mut flat = caps(2, 8);
        flat.current_extent = Extent2D {
            width: 0,
            height: 0,
        };
        assert_eq!(plan_of(&flat, &[]), Ok(None));
    }

    #[test]
    fn a_surface_that_cannot_be_used_is_unsupported() {
        let no_format = plan_of(&caps(2, 8), &[format(OTHER_FORMAT, 0)]);
        assert!(matches!(no_format, Err(VulkanError::Unsupported(_))));
        let mut no_usage = caps(2, 8);
        no_usage.supported_usage_flags = 0;
        assert!(matches!(
            plan_of(&no_usage, &good_formats()),
            Err(VulkanError::Unsupported(_))
        ));
        let mut no_alpha = caps(2, 8);
        no_alpha.supported_composite_alpha = 0;
        assert!(matches!(
            plan_of(&no_alpha, &good_formats()),
            Err(VulkanError::Unsupported(_))
        ));
    }

    fn memory(types: &[u32]) -> PhysicalDeviceMemoryProperties {
        let mut properties = PhysicalDeviceMemoryProperties::zeroed();
        properties.memory_type_count = u32::try_from(types.len()).unwrap_or(0);
        for (slot, flags) in properties.memory_types.iter_mut().zip(types) {
            slot.property_flags = *flags;
        }
        properties
    }

    #[test]
    fn the_first_allowed_memory_type_with_all_wanted_properties_is_picked() {
        let both = MEMORY_HOST_VISIBLE | MEMORY_HOST_COHERENT;
        let properties = memory(&[1, MEMORY_HOST_VISIBLE, both, both | 1]);
        assert_eq!(pick_memory_type(&properties, 0b1111, both), Some(2));
        assert_eq!(pick_memory_type(&properties, 0b1000, both), Some(3));
        assert_eq!(pick_memory_type(&properties, 0b0011, both), None);
        assert_eq!(pick_memory_type(&properties, 0, both), None);
    }

    #[test]
    fn memory_types_past_the_count_are_not_considered() {
        let both = MEMORY_HOST_VISIBLE | MEMORY_HOST_COHERENT;
        let mut properties = memory(&[1]);
        properties.memory_types[5].property_flags = both;
        assert_eq!(pick_memory_type(&properties, u32::MAX, both), None);
    }
}
