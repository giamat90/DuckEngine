//! Port types to the adapter's own resource types, and the adapter's errors to port errors.
//! Pure functions, so each mapping is a table that is tested on its own.

use crate::error::VulkanError;
use crate::resources::{
    AddressMode, BufferDesc, BufferUsage, Filter, Region, SamplerDesc, TextureDesc, TextureFormat,
};
use render::desc as port;
use render::error::RenderError;

/// `VK_ERROR_OUT_OF_HOST_MEMORY`.
const OUT_OF_HOST_MEMORY: i32 = -1;
/// `VK_ERROR_OUT_OF_DEVICE_MEMORY`.
const OUT_OF_DEVICE_MEMORY: i32 = -2;
/// `VK_ERROR_DEVICE_LOST`.
const DEVICE_LOST: i32 = -4;

/// The port error for an adapter error. A call that fails for memory is `OutOfMemory`; a lost
/// device or surface is `SurfaceLost` (the app recreates its surface or its device); the rest
/// is `Unsupported`, because the port has no more precise word for a driver failure.
#[must_use]
pub fn render_error(error: &VulkanError) -> RenderError {
    match error {
        VulkanError::SurfaceLost => RenderError::SurfaceLost,
        VulkanError::InvalidHandle => RenderError::InvalidHandle,
        VulkanError::Invalid(_) => RenderError::InvalidDescription,
        VulkanError::Call { result, .. } => match *result {
            OUT_OF_HOST_MEMORY | OUT_OF_DEVICE_MEMORY => RenderError::OutOfMemory,
            DEVICE_LOST => RenderError::SurfaceLost,
            _ => RenderError::Unsupported,
        },
        VulkanError::NoLoader
        | VulkanError::MissingFunction(_)
        | VulkanError::InstanceTooOld(_)
        | VulkanError::NoSuitableDevice
        | VulkanError::Unsupported(_) => RenderError::Unsupported,
    }
}

#[must_use]
pub fn buffer_desc(desc: port::BufferDesc) -> BufferDesc {
    BufferDesc {
        size: desc.size,
        usage: match desc.usage {
            port::BufferUsage::Vertex => BufferUsage::Vertex,
            port::BufferUsage::Index => BufferUsage::Index,
            port::BufferUsage::Uniform => BufferUsage::Uniform,
        },
    }
}

#[must_use]
pub fn texture_format(format: port::TextureFormat) -> TextureFormat {
    match format {
        port::TextureFormat::Rgba8 => TextureFormat::Rgba8,
        port::TextureFormat::Bgra8 => TextureFormat::Bgra8,
        port::TextureFormat::Depth32F => TextureFormat::Depth32F,
    }
}

#[must_use]
pub fn texture_desc(desc: port::TextureDesc) -> TextureDesc {
    TextureDesc {
        width: desc.width,
        height: desc.height,
        mip_count: desc.mip_count,
        format: texture_format(desc.format),
    }
}

fn filter(filter: port::Filter) -> Filter {
    match filter {
        port::Filter::Nearest => Filter::Nearest,
        port::Filter::Linear => Filter::Linear,
    }
}

#[must_use]
pub fn sampler_desc(desc: port::SamplerDesc) -> SamplerDesc {
    SamplerDesc {
        min_filter: filter(desc.min_filter),
        mag_filter: filter(desc.mag_filter),
        address: match desc.address {
            port::AddressMode::Clamp => AddressMode::Clamp,
            port::AddressMode::Repeat => AddressMode::Repeat,
        },
    }
}

/// A port rectangle as a region.
#[must_use]
pub fn region(rect: port::Rect) -> Region {
    Region {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_adapter_error_has_a_port_error() {
        let call = |result| VulkanError::Call {
            call: "vkAllocateMemory",
            result,
        };
        assert_eq!(render_error(&call(-1)), RenderError::OutOfMemory);
        assert_eq!(render_error(&call(-2)), RenderError::OutOfMemory);
        assert_eq!(render_error(&call(-4)), RenderError::SurfaceLost);
        assert_eq!(render_error(&call(-3)), RenderError::Unsupported);
        assert_eq!(
            render_error(&VulkanError::SurfaceLost),
            RenderError::SurfaceLost
        );
        assert_eq!(
            render_error(&VulkanError::InvalidHandle),
            RenderError::InvalidHandle
        );
        assert_eq!(
            render_error(&VulkanError::Invalid("x")),
            RenderError::InvalidDescription
        );
        assert_eq!(
            render_error(&VulkanError::Unsupported("x")),
            RenderError::Unsupported
        );
        assert_eq!(
            render_error(&VulkanError::NoLoader),
            RenderError::Unsupported
        );
    }

    #[test]
    fn descriptions_keep_their_numbers_and_kinds() {
        let buffer = buffer_desc(port::BufferDesc {
            size: 12,
            usage: port::BufferUsage::Index,
        });
        assert_eq!((buffer.size, buffer.usage), (12, BufferUsage::Index));
        let texture = texture_desc(port::TextureDesc {
            width: 3,
            height: 4,
            mip_count: 1,
            format: port::TextureFormat::Depth32F,
        });
        assert_eq!(
            (texture.width, texture.height, texture.format),
            (3, 4, TextureFormat::Depth32F)
        );
        let rect = port::Rect {
            x: -1,
            y: 2,
            width: 3,
            height: 4,
        };
        let region = region(rect);
        assert_eq!(
            (region.x, region.y, region.width, region.height),
            (-1, 2, 3, 4)
        );
        assert_eq!(
            texture_format(port::TextureFormat::Bgra8),
            TextureFormat::Bgra8
        );
        let sampler = sampler_desc(port::SamplerDesc {
            min_filter: port::Filter::Nearest,
            mag_filter: port::Filter::Linear,
            address: port::AddressMode::Repeat,
        });
        assert_eq!(
            (sampler.min_filter, sampler.mag_filter, sampler.address),
            (Filter::Nearest, Filter::Linear, AddressMode::Repeat)
        );
    }
}
