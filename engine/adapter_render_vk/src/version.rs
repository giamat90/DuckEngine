//! Vulkan version numbers and the baseline the adapter requires.

use core::fmt;

/// A Vulkan API version: `variant << 29 | major << 22 | minor << 12 | patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiVersion(u32);

/// The oldest Vulkan the adapter runs on: 1.3 core, which has dynamic rendering and
/// synchronization2 (choice O3 of the phase 6 design note).
pub const BASELINE: ApiVersion = ApiVersion::new(1, 3, 0);

impl ApiVersion {
    /// Packs a standard (variant 0) version.
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self((major << 22) | (minor << 12) | patch)
    }

    /// Reads the packed value a driver or the loader returned.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The packed value, as Vulkan takes it.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The variant: 0 for Khronos Vulkan.
    #[must_use]
    pub const fn variant(self) -> u32 {
        self.0 >> 29
    }

    /// The major version.
    #[must_use]
    pub const fn major(self) -> u32 {
        (self.0 >> 22) & 0x7F
    }

    /// The minor version.
    #[must_use]
    pub const fn minor(self) -> u32 {
        (self.0 >> 12) & 0x3FF
    }

    /// The patch level.
    #[must_use]
    pub const fn patch(self) -> u32 {
        self.0 & 0xFFF
    }

    /// Whether `other` is at least this version in major and minor (the patch level does not
    /// count) and is a standard variant.
    #[must_use]
    pub const fn is_met_by(self, other: Self) -> bool {
        other.variant() == 0
            && (other.major() > self.major()
                || (other.major() == self.major() && other.minor() >= self.minor()))
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major(), self.minor(), self.patch())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_packed_the_way_vulkan_packs_it() {
        let v = ApiVersion::new(1, 3, 296);
        assert_eq!(v.raw(), (1 << 22) | (3 << 12) | 0x128);
        assert_eq!((v.major(), v.minor(), v.patch()), (1, 3, 296));
    }

    #[test]
    fn a_raw_value_from_a_driver_is_read_back() {
        // 1.4.318, as lavapipe on Ubuntu 24.04 reports it (card 6-5).
        let v = ApiVersion::from_raw(0x0040_4000 | 0x13E);
        assert_eq!((v.major(), v.minor(), v.patch()), (1, 4, 318));
        assert_eq!(v.to_string(), "1.4.318");
    }

    #[test]
    fn the_baseline_is_vulkan_1_3() {
        assert!(BASELINE.is_met_by(ApiVersion::new(1, 3, 0)));
        assert!(BASELINE.is_met_by(ApiVersion::new(1, 4, 357)));
        assert!(BASELINE.is_met_by(ApiVersion::new(2, 0, 0)));
        assert!(!BASELINE.is_met_by(ApiVersion::new(1, 2, 198)));
        assert!(!BASELINE.is_met_by(ApiVersion::new(1, 0, 0)));
    }

    #[test]
    fn a_patch_level_never_lifts_an_older_minor_version() {
        assert!(!BASELINE.is_met_by(ApiVersion::new(1, 2, 4095)));
    }

    #[test]
    fn a_non_zero_variant_is_not_standard_vulkan() {
        // Variant bits are the top three; a variant other than 0 is not a Khronos version.
        let odd = ApiVersion::from_raw((1 << 29) | ApiVersion::new(1, 4, 0).raw());
        assert!(!BASELINE.is_met_by(odd));
    }
}
