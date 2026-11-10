//! The draw half of the fake driver: shader modules, descriptor layouts and pools, pipelines and
//! the commands that bind and draw. Every command is recorded, so a test can read back what the
//! adapter asked for; creating calls check what the validation layer would (a pipeline names a
//! layout that is alive and the dynamic state the adapter relies on, a set comes from a pool that
//! has room) and panic on a violation.

use super::{note, with};
use crate::vk::DeviceHandle;
use crate::vk_draw::{
    self as vd, DescriptorPoolCreateInfo, DescriptorSetAllocateInfo, DescriptorSetLayoutCreateInfo,
    GraphicsPipelineCreateInfo, Handle, PipelineLayoutCreateInfo, PipelineRendering,
    ShaderModuleCreateInfo, Viewport, WriteDescriptorSet,
};
use crate::vk_present::{CommandBufferHandle, Rect2D};
use core::ffi::c_void;
use std::collections::HashMap;

/// `VK_ERROR_OUT_OF_HOST_MEMORY`.
const OUT_OF_HOST_MEMORY: i32 = -1;
/// `VK_ERROR_OUT_OF_POOL_MEMORY`.
const OUT_OF_POOL_MEMORY: i32 = -1_000_069_000;

/// A descriptor pool: how many sets it holds and how many it has given out.
#[derive(Clone, Copy, Debug)]
pub struct PoolRecord {
    pub max_sets: u32,
    pub allocated: u32,
}

/// What `vkCreateGraphicsPipelines` was asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct PipelineRecord {
    /// Stage bit and module of each stage, in order.
    pub stages: Vec<(u32, Handle)>,
    pub entries: Vec<String>,
    pub stride: u32,
    /// Location, format and offset of each attribute.
    pub attributes: Vec<(u32, u32, u32)>,
    pub color_format: u32,
    pub depth_format: u32,
    pub depth_test: bool,
    pub depth_write: bool,
    pub blend: bool,
    pub layout: Handle,
}

/// One descriptor write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteRecord {
    pub set: Handle,
    pub binding: u32,
    pub descriptor_type: u32,
    /// The buffer of a uniform write, or the image view of a texture write.
    pub object: Handle,
    /// The sampler of a texture write, else 0.
    pub sampler: Handle,
    /// The layout of a texture write, else 0.
    pub layout: u32,
}

/// Each draw-related command, in the order recorded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawCommand {
    BindPipeline(Handle),
    BindSets {
        first: u32,
        sets: [Handle; 2],
    },
    BindVertex {
        buffer: u64,
        offset: u64,
    },
    BindIndex {
        buffer: u64,
        offset: u64,
        index_type: u32,
    },
    Viewport(Viewport),
    Scissor(Rect2D),
    Draw {
        vertices: u32,
        instances: u32,
    },
    DrawIndexed {
        indices: u32,
        instances: u32,
    },
}

/// The state of the draw half.
pub struct DrawWorld {
    pub modules: HashMap<Handle, Vec<u32>>,
    pub set_layouts: Vec<Handle>,
    pub pipeline_layouts: HashMap<Handle, Vec<Handle>>,
    pub pools: HashMap<Handle, PoolRecord>,
    pub pipelines: HashMap<Handle, PipelineRecord>,
    pub commands: Vec<DrawCommand>,
    pub writes: Vec<WriteRecord>,
    pub pool_resets: u32,
    next_set: Handle,
}

impl DrawWorld {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            set_layouts: Vec::new(),
            pipeline_layouts: HashMap::new(),
            pools: HashMap::new(),
            pipelines: HashMap::new(),
            commands: Vec::new(),
            writes: Vec::new(),
            pool_resets: 0,
            next_set: 0x9000_0000,
        }
    }
}

fn refused(call: &str) -> bool {
    with(|script| script.fail_call == Some(call))
}

fn new_handle(kind: &'static str) -> u64 {
    super::present::alloc(kind)
}

macro_rules! address {
    ($f:expr) => {
        ($f as *const ()).cast_mut().cast::<c_void>()
    };
}

/// The address of the draw function `name`, or null.
pub fn lookup(name: &str) -> *mut c_void {
    let objects = lookup_objects(name);
    if objects.is_null() {
        lookup_commands(name)
    } else {
        objects
    }
}

/// The functions that make, change and destroy objects.
fn lookup_objects(name: &str) -> *mut c_void {
    match name {
        "vkCreateShaderModule" => {
            address!(create_shader_module as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyShaderModule" => {
            address!(destroy_shader_module as unsafe extern "system" fn(_, _, _))
        }
        "vkCreateDescriptorSetLayout" => {
            address!(create_set_layout as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyDescriptorSetLayout" => {
            address!(destroy_set_layout as unsafe extern "system" fn(_, _, _))
        }
        "vkCreatePipelineLayout" => {
            address!(create_pipeline_layout as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyPipelineLayout" => {
            address!(destroy_pipeline_layout as unsafe extern "system" fn(_, _, _))
        }
        "vkCreateDescriptorPool" => {
            address!(create_pool as unsafe extern "system" fn(_, _, _, _) -> _)
        }
        "vkDestroyDescriptorPool" => {
            address!(destroy_pool as unsafe extern "system" fn(_, _, _))
        }
        "vkResetDescriptorPool" => {
            address!(reset_pool as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkAllocateDescriptorSets" => {
            address!(allocate_sets as unsafe extern "system" fn(_, _, _) -> _)
        }
        "vkUpdateDescriptorSets" => {
            address!(update_sets as unsafe extern "system" fn(_, _, _, _, _))
        }
        "vkCreateGraphicsPipelines" => {
            address!(create_pipelines as unsafe extern "system" fn(_, _, _, _, _, _) -> _)
        }
        "vkDestroyPipeline" => address!(destroy_pipeline as unsafe extern "system" fn(_, _, _)),
        _ => core::ptr::null_mut(),
    }
}

/// The functions that record into a command buffer.
fn lookup_commands(name: &str) -> *mut c_void {
    match name {
        "vkCmdBindPipeline" => address!(cmd_bind_pipeline as unsafe extern "system" fn(_, _, _)),
        "vkCmdBindDescriptorSets" => {
            address!(cmd_bind_sets as unsafe extern "system" fn(_, _, _, _, _, _, _, _))
        }
        "vkCmdBindVertexBuffers" => {
            address!(cmd_bind_vertex as unsafe extern "system" fn(_, _, _, _, _))
        }
        "vkCmdBindIndexBuffer" => {
            address!(cmd_bind_index as unsafe extern "system" fn(_, _, _, _))
        }
        "vkCmdSetViewport" => address!(cmd_viewport as unsafe extern "system" fn(_, _, _, _)),
        "vkCmdSetScissor" => address!(cmd_scissor as unsafe extern "system" fn(_, _, _, _)),
        "vkCmdDraw" => address!(cmd_draw as unsafe extern "system" fn(_, _, _, _, _)),
        "vkCmdDrawIndexed" => {
            address!(cmd_draw_indexed as unsafe extern "system" fn(_, _, _, _, _, _))
        }
        _ => core::ptr::null_mut(),
    }
}

unsafe extern "system" fn create_shader_module(
    _device: DeviceHandle,
    info: *const ShaderModuleCreateInfo,
    _allocator: *const c_void,
    out: *mut Handle,
) -> i32 {
    note("vkCreateShaderModule");
    if refused("vkCreateShaderModule") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vd::STYPE_SHADER_MODULE_CREATE_INFO);
    assert_eq!(info.code_size % 4, 0, "SPIR-V is whole words");
    let words = unsafe { core::slice::from_raw_parts(info.code, info.code_size / 4) }.to_vec();
    let handle = new_handle("shader module");
    with(|script| script.draw.modules.insert(handle, words));
    unsafe { *out = handle };
    0
}

unsafe extern "system" fn destroy_shader_module(
    _device: DeviceHandle,
    module: Handle,
    _allocator: *const c_void,
) {
    note("vkDestroyShaderModule");
    super::present::release("shader module", module);
    with(|script| script.draw.modules.remove(&module));
}

unsafe extern "system" fn create_set_layout(
    _device: DeviceHandle,
    info: *const DescriptorSetLayoutCreateInfo,
    _allocator: *const c_void,
    out: *mut Handle,
) -> i32 {
    note("vkCreateDescriptorSetLayout");
    if refused("vkCreateDescriptorSetLayout") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vd::STYPE_DESCRIPTOR_SET_LAYOUT);
    let handle = new_handle("set layout");
    with(|script| script.draw.set_layouts.push(handle));
    unsafe { *out = handle };
    0
}

unsafe extern "system" fn destroy_set_layout(
    _device: DeviceHandle,
    layout: Handle,
    _allocator: *const c_void,
) {
    note("vkDestroyDescriptorSetLayout");
    super::present::release("set layout", layout);
    with(|script| script.draw.set_layouts.retain(|known| *known != layout));
}

unsafe extern "system" fn create_pipeline_layout(
    _device: DeviceHandle,
    info: *const PipelineLayoutCreateInfo,
    _allocator: *const c_void,
    out: *mut Handle,
) -> i32 {
    note("vkCreatePipelineLayout");
    if refused("vkCreatePipelineLayout") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vd::STYPE_PIPELINE_LAYOUT);
    let sets =
        unsafe { core::slice::from_raw_parts(info.set_layouts, info.set_layout_count as usize) }
            .to_vec();
    with(|script| {
        for set in &sets {
            assert!(
                script.draw.set_layouts.contains(set),
                "a set layout that is not alive"
            );
        }
    });
    let handle = new_handle("pipeline layout");
    with(|script| script.draw.pipeline_layouts.insert(handle, sets));
    unsafe { *out = handle };
    0
}

unsafe extern "system" fn destroy_pipeline_layout(
    _device: DeviceHandle,
    layout: Handle,
    _allocator: *const c_void,
) {
    note("vkDestroyPipelineLayout");
    super::present::release("pipeline layout", layout);
    with(|script| script.draw.pipeline_layouts.remove(&layout));
}

unsafe extern "system" fn create_pool(
    _device: DeviceHandle,
    info: *const DescriptorPoolCreateInfo,
    _allocator: *const c_void,
    out: *mut Handle,
) -> i32 {
    note("vkCreateDescriptorPool");
    if refused("vkCreateDescriptorPool") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vd::STYPE_DESCRIPTOR_POOL);
    let handle = new_handle("descriptor pool");
    with(|script| {
        script.draw.pools.insert(
            handle,
            PoolRecord {
                max_sets: info.max_sets,
                allocated: 0,
            },
        );
    });
    unsafe { *out = handle };
    0
}

unsafe extern "system" fn destroy_pool(
    _device: DeviceHandle,
    pool: Handle,
    _allocator: *const c_void,
) {
    note("vkDestroyDescriptorPool");
    super::present::release("descriptor pool", pool);
    with(|script| script.draw.pools.remove(&pool));
}

unsafe extern "system" fn reset_pool(_device: DeviceHandle, pool: Handle, _flags: u32) -> i32 {
    note("vkResetDescriptorPool");
    with(|script| {
        let Some(record) = script.draw.pools.get_mut(&pool) else {
            panic!("a pool that is not alive was reset");
        };
        record.allocated = 0;
        script.draw.pool_resets += 1;
    });
    0
}

unsafe extern "system" fn allocate_sets(
    _device: DeviceHandle,
    info: *const DescriptorSetAllocateInfo,
    out: *mut Handle,
) -> i32 {
    note("vkAllocateDescriptorSets");
    if refused("vkAllocateDescriptorSets") {
        return OUT_OF_HOST_MEMORY;
    }
    let info = unsafe { &*info };
    assert_eq!(info.s_type, vd::STYPE_DESCRIPTOR_SET_ALLOCATE);
    with(|script| {
        let world = &mut script.draw;
        let Some(pool) = world.pools.get_mut(&info.descriptor_pool) else {
            panic!("sets from a pool that is not alive");
        };
        if pool.allocated + info.descriptor_set_count > pool.max_sets {
            return OUT_OF_POOL_MEMORY;
        }
        pool.allocated += info.descriptor_set_count;
        for index in 0..info.descriptor_set_count as usize {
            world.next_set += 1;
            unsafe { *out.add(index) = world.next_set };
        }
        0
    })
}

unsafe extern "system" fn update_sets(
    _device: DeviceHandle,
    count: u32,
    writes: *const WriteDescriptorSet,
    copies: u32,
    _copy_info: *const c_void,
) {
    note("vkUpdateDescriptorSets");
    assert_eq!(copies, 0);
    for write in unsafe { core::slice::from_raw_parts(writes, count as usize) } {
        assert_eq!(write.s_type, vd::STYPE_WRITE_DESCRIPTOR_SET);
        assert_eq!(write.descriptor_count, 1);
        let record = if write.descriptor_type == vd::DESCRIPTOR_UNIFORM_BUFFER {
            let info = unsafe { &*write.buffer_info };
            WriteRecord {
                set: write.dst_set,
                binding: write.dst_binding,
                descriptor_type: write.descriptor_type,
                object: info.buffer,
                sampler: 0,
                layout: 0,
            }
        } else {
            let info = unsafe { &*write.image_info };
            WriteRecord {
                set: write.dst_set,
                binding: write.dst_binding,
                descriptor_type: write.descriptor_type,
                object: info.image_view,
                sampler: info.sampler,
                layout: info.image_layout,
            }
        };
        with(|script| script.draw.writes.push(record));
    }
}

unsafe extern "system" fn create_pipelines(
    _device: DeviceHandle,
    _cache: Handle,
    count: u32,
    infos: *const GraphicsPipelineCreateInfo,
    _allocator: *const c_void,
    out: *mut Handle,
) -> i32 {
    note("vkCreateGraphicsPipelines");
    if refused("vkCreateGraphicsPipelines") {
        return OUT_OF_HOST_MEMORY;
    }
    assert_eq!(count, 1);
    let info = unsafe { &*infos };
    assert_eq!(info.s_type, vd::STYPE_GRAPHICS_PIPELINE);
    let record = unsafe { read_pipeline(info) };
    let handle = new_handle("pipeline");
    with(|script| script.draw.pipelines.insert(handle, record));
    unsafe { *out = handle };
    0
}

/// Reads and checks a pipeline description.
unsafe fn read_pipeline(info: &GraphicsPipelineCreateInfo) -> PipelineRecord {
    let stages = unsafe { core::slice::from_raw_parts(info.stages, info.stage_count as usize) };
    assert_eq!(stages.len(), 2, "a vertex and a fragment stage");
    let input = unsafe { &*info.vertex_input };
    assert_eq!(input.binding_count, 1, "one vertex buffer");
    let binding = unsafe { &*input.bindings };
    assert_eq!(binding.binding, 0);
    let attributes =
        unsafe { core::slice::from_raw_parts(input.attributes, input.attribute_count as usize) }
            .iter()
            .map(|a| (a.location, a.format, a.offset))
            .collect();
    let dynamic = unsafe { &*info.dynamic };
    let dynamic_states = unsafe {
        core::slice::from_raw_parts(dynamic.dynamic_states, dynamic.dynamic_state_count as usize)
    };
    assert!(
        dynamic_states.contains(&vd::DYNAMIC_VIEWPORT)
            && dynamic_states.contains(&vd::DYNAMIC_SCISSOR)
    );
    let viewport = unsafe { &*info.viewport };
    assert_eq!((viewport.viewport_count, viewport.scissor_count), (1, 1));
    assert_eq!(info.render_pass, 0, "dynamic rendering has no render pass");
    let rendering = unsafe { &*info.p_next.cast::<PipelineRendering>() };
    assert_eq!(rendering.s_type, vd::STYPE_PIPELINE_RENDERING);
    assert_eq!(rendering.color_attachment_count, 1);
    let color_format = unsafe { *rendering.color_attachment_formats };
    let depth = unsafe { &*info.depth_stencil };
    let blend = unsafe { &*(*info.color_blend).attachments };
    with(|script| {
        assert!(
            script.draw.pipeline_layouts.contains_key(&info.layout),
            "a pipeline layout that is not alive"
        );
        for stage in stages {
            assert!(
                script.draw.modules.contains_key(&stage.module),
                "a module that is not alive"
            );
        }
    });
    PipelineRecord {
        stages: stages.iter().map(|s| (s.stage, s.module)).collect(),
        entries: stages
            .iter()
            .map(|s| {
                unsafe { core::ffi::CStr::from_ptr(s.name) }
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        stride: binding.stride,
        attributes,
        color_format,
        depth_format: rendering.depth_attachment_format,
        depth_test: depth.depth_test_enable != 0,
        depth_write: depth.depth_write_enable != 0,
        blend: blend.blend_enable != 0,
        layout: info.layout,
    }
}

unsafe extern "system" fn destroy_pipeline(
    _device: DeviceHandle,
    pipeline: Handle,
    _allocator: *const c_void,
) {
    note("vkDestroyPipeline");
    super::present::release("pipeline", pipeline);
    with(|script| script.draw.pipelines.remove(&pipeline));
}

fn record(command: DrawCommand) {
    with(|script| script.draw.commands.push(command));
}

unsafe extern "system" fn cmd_bind_pipeline(
    _cmd: CommandBufferHandle,
    point: u32,
    pipeline: Handle,
) {
    note("vkCmdBindPipeline");
    assert_eq!(point, vd::BIND_POINT_GRAPHICS);
    with(|script| {
        assert!(
            script.draw.pipelines.contains_key(&pipeline),
            "a pipeline that is not alive"
        );
    });
    record(DrawCommand::BindPipeline(pipeline));
}

unsafe extern "system" fn cmd_bind_sets(
    _cmd: CommandBufferHandle,
    point: u32,
    layout: Handle,
    first: u32,
    count: u32,
    sets: *const Handle,
    dynamic_count: u32,
    _dynamic: *const u32,
) {
    note("vkCmdBindDescriptorSets");
    assert_eq!(point, vd::BIND_POINT_GRAPHICS);
    assert_eq!((count, dynamic_count), (2, 0));
    with(|script| assert!(script.draw.pipeline_layouts.contains_key(&layout)));
    let sets = unsafe { core::slice::from_raw_parts(sets, 2) };
    record(DrawCommand::BindSets {
        first,
        sets: [sets[0], sets[1]],
    });
}

unsafe extern "system" fn cmd_bind_vertex(
    _cmd: CommandBufferHandle,
    first: u32,
    count: u32,
    buffers: *const u64,
    offsets: *const u64,
) {
    note("vkCmdBindVertexBuffers");
    assert_eq!((first, count), (0, 1));
    record(DrawCommand::BindVertex {
        buffer: unsafe { *buffers },
        offset: unsafe { *offsets },
    });
}

unsafe extern "system" fn cmd_bind_index(
    _cmd: CommandBufferHandle,
    buffer: u64,
    offset: u64,
    index_type: u32,
) {
    note("vkCmdBindIndexBuffer");
    record(DrawCommand::BindIndex {
        buffer,
        offset,
        index_type,
    });
}

unsafe extern "system" fn cmd_viewport(
    _cmd: CommandBufferHandle,
    first: u32,
    count: u32,
    viewports: *const Viewport,
) {
    note("vkCmdSetViewport");
    assert_eq!((first, count), (0, 1));
    record(DrawCommand::Viewport(unsafe { *viewports }));
}

unsafe extern "system" fn cmd_scissor(
    _cmd: CommandBufferHandle,
    first: u32,
    count: u32,
    scissors: *const Rect2D,
) {
    note("vkCmdSetScissor");
    assert_eq!((first, count), (0, 1));
    record(DrawCommand::Scissor(unsafe { *scissors }));
}

unsafe extern "system" fn cmd_draw(
    _cmd: CommandBufferHandle,
    vertices: u32,
    instances: u32,
    first_vertex: u32,
    first_instance: u32,
) {
    note("vkCmdDraw");
    assert_eq!((first_vertex, first_instance), (0, 0));
    record(DrawCommand::Draw {
        vertices,
        instances,
    });
}

unsafe extern "system" fn cmd_draw_indexed(
    _cmd: CommandBufferHandle,
    indices: u32,
    instances: u32,
    first_index: u32,
    vertex_offset: i32,
    first_instance: u32,
) {
    note("vkCmdDrawIndexed");
    assert_eq!((first_index, vertex_offset, first_instance), (0, 0, 0));
    record(DrawCommand::DrawIndexed { indices, instances });
}
