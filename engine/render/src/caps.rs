//! What a device can do. Engine code branches on these, never on which backend it talks to.

use crate::desc::IndexType;

/// Capabilities reported by `IRenderDevice::caps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderCaps {
    /// Largest texture width or height in texels.
    pub max_texture_size: u32,
    /// Required alignment in bytes of a uniform buffer offset.
    pub uniform_buffer_alignment: u32,
    /// Whether 16-bit indices are supported.
    pub index_u16: bool,
    /// Whether 32-bit indices are supported.
    pub index_u32: bool,
    /// Whether clip-space depth is 0..1 natively (the engine convention).
    pub depth_zero_to_one: bool,
    /// Human-readable device name, for logs and reports only.
    pub device_name: &'static str,
}

impl RenderCaps {
    /// Whether the device accepts indices of this type.
    #[must_use]
    pub fn supports_index(&self, index_type: IndexType) -> bool {
        match index_type {
            IndexType::U16 => self.index_u16,
            IndexType::U32 => self.index_u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_support_follows_the_flags() {
        let caps = RenderCaps {
            max_texture_size: 4096,
            uniform_buffer_alignment: 256,
            index_u16: true,
            index_u32: false,
            depth_zero_to_one: true,
            device_name: "test",
        };
        assert!(caps.supports_index(IndexType::U16));
        assert!(!caps.supports_index(IndexType::U32));
    }
}
