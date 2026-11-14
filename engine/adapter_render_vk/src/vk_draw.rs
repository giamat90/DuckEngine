//! Declarations for passes with depth and for drawing: the stage and access bits the render
//! states use, and the structures of shaders, pipelines and
//! descriptors. Values are from the Vulkan 1.3 headers (Vulkan-Headers 1.3.296, 64-bit targets).

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

use crate::vk_present::Rect2D;
use crate::vk_resource::SamplerHandle;
use core::ffi::{c_char, c_void};

/// `VkShaderModule`, `VkPipeline`, `VkPipelineLayout`, `VkDescriptorSetLayout`,
/// `VkDescriptorPool` and `VkDescriptorSet`: all non-dispatchable handles.
pub type Handle = u64;

/// `VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO`.
pub const STYPE_SHADER_MODULE_CREATE_INFO: u32 = 16;
/// `VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO`.
pub const STYPE_PIPELINE_SHADER_STAGE: u32 = 18;
/// `VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_VERTEX_INPUT: u32 = 19;
/// `VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_INPUT_ASSEMBLY: u32 = 20;
/// `VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_VIEWPORT: u32 = 22;
/// `VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_RASTERIZATION: u32 = 23;
/// `VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_MULTISAMPLE: u32 = 24;
/// `VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_DEPTH_STENCIL: u32 = 25;
/// `VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_COLOR_BLEND: u32 = 26;
/// `VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO`.
pub const STYPE_PIPELINE_DYNAMIC: u32 = 27;
/// `VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO`.
pub const STYPE_GRAPHICS_PIPELINE: u32 = 28;
/// `VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO`.
pub const STYPE_PIPELINE_LAYOUT: u32 = 30;
/// `VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO`.
pub const STYPE_DESCRIPTOR_SET_LAYOUT: u32 = 32;
/// `VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO`.
pub const STYPE_DESCRIPTOR_POOL: u32 = 33;
/// `VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO`.
pub const STYPE_DESCRIPTOR_SET_ALLOCATE: u32 = 34;
/// `VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET`.
pub const STYPE_WRITE_DESCRIPTOR_SET: u32 = 35;
/// `VK_STRUCTURE_TYPE_PIPELINE_RENDERING_CREATE_INFO`.
pub const STYPE_PIPELINE_RENDERING: u32 = 1_000_044_002;

/// `VK_SHADER_STAGE_VERTEX_BIT`.
pub const SHADER_STAGE_VERTEX: u32 = 0x1;
/// `VK_SHADER_STAGE_FRAGMENT_BIT`.
pub const SHADER_STAGE_FRAGMENT: u32 = 0x10;

/// `VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER`.
pub const DESCRIPTOR_COMBINED_IMAGE_SAMPLER: u32 = 1;
/// `VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER`.
pub const DESCRIPTOR_UNIFORM_BUFFER: u32 = 6;

/// `VK_PIPELINE_BIND_POINT_GRAPHICS`.
pub const BIND_POINT_GRAPHICS: u32 = 0;
/// `VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST`.
pub const TOPOLOGY_TRIANGLE_LIST: u32 = 3;
/// `VK_POLYGON_MODE_FILL`.
pub const POLYGON_FILL: u32 = 0;
/// `VK_CULL_MODE_NONE`.
pub const CULL_NONE: u32 = 0;
/// `VK_FRONT_FACE_COUNTER_CLOCKWISE`.
pub const FRONT_COUNTER_CLOCKWISE: u32 = 0;
/// `VK_VERTEX_INPUT_RATE_VERTEX`.
pub const INPUT_RATE_VERTEX: u32 = 0;
/// `VK_COMPARE_OP_LESS_OR_EQUAL`.
pub const COMPARE_LESS_OR_EQUAL: u32 = 3;
/// `VK_COMPARE_OP_ALWAYS`.
pub const COMPARE_ALWAYS: u32 = 7;
/// `VK_DYNAMIC_STATE_VIEWPORT`.
pub const DYNAMIC_VIEWPORT: u32 = 0;
/// `VK_DYNAMIC_STATE_SCISSOR`.
pub const DYNAMIC_SCISSOR: u32 = 1;
/// `VK_BLEND_FACTOR_ZERO`.
pub const BLEND_ZERO: u32 = 0;
/// `VK_BLEND_FACTOR_ONE`.
pub const BLEND_ONE: u32 = 1;
/// `VK_BLEND_FACTOR_SRC_ALPHA`.
pub const BLEND_SRC_ALPHA: u32 = 6;
/// `VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA`.
pub const BLEND_ONE_MINUS_SRC_ALPHA: u32 = 7;
/// `VK_BLEND_OP_ADD`.
pub const BLEND_OP_ADD: u32 = 0;
/// `VK_COLOR_COMPONENT_R_BIT | G | B | A`.
pub const COLOR_WRITE_ALL: u32 = 0xF;
/// `VK_INDEX_TYPE_UINT16`.
pub const INDEX_UINT16: u32 = 0;
/// `VK_INDEX_TYPE_UINT32`.
pub const INDEX_UINT32: u32 = 1;

/// `VK_FORMAT_R32G32_SFLOAT`.
pub const FORMAT_R32G32_SFLOAT: u32 = 103;
/// `VK_FORMAT_R32G32B32_SFLOAT`.
pub const FORMAT_R32G32B32_SFLOAT: u32 = 106;
/// `VK_FORMAT_R32G32B32A32_SFLOAT`.
pub const FORMAT_R32G32B32A32_SFLOAT: u32 = 109;
/// `VK_FORMAT_UNDEFINED`.
pub const FORMAT_UNDEFINED: u32 = 0;

/// `VkShaderModuleCreateInfo`.
#[repr(C)]
pub struct ShaderModuleCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub code_size: usize,
    pub code: *const u32,
}

/// `VkDescriptorSetLayoutBinding`.
#[repr(C)]
pub struct DescriptorSetLayoutBinding {
    pub binding: u32,
    pub descriptor_type: u32,
    pub descriptor_count: u32,
    pub stage_flags: u32,
    pub immutable_samplers: *const SamplerHandle,
}

/// `VkDescriptorSetLayoutCreateInfo`.
#[repr(C)]
pub struct DescriptorSetLayoutCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub binding_count: u32,
    pub bindings: *const DescriptorSetLayoutBinding,
}

/// `VkPipelineLayoutCreateInfo`.
#[repr(C)]
pub struct PipelineLayoutCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub set_layout_count: u32,
    pub set_layouts: *const Handle,
    pub push_constant_range_count: u32,
    pub push_constant_ranges: *const c_void,
}

/// `VkDescriptorPoolSize`.
#[repr(C)]
pub struct DescriptorPoolSize {
    pub descriptor_type: u32,
    pub descriptor_count: u32,
}

/// `VkDescriptorPoolCreateInfo`.
#[repr(C)]
pub struct DescriptorPoolCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub max_sets: u32,
    pub pool_size_count: u32,
    pub pool_sizes: *const DescriptorPoolSize,
}

/// `VkDescriptorSetAllocateInfo`.
#[repr(C)]
pub struct DescriptorSetAllocateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub descriptor_pool: Handle,
    pub descriptor_set_count: u32,
    pub set_layouts: *const Handle,
}

/// `VkDescriptorBufferInfo`.
#[repr(C)]
pub struct DescriptorBufferInfo {
    pub buffer: Handle,
    pub offset: u64,
    pub range: u64,
}

/// `VkDescriptorImageInfo`.
#[repr(C)]
pub struct DescriptorImageInfo {
    pub sampler: SamplerHandle,
    pub image_view: Handle,
    pub image_layout: u32,
}

/// `VkWriteDescriptorSet`.
#[repr(C)]
pub struct WriteDescriptorSet {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub dst_set: Handle,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptor_count: u32,
    pub descriptor_type: u32,
    pub image_info: *const DescriptorImageInfo,
    pub buffer_info: *const DescriptorBufferInfo,
    pub texel_buffer_view: *const Handle,
}

/// `VkPipelineShaderStageCreateInfo`.
#[repr(C)]
pub struct PipelineShaderStage {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub stage: u32,
    pub module: Handle,
    pub name: *const c_char,
    pub specialization_info: *const c_void,
}

/// `VkVertexInputBindingDescription`.
#[repr(C)]
pub struct VertexInputBinding {
    pub binding: u32,
    pub stride: u32,
    pub input_rate: u32,
}

/// `VkVertexInputAttributeDescription`.
#[repr(C)]
pub struct VertexInputAttribute {
    pub location: u32,
    pub binding: u32,
    pub format: u32,
    pub offset: u32,
}

/// `VkPipelineVertexInputStateCreateInfo`.
#[repr(C)]
pub struct PipelineVertexInput {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub binding_count: u32,
    pub bindings: *const VertexInputBinding,
    pub attribute_count: u32,
    pub attributes: *const VertexInputAttribute,
}

/// `VkPipelineInputAssemblyStateCreateInfo`.
#[repr(C)]
pub struct PipelineInputAssembly {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub topology: u32,
    pub primitive_restart_enable: u32,
}

/// `VkViewport`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

/// `VkPipelineViewportStateCreateInfo`.
#[repr(C)]
pub struct PipelineViewport {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub viewport_count: u32,
    pub viewports: *const Viewport,
    pub scissor_count: u32,
    pub scissors: *const Rect2D,
}

/// `VkPipelineRasterizationStateCreateInfo`.
#[repr(C)]
pub struct PipelineRasterization {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub depth_clamp_enable: u32,
    pub rasterizer_discard_enable: u32,
    pub polygon_mode: u32,
    pub cull_mode: u32,
    pub front_face: u32,
    pub depth_bias_enable: u32,
    pub depth_bias_constant_factor: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope_factor: f32,
    pub line_width: f32,
}

/// `VkPipelineMultisampleStateCreateInfo`.
#[repr(C)]
pub struct PipelineMultisample {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub rasterization_samples: u32,
    pub sample_shading_enable: u32,
    pub min_sample_shading: f32,
    pub sample_mask: *const u32,
    pub alpha_to_coverage_enable: u32,
    pub alpha_to_one_enable: u32,
}

/// `VkStencilOpState`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct StencilOpState {
    pub fail_op: u32,
    pub pass_op: u32,
    pub depth_fail_op: u32,
    pub compare_op: u32,
    pub compare_mask: u32,
    pub write_mask: u32,
    pub reference: u32,
}

/// `VkPipelineDepthStencilStateCreateInfo`.
#[repr(C)]
pub struct PipelineDepthStencil {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub depth_test_enable: u32,
    pub depth_write_enable: u32,
    pub depth_compare_op: u32,
    pub depth_bounds_test_enable: u32,
    pub stencil_test_enable: u32,
    pub front: StencilOpState,
    pub back: StencilOpState,
    pub min_depth_bounds: f32,
    pub max_depth_bounds: f32,
}

/// `VkPipelineColorBlendAttachmentState`.
#[repr(C)]
pub struct ColorBlendAttachment {
    pub blend_enable: u32,
    pub src_color_blend_factor: u32,
    pub dst_color_blend_factor: u32,
    pub color_blend_op: u32,
    pub src_alpha_blend_factor: u32,
    pub dst_alpha_blend_factor: u32,
    pub alpha_blend_op: u32,
    pub color_write_mask: u32,
}

/// `VkPipelineColorBlendStateCreateInfo`.
#[repr(C)]
pub struct PipelineColorBlend {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub logic_op_enable: u32,
    pub logic_op: u32,
    pub attachment_count: u32,
    pub attachments: *const ColorBlendAttachment,
    pub blend_constants: [f32; 4],
}

/// `VkPipelineDynamicStateCreateInfo`.
#[repr(C)]
pub struct PipelineDynamic {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub dynamic_state_count: u32,
    pub dynamic_states: *const u32,
}

/// `VkPipelineRenderingCreateInfo`.
#[repr(C)]
pub struct PipelineRendering {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub view_mask: u32,
    pub color_attachment_count: u32,
    pub color_attachment_formats: *const u32,
    pub depth_attachment_format: u32,
    pub stencil_attachment_format: u32,
}

/// `VkGraphicsPipelineCreateInfo`.
#[repr(C)]
pub struct GraphicsPipelineCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub stage_count: u32,
    pub stages: *const PipelineShaderStage,
    pub vertex_input: *const PipelineVertexInput,
    pub input_assembly: *const PipelineInputAssembly,
    pub tessellation: *const c_void,
    pub viewport: *const PipelineViewport,
    pub rasterization: *const PipelineRasterization,
    pub multisample: *const PipelineMultisample,
    pub depth_stencil: *const PipelineDepthStencil,
    pub color_blend: *const PipelineColorBlend,
    pub dynamic: *const PipelineDynamic,
    pub layout: Handle,
    pub render_pass: Handle,
    pub subpass: u32,
    pub base_pipeline: Handle,
    pub base_pipeline_index: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    /// Sizes worked out field by field from `vulkan_core.h` for 64-bit targets.
    #[test]
    fn the_descriptor_structures_have_the_sizes_of_the_headers() {
        assert_eq!(size_of::<ShaderModuleCreateInfo>(), 40);
        assert_eq!(size_of::<DescriptorSetLayoutBinding>(), 24);
        assert_eq!(size_of::<DescriptorSetLayoutCreateInfo>(), 32);
        assert_eq!(size_of::<PipelineLayoutCreateInfo>(), 48);
        assert_eq!(size_of::<DescriptorPoolSize>(), 8);
        assert_eq!(size_of::<DescriptorPoolCreateInfo>(), 40);
        assert_eq!(size_of::<DescriptorSetAllocateInfo>(), 40);
        assert_eq!(size_of::<DescriptorBufferInfo>(), 24);
        assert_eq!(size_of::<DescriptorImageInfo>(), 24);
        assert_eq!(size_of::<WriteDescriptorSet>(), 64);
    }

    #[test]
    fn the_pipeline_structures_have_the_sizes_of_the_headers() {
        assert_eq!(size_of::<PipelineShaderStage>(), 48);
        assert_eq!(size_of::<VertexInputBinding>(), 12);
        assert_eq!(size_of::<VertexInputAttribute>(), 16);
        assert_eq!(size_of::<PipelineVertexInput>(), 48);
        assert_eq!(size_of::<PipelineInputAssembly>(), 32);
        assert_eq!(size_of::<Viewport>(), 24);
        assert_eq!(size_of::<PipelineViewport>(), 48);
        assert_eq!(size_of::<PipelineRasterization>(), 64);
    }

    #[test]
    fn the_pipeline_state_structures_have_the_sizes_of_the_headers() {
        assert_eq!(size_of::<PipelineMultisample>(), 48);
        assert_eq!(size_of::<StencilOpState>(), 28);
        assert_eq!(size_of::<PipelineDepthStencil>(), 104);
        assert_eq!(size_of::<ColorBlendAttachment>(), 32);
        assert_eq!(size_of::<PipelineColorBlend>(), 56);
        assert_eq!(size_of::<PipelineDynamic>(), 32);
        assert_eq!(size_of::<PipelineRendering>(), 40);
        assert_eq!(size_of::<GraphicsPipelineCreateInfo>(), 144);
    }
}
