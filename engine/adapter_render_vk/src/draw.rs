//! Shaders, pipelines, descriptors and draw commands (card 6-14).
//!
//! One fixed binding scheme serves every pipeline: set 0 holds [`BINDINGS`] uniform buffers and
//! set 1 holds [`BINDINGS`] combined image samplers, both visible to the vertex and the fragment
//! stage, and the binding number is the slot the stream binds. The pipeline layout is therefore
//! one object for the device. Descriptor sets are taken from a pool per frame in flight and the
//! pool is reset when the frame comes round again; nothing frees a set on its own.
//!
//! Every object made here is also listed, so that what a caller forgot to destroy is freed when
//! the [`Draw`] is dropped.
#![expect(
    unsafe_code,
    reason = "shaders, pipelines and draw commands are a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::error::VulkanError;
use crate::gear::{DrawFns, load_draw_fns, ok};
use crate::spirv::BINDINGS;
use crate::vk::DeviceHandle;
use crate::vk_draw::{
    self as vd, ColorBlendAttachment, DescriptorBufferInfo, DescriptorImageInfo,
    DescriptorPoolCreateInfo, DescriptorPoolSize, DescriptorSetAllocateInfo,
    DescriptorSetLayoutBinding, DescriptorSetLayoutCreateInfo, GraphicsPipelineCreateInfo, Handle,
    PipelineColorBlend, PipelineDepthStencil, PipelineDynamic, PipelineInputAssembly,
    PipelineLayoutCreateInfo, PipelineMultisample, PipelineRasterization, PipelineRendering,
    PipelineShaderStage, PipelineVertexInput, PipelineViewport, ShaderModuleCreateInfo,
    StencilOpState, VertexInputAttribute, VertexInputBinding, Viewport, WriteDescriptorSet,
};
use crate::vk_present::{BufferHandle, CommandBufferHandle, Rect2D};
use crate::vk_resource::{LAYOUT_SHADER_READ, SamplerHandle};
use core::ffi::CStr;
use std::rc::Rc;

/// `VK_WHOLE_SIZE`: the descriptor covers the buffer from its offset to its end.
const WHOLE_SIZE: u64 = u64::MAX;
/// Sets one frame's pool can give out: two per draw whose bindings changed.
const POOL_SETS: u32 = 2048;
/// Descriptors of each type in a frame's pool.
const POOL_DESCRIPTORS: u32 = POOL_SETS / 2 * BINDINGS;

/// A vertex attribute of a pipeline, in Vulkan terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub location: u32,
    pub format: u32,
    pub offset: u32,
}

/// A shader stage of a pipeline: the module and its entry point.
#[derive(Clone, Copy)]
pub struct Stage<'a> {
    pub module: Handle,
    pub entry: &'a CStr,
}

/// What a graphics pipeline is made from.
pub struct PipelineSpec<'a> {
    pub vertex: Stage<'a>,
    pub fragment: Stage<'a>,
    pub stride: u32,
    pub attributes: &'a [Attribute],
    pub blend: bool,
    pub depth_test: bool,
    pub depth_write: bool,
    pub color_format: u32,
    /// `FORMAT_UNDEFINED` for a pass without depth.
    pub depth_format: u32,
}

/// What is bound to the slots when a draw happens.
#[derive(Clone, Copy, Default)]
pub struct Bindings {
    /// The `VkBuffer` of each uniform slot.
    pub uniforms: [Option<BufferHandle>; BINDINGS as usize],
    /// The image view and sampler of each texture slot.
    pub textures: [Option<(Handle, SamplerHandle)>; BINDINGS as usize],
}

/// The shader, pipeline and descriptor objects of a device.
pub struct Draw {
    fns: DrawFns,
    device: DeviceHandle,
    set_layouts: [Handle; 2],
    layout: Handle,
    shaders: Vec<Handle>,
    pipelines: Vec<Handle>,
    pools: Vec<Handle>,
    // Last: the core outlives the objects made from it.
    _core: Rc<Core>,
}

impl Draw {
    /// Loads the functions and makes the two set layouts and the pipeline layout.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] or [`VulkanError::Call`].
    pub fn new(core: &Rc<Core>) -> Result<Self, VulkanError> {
        let mut draw = Self {
            fns: load_draw_fns(core)?,
            device: core.device.handle(),
            set_layouts: [0; 2],
            layout: 0,
            shaders: Vec::new(),
            pipelines: Vec::new(),
            pools: Vec::new(),
            _core: Rc::clone(core),
        };
        // On a failure the drop of `draw` frees what was made so far.
        draw.set_layouts = [
            draw.make_set_layout(vd::DESCRIPTOR_UNIFORM_BUFFER)?,
            draw.make_set_layout(vd::DESCRIPTOR_COMBINED_IMAGE_SAMPLER)?,
        ];
        draw.layout = draw.make_pipeline_layout()?;
        Ok(draw)
    }

    fn make_set_layout(&self, descriptor_type: u32) -> Result<Handle, VulkanError> {
        let bindings: [DescriptorSetLayoutBinding; BINDINGS as usize] =
            core::array::from_fn(|slot| DescriptorSetLayoutBinding {
                binding: u32::try_from(slot).unwrap_or(0),
                descriptor_type,
                descriptor_count: 1,
                stage_flags: vd::SHADER_STAGE_VERTEX | vd::SHADER_STAGE_FRAGMENT,
                immutable_samplers: core::ptr::null(),
            });
        let info = DescriptorSetLayoutCreateInfo {
            s_type: vd::STYPE_DESCRIPTOR_SET_LAYOUT,
            p_next: core::ptr::null(),
            flags: 0,
            binding_count: BINDINGS,
            bindings: bindings.as_ptr(),
        };
        let mut layout = 0;
        // SAFETY: `info` and `bindings` live until the call returns; `layout` is one handle.
        let result = unsafe {
            (self.fns.create_descriptor_set_layout)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut layout,
            )
        };
        ok("vkCreateDescriptorSetLayout", result).map(|()| layout)
    }

    fn make_pipeline_layout(&self) -> Result<Handle, VulkanError> {
        let info = PipelineLayoutCreateInfo {
            s_type: vd::STYPE_PIPELINE_LAYOUT,
            p_next: core::ptr::null(),
            flags: 0,
            set_layout_count: 2,
            set_layouts: self.set_layouts.as_ptr(),
            push_constant_range_count: 0,
            push_constant_ranges: core::ptr::null(),
        };
        let mut layout = 0;
        // SAFETY: `info` and the set layouts live until the call returns.
        let result = unsafe {
            (self.fns.create_pipeline_layout)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut layout,
            )
        };
        ok("vkCreatePipelineLayout", result).map(|()| layout)
    }

    // ----- shaders -----

    /// Makes a shader module from SPIR-V words (already checked and retargeted).
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver refuses.
    pub fn create_shader(&mut self, words: &[u32]) -> Result<Handle, VulkanError> {
        let info = ShaderModuleCreateInfo {
            s_type: vd::STYPE_SHADER_MODULE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            code_size: words.len() * 4,
            code: words.as_ptr(),
        };
        let mut module = 0;
        // SAFETY: `info` points at `words`, which lives until the call returns.
        let result = unsafe {
            (self.fns.create_shader_module)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut module,
            )
        };
        ok("vkCreateShaderModule", result)?;
        self.shaders.push(module);
        Ok(module)
    }

    /// Destroys a module made by [`Draw::create_shader`]; a handle not listed is ignored.
    pub fn destroy_shader(&mut self, module: Handle) {
        if let Some(at) = self.shaders.iter().position(|known| *known == module) {
            self.shaders.swap_remove(at);
            // SAFETY: the module was made on this device and is not used by a pending draw (the
            // caller waited for the device); a pipeline keeps its own copy of the code.
            unsafe { (self.fns.destroy_shader_module)(self.device, module, core::ptr::null()) };
        }
    }

    // ----- pipelines -----

    /// Makes a graphics pipeline for dynamic rendering.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver refuses.
    pub fn create_pipeline(&mut self, spec: &PipelineSpec<'_>) -> Result<Handle, VulkanError> {
        let stages = [
            shader_stage(vd::SHADER_STAGE_VERTEX, spec.vertex),
            shader_stage(vd::SHADER_STAGE_FRAGMENT, spec.fragment),
        ];
        let binding = [VertexInputBinding {
            binding: 0,
            stride: spec.stride,
            input_rate: vd::INPUT_RATE_VERTEX,
        }];
        let attributes: Vec<VertexInputAttribute> = spec
            .attributes
            .iter()
            .map(|a| VertexInputAttribute {
                location: a.location,
                binding: 0,
                format: a.format,
                offset: a.offset,
            })
            .collect();
        let input = vertex_input(&binding, &attributes);
        let assembly = input_assembly();
        let viewport = viewport_state();
        let raster = rasterization();
        let multisample = multisample();
        let depth = depth_state(spec);
        let attachment = [blend_attachment(spec.blend)];
        let blend = color_blend(&attachment);
        let dynamic_states = [vd::DYNAMIC_VIEWPORT, vd::DYNAMIC_SCISSOR];
        let dynamic = dynamic_state(&dynamic_states);
        let formats = [spec.color_format];
        let rendering = rendering_formats(&formats, spec.depth_format);
        let info = GraphicsPipelineCreateInfo {
            s_type: vd::STYPE_GRAPHICS_PIPELINE,
            p_next: (&raw const rendering).cast(),
            flags: 0,
            stage_count: 2,
            stages: stages.as_ptr(),
            vertex_input: &raw const input,
            input_assembly: &raw const assembly,
            tessellation: core::ptr::null(),
            viewport: &raw const viewport,
            rasterization: &raw const raster,
            multisample: &raw const multisample,
            depth_stencil: &raw const depth,
            color_blend: &raw const blend,
            dynamic: &raw const dynamic,
            layout: self.layout,
            render_pass: 0,
            subpass: 0,
            base_pipeline: 0,
            base_pipeline_index: -1,
        };
        self.submit_pipeline(&info)
    }

    fn submit_pipeline(
        &mut self,
        info: &GraphicsPipelineCreateInfo,
    ) -> Result<Handle, VulkanError> {
        let mut pipeline = 0;
        // SAFETY: `info` and everything it points to live in the caller until this returns.
        let result = unsafe {
            (self.fns.create_graphics_pipelines)(
                self.device,
                0,
                1,
                info,
                core::ptr::null(),
                &raw mut pipeline,
            )
        };
        ok("vkCreateGraphicsPipelines", result)?;
        self.pipelines.push(pipeline);
        Ok(pipeline)
    }

    /// Destroys a pipeline made by [`Draw::create_pipeline`]; a handle not listed is ignored.
    pub fn destroy_pipeline(&mut self, pipeline: Handle) {
        if let Some(at) = self.pipelines.iter().position(|known| *known == pipeline) {
            self.pipelines.swap_remove(at);
            // SAFETY: the pipeline was made on this device and no pending command uses it (the
            // caller waited for the device).
            unsafe { (self.fns.destroy_pipeline)(self.device, pipeline, core::ptr::null()) };
        }
    }

    // ----- descriptor pools -----

    /// Makes a pool for the sets of one frame in flight.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver refuses.
    pub fn create_pool(&mut self) -> Result<Handle, VulkanError> {
        let sizes = [
            DescriptorPoolSize {
                descriptor_type: vd::DESCRIPTOR_UNIFORM_BUFFER,
                descriptor_count: POOL_DESCRIPTORS,
            },
            DescriptorPoolSize {
                descriptor_type: vd::DESCRIPTOR_COMBINED_IMAGE_SAMPLER,
                descriptor_count: POOL_DESCRIPTORS,
            },
        ];
        let info = DescriptorPoolCreateInfo {
            s_type: vd::STYPE_DESCRIPTOR_POOL,
            p_next: core::ptr::null(),
            flags: 0,
            max_sets: POOL_SETS,
            pool_size_count: 2,
            pool_sizes: sizes.as_ptr(),
        };
        let mut pool = 0;
        // SAFETY: `info` and `sizes` live until the call returns.
        let result = unsafe {
            (self.fns.create_descriptor_pool)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut pool,
            )
        };
        ok("vkCreateDescriptorPool", result)?;
        self.pools.push(pool);
        Ok(pool)
    }

    /// Destroys a pool (and with it its sets); a handle not listed is ignored.
    pub fn destroy_pool(&mut self, pool: Handle) {
        if let Some(at) = self.pools.iter().position(|known| *known == pool) {
            self.pools.swap_remove(at);
            // SAFETY: the pool was made on this device and none of its sets is in a pending
            // command buffer (the caller waited for the device).
            unsafe { (self.fns.destroy_descriptor_pool)(self.device, pool, core::ptr::null()) };
        }
    }

    /// Gives back every set of `pool`; the frame that used them has finished.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver refuses.
    pub fn reset_pool(&self, pool: Handle) -> Result<(), VulkanError> {
        // SAFETY: the pool belongs to this device and no command buffer using its sets is pending.
        ok("vkResetDescriptorPool", unsafe {
            (self.fns.reset_descriptor_pool)(self.device, pool, 0)
        })
    }

    // ----- commands -----

    /// Gives the commands a new pair of sets from `pool`, writes the bound slots into them and
    /// binds them.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the pool is out of sets.
    pub fn bind_descriptors(
        &self,
        cmd: CommandBufferHandle,
        pool: Handle,
        bindings: &Bindings,
    ) -> Result<(), VulkanError> {
        let sets = self.allocate_sets(pool)?;
        let uniforms = bindings
            .uniforms
            .iter()
            .enumerate()
            .filter_map(|(slot, buffer)| buffer.map(|buffer| (slot, buffer)));
        for (slot, buffer) in uniforms {
            self.write_uniform(sets[0], slot, buffer);
        }
        let textures = bindings
            .textures
            .iter()
            .enumerate()
            .filter_map(|(slot, texture)| texture.map(|(view, sampler)| (slot, view, sampler)));
        for (slot, view, sampler) in textures {
            self.write_texture(sets[1], slot, view, sampler);
        }
        // SAFETY: the buffer is recording; the layout and the two sets match.
        unsafe {
            (self.fns.cmd_bind_descriptor_sets)(
                cmd,
                vd::BIND_POINT_GRAPHICS,
                self.layout,
                0,
                2,
                sets.as_ptr(),
                0,
                core::ptr::null(),
            );
        }
        Ok(())
    }

    fn allocate_sets(&self, pool: Handle) -> Result<[Handle; 2], VulkanError> {
        let info = DescriptorSetAllocateInfo {
            s_type: vd::STYPE_DESCRIPTOR_SET_ALLOCATE,
            p_next: core::ptr::null(),
            descriptor_pool: pool,
            descriptor_set_count: 2,
            set_layouts: self.set_layouts.as_ptr(),
        };
        let mut sets = [0; 2];
        // SAFETY: `info` lives until the call returns; `sets` has room for the two handles.
        let result = unsafe {
            (self.fns.allocate_descriptor_sets)(self.device, &raw const info, sets.as_mut_ptr())
        };
        ok("vkAllocateDescriptorSets", result).map(|()| sets)
    }

    fn write_uniform(&self, set: Handle, slot: usize, buffer: BufferHandle) {
        let info = DescriptorBufferInfo {
            buffer,
            offset: 0,
            range: WHOLE_SIZE,
        };
        let write = WriteDescriptorSet {
            s_type: vd::STYPE_WRITE_DESCRIPTOR_SET,
            p_next: core::ptr::null(),
            dst_set: set,
            dst_binding: u32::try_from(slot).unwrap_or(0),
            dst_array_element: 0,
            descriptor_count: 1,
            descriptor_type: vd::DESCRIPTOR_UNIFORM_BUFFER,
            image_info: core::ptr::null(),
            buffer_info: &raw const info,
            texel_buffer_view: core::ptr::null(),
        };
        self.update(&write);
    }

    fn write_texture(&self, set: Handle, slot: usize, view: Handle, sampler: SamplerHandle) {
        let info = DescriptorImageInfo {
            sampler,
            image_view: view,
            image_layout: LAYOUT_SHADER_READ,
        };
        let write = WriteDescriptorSet {
            s_type: vd::STYPE_WRITE_DESCRIPTOR_SET,
            p_next: core::ptr::null(),
            dst_set: set,
            dst_binding: u32::try_from(slot).unwrap_or(0),
            dst_array_element: 0,
            descriptor_count: 1,
            descriptor_type: vd::DESCRIPTOR_COMBINED_IMAGE_SAMPLER,
            image_info: &raw const info,
            buffer_info: core::ptr::null(),
            texel_buffer_view: core::ptr::null(),
        };
        self.update(&write);
    }

    fn update(&self, write: &WriteDescriptorSet) {
        // SAFETY: `write` and the info it points to live until the call returns; the set was just
        // allocated and is not in use by a pending command buffer.
        unsafe {
            (self.fns.update_descriptor_sets)(self.device, 1, write, 0, core::ptr::null());
        }
    }

    /// Binds a pipeline.
    pub fn bind_pipeline(&self, cmd: CommandBufferHandle, pipeline: Handle) {
        // SAFETY: the buffer is recording inside a rendering scope; the pipeline is alive.
        unsafe { (self.fns.cmd_bind_pipeline)(cmd, vd::BIND_POINT_GRAPHICS, pipeline) };
    }

    /// Sets the viewport and the scissor rectangle.
    pub fn set_viewport(&self, cmd: CommandBufferHandle, viewport: &Viewport, scissor: &Rect2D) {
        // SAFETY: the buffer is recording; both values live until the calls return.
        unsafe {
            (self.fns.cmd_set_viewport)(cmd, 0, 1, viewport);
            (self.fns.cmd_set_scissor)(cmd, 0, 1, scissor);
        }
    }

    /// Binds the vertex buffer of slot 0.
    pub fn bind_vertex(&self, cmd: CommandBufferHandle, buffer: BufferHandle, offset: u64) {
        // SAFETY: the buffer is recording; one buffer and one offset at the given addresses.
        unsafe {
            (self.fns.cmd_bind_vertex_buffers)(cmd, 0, 1, &raw const buffer, &raw const offset);
        }
    }

    /// Binds the index buffer.
    pub fn bind_index(
        &self,
        cmd: CommandBufferHandle,
        buffer: BufferHandle,
        offset: u64,
        wide: bool,
    ) {
        let index_type = if wide {
            vd::INDEX_UINT32
        } else {
            vd::INDEX_UINT16
        };
        // SAFETY: the buffer is recording; the index buffer is alive.
        unsafe { (self.fns.cmd_bind_index_buffer)(cmd, buffer, offset, index_type) };
    }

    /// Draws `vertices` vertices of `instances` instances.
    pub fn draw(&self, cmd: CommandBufferHandle, vertices: u32, instances: u32) {
        // SAFETY: the buffer is recording inside a rendering scope with a pipeline bound.
        unsafe { (self.fns.cmd_draw)(cmd, vertices, instances, 0, 0) };
    }

    /// Draws `indices` indices of `instances` instances.
    pub fn draw_indexed(&self, cmd: CommandBufferHandle, indices: u32, instances: u32) {
        // SAFETY: the buffer is recording inside a rendering scope with a pipeline and an index
        // buffer bound.
        unsafe { (self.fns.cmd_draw_indexed)(cmd, indices, instances, 0, 0, 0) };
    }
}

impl Drop for Draw {
    fn drop(&mut self) {
        let device = self.device;
        let fns = self.fns;
        // Every object below was made on this device, is listed here once, and the owner has
        // waited for the device (the presenters wait when they are dropped).
        for pool in self.pools.drain(..) {
            // SAFETY: a pool made on this device, destroyed once.
            unsafe { (fns.destroy_descriptor_pool)(device, pool, core::ptr::null()) };
        }
        for pipeline in self.pipelines.drain(..) {
            // SAFETY: a pipeline made on this device, destroyed once.
            unsafe { (fns.destroy_pipeline)(device, pipeline, core::ptr::null()) };
        }
        for module in self.shaders.drain(..) {
            // SAFETY: a module made on this device, destroyed once.
            unsafe { (fns.destroy_shader_module)(device, module, core::ptr::null()) };
        }
        if self.layout != 0 {
            // SAFETY: the pipeline layout made in `new`, after the pipelines that used it.
            unsafe { (fns.destroy_pipeline_layout)(device, self.layout, core::ptr::null()) };
        }
        for layout in self.set_layouts.into_iter().filter(|layout| *layout != 0) {
            // SAFETY: a set layout made in `new`, after the pipeline layout that used it.
            unsafe { (fns.destroy_descriptor_set_layout)(device, layout, core::ptr::null()) };
        }
    }
}

// ----- pipeline state, in small pure builders -----

fn shader_stage(stage: u32, from: Stage<'_>) -> PipelineShaderStage {
    PipelineShaderStage {
        s_type: vd::STYPE_PIPELINE_SHADER_STAGE,
        p_next: core::ptr::null(),
        flags: 0,
        stage,
        module: from.module,
        name: from.entry.as_ptr(),
        specialization_info: core::ptr::null(),
    }
}

fn vertex_input(
    bindings: &[VertexInputBinding; 1],
    attributes: &[VertexInputAttribute],
) -> PipelineVertexInput {
    PipelineVertexInput {
        s_type: vd::STYPE_PIPELINE_VERTEX_INPUT,
        p_next: core::ptr::null(),
        flags: 0,
        binding_count: 1,
        bindings: bindings.as_ptr(),
        attribute_count: u32::try_from(attributes.len()).unwrap_or(0),
        attributes: attributes.as_ptr(),
    }
}

fn input_assembly() -> PipelineInputAssembly {
    PipelineInputAssembly {
        s_type: vd::STYPE_PIPELINE_INPUT_ASSEMBLY,
        p_next: core::ptr::null(),
        flags: 0,
        topology: vd::TOPOLOGY_TRIANGLE_LIST,
        primitive_restart_enable: 0,
    }
}

/// One viewport and one scissor, both set by commands.
fn viewport_state() -> PipelineViewport {
    PipelineViewport {
        s_type: vd::STYPE_PIPELINE_VIEWPORT,
        p_next: core::ptr::null(),
        flags: 0,
        viewport_count: 1,
        viewports: core::ptr::null(),
        scissor_count: 1,
        scissors: core::ptr::null(),
    }
}

/// Filled triangles, no culling (as the OpenGL adapter draws).
fn rasterization() -> PipelineRasterization {
    PipelineRasterization {
        s_type: vd::STYPE_PIPELINE_RASTERIZATION,
        p_next: core::ptr::null(),
        flags: 0,
        depth_clamp_enable: 0,
        rasterizer_discard_enable: 0,
        polygon_mode: vd::POLYGON_FILL,
        cull_mode: vd::CULL_NONE,
        front_face: vd::FRONT_COUNTER_CLOCKWISE,
        depth_bias_enable: 0,
        depth_bias_constant_factor: 0.0,
        depth_bias_clamp: 0.0,
        depth_bias_slope_factor: 0.0,
        line_width: 1.0,
    }
}

fn multisample() -> PipelineMultisample {
    PipelineMultisample {
        s_type: vd::STYPE_PIPELINE_MULTISAMPLE,
        p_next: core::ptr::null(),
        flags: 0,
        rasterization_samples: crate::vk_resource::SAMPLES_1,
        sample_shading_enable: 0,
        min_sample_shading: 0.0,
        sample_mask: core::ptr::null(),
        alpha_to_coverage_enable: 0,
        alpha_to_one_enable: 0,
    }
}

fn depth_state(spec: &PipelineSpec<'_>) -> PipelineDepthStencil {
    PipelineDepthStencil {
        s_type: vd::STYPE_PIPELINE_DEPTH_STENCIL,
        p_next: core::ptr::null(),
        flags: 0,
        depth_test_enable: u32::from(spec.depth_test),
        depth_write_enable: u32::from(spec.depth_write),
        depth_compare_op: if spec.depth_test {
            vd::COMPARE_LESS_OR_EQUAL
        } else {
            vd::COMPARE_ALWAYS
        },
        depth_bounds_test_enable: 0,
        stencil_test_enable: 0,
        front: StencilOpState::default(),
        back: StencilOpState::default(),
        min_depth_bounds: 0.0,
        max_depth_bounds: 1.0,
    }
}

/// Opaque replaces the target; alpha blends by the source alpha (the colour rule of the port).
fn blend_attachment(alpha: bool) -> ColorBlendAttachment {
    let (src, dst) = if alpha {
        (vd::BLEND_SRC_ALPHA, vd::BLEND_ONE_MINUS_SRC_ALPHA)
    } else {
        (vd::BLEND_ONE, vd::BLEND_ZERO)
    };
    ColorBlendAttachment {
        blend_enable: u32::from(alpha),
        src_color_blend_factor: src,
        dst_color_blend_factor: dst,
        color_blend_op: vd::BLEND_OP_ADD,
        src_alpha_blend_factor: src,
        dst_alpha_blend_factor: dst,
        alpha_blend_op: vd::BLEND_OP_ADD,
        color_write_mask: vd::COLOR_WRITE_ALL,
    }
}

fn color_blend(attachment: &[ColorBlendAttachment; 1]) -> PipelineColorBlend {
    PipelineColorBlend {
        s_type: vd::STYPE_PIPELINE_COLOR_BLEND,
        p_next: core::ptr::null(),
        flags: 0,
        logic_op_enable: 0,
        logic_op: 0,
        attachment_count: 1,
        attachments: attachment.as_ptr(),
        blend_constants: [0.0; 4],
    }
}

fn dynamic_state(states: &[u32; 2]) -> PipelineDynamic {
    PipelineDynamic {
        s_type: vd::STYPE_PIPELINE_DYNAMIC,
        p_next: core::ptr::null(),
        flags: 0,
        dynamic_state_count: 2,
        dynamic_states: states.as_ptr(),
    }
}

fn rendering_formats(color: &[u32; 1], depth: u32) -> PipelineRendering {
    PipelineRendering {
        s_type: vd::STYPE_PIPELINE_RENDERING,
        p_next: core::ptr::null(),
        view_mask: 0,
        color_attachment_count: 1,
        color_attachment_formats: color.as_ptr(),
        depth_attachment_format: depth,
        stencil_attachment_format: vd::FORMAT_UNDEFINED,
    }
}

#[cfg(test)]
mod tests;
