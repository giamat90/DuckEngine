//! Layer 3 adapter: Vulkan 1.3, loaded at run time (card 6-6, part 1 of the Vulkan render
//! adapter).
//!
//! This is the only code of the engine that knows Vulkan. Part 1 delivers what a device needs
//! before it can draw: the system Vulkan loader is opened at run time (no binding crate, no build
//! dependency), an instance and a physical device are chosen, a logical device and its graphics
//! queue are made. The surface and swapchain (6-7), memory and resources (6-8) and the
//! `IRenderDevice` implementation (6-9) follow.
//!
//! Without a Vulkan loader, or on a machine whose devices lack Vulkan 1.3 with dynamic rendering
//! and synchronization2, every call that starts things returns an error value
//! ([`VulkanError`]); nothing panics.

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("the Vulkan adapter supports Windows and Linux only");

// `lookup` first: it defines the `function_table!` macro the modules below use.
#[macro_use]
mod lookup;
mod context;
mod debug;
mod device;
mod dynlib;
mod entry;
mod error;
#[cfg(test)]
mod fake;
mod instance;
mod select;
mod version;
mod vk;

pub use context::{DeviceInfo, VulkanContext, VulkanOptions};
pub use debug::ValidationReport;
pub use error::VulkanError;
pub use select::{Candidate, DeviceType, choose_device};
pub use version::{ApiVersion, BASELINE};
