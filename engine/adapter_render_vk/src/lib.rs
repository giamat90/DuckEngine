//! Layer 3 adapter: Vulkan 1.3, loaded at run time (cards 6-6 and 6-7, parts 1 and 2 of the
//! Vulkan render adapter).
//!
//! This is the only code of the engine that knows Vulkan. Part 1 delivers what a device needs
//! before it can draw: the system Vulkan loader is opened at run time (no binding crate, no build
//! dependency), an instance and a physical device are chosen, a logical device and its graphics
//! queue are made. Part 2 adds the [`Presenter`]: a window surface made from the platform's native
//! token, a Bgra8 swapchain, two frames in flight and a clear colour, which a test can read back
//! from the image. Memory and resources (6-8) and the `IRenderDevice` implementation (6-9) follow.
//!
//! Without a Vulkan loader, or on a machine whose devices lack Vulkan 1.3 with dynamic rendering
//! and synchronization2, every call that starts things returns an error value
//! ([`VulkanError`]); nothing panics.

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("the Vulkan adapter supports Windows and Linux only");

// `lookup` first: it defines the `function_table!` macro the modules below use.
#[macro_use]
mod lookup;
mod capture;
mod context;
mod debug;
mod device;
mod dynlib;
mod entry;
mod error;
#[cfg(test)]
mod fake;
mod frame;
mod gear;
mod instance;
mod memory;
mod plan;
mod presenter;
mod resources;
mod select;
mod slab;
mod suballoc;
mod surface;
mod swapchain;
mod upload;
mod version;
mod vk;
mod vk_present;
mod vk_resource;

pub use context::{DeviceInfo, VulkanContext, VulkanOptions};
pub use debug::ValidationReport;
pub use error::VulkanError;
pub use presenter::{FrameOutcome, Presenter};
pub use resources::{
    AddressMode, BufferDesc, BufferId, BufferUsage, Filter, Region, Resources, SamplerDesc,
    SamplerId, TextureDesc, TextureFormat, TextureHandles, TextureId,
};
pub use select::{Candidate, DeviceType, choose_device};
pub use version::{ApiVersion, BASELINE};
