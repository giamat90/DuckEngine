//! Declarations for passes with depth and for drawing: the stage and access bits the render
//! states use, and (with the pipeline cards) the structures of shaders, pipelines and
//! descriptors. Values are from the Vulkan 1.3 headers.

/// `VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT`.
pub const STAGE_FRAGMENT_SHADER: u64 = 0x80;
/// `VK_PIPELINE_STAGE_2_EARLY_FRAGMENT_TESTS_BIT`.
pub const STAGE_EARLY_FRAGMENT_TESTS: u64 = 0x100;
/// `VK_PIPELINE_STAGE_2_LATE_FRAGMENT_TESTS_BIT`.
pub const STAGE_LATE_FRAGMENT_TESTS: u64 = 0x200;
/// `VK_ACCESS_2_SHADER_READ_BIT`.
pub const ACCESS_SHADER_READ: u64 = 0x20;
/// `VK_ACCESS_2_COLOR_ATTACHMENT_READ_BIT`.
pub const ACCESS_COLOR_ATTACHMENT_READ: u64 = 0x80;
/// `VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_READ_BIT`.
pub const ACCESS_DEPTH_STENCIL_READ: u64 = 0x200;
/// `VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT`.
pub const ACCESS_DEPTH_STENCIL_WRITE: u64 = 0x400;
