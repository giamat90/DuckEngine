//! The render device against the fake driver: no GPU, no window. The fake checks that every
//! image barrier starts from the layout the image is in, so a wrong layout track fails here.

use super::*;
use crate::fake::draw::DrawCommand;
use crate::fake::{self, FakeGpu, Script};
use crate::vk_draw::Viewport;
use crate::vk_draw::{
    STAGE_EARLY_FRAGMENT_TESTS, STAGE_FRAGMENT_SHADER, STAGE_LATE_FRAGMENT_TESTS,
};
use crate::vk_present::{
    ERROR_SURFACE_LOST, LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC, LAYOUT_UNDEFINED,
    LOAD_OP_CLEAR, STAGE_COLOR_ATTACHMENT_OUTPUT,
};
use crate::vk_resource::{ASPECT_DEPTH, LAYOUT_DEPTH_ATTACHMENT, LAYOUT_SHADER_READ};
use render::desc::{
    AddressMode, BlendMode, BufferUsage, DepthState, Filter, IndexType, PassDesc, RenderTarget,
    ResourceState, ShaderStage, VertexAttribute, VertexFormat,
};

const TOKEN: NativeToken = NativeToken(0x5EED);
const DISCRETE: u32 = 2;

fn script() -> Script {
    Script::with_gpus(vec![FakeGpu::good("Fake GPU", DISCRETE)])
}

fn device() -> VulkanDevice {
    let context = fake::started(script());
    match VulkanDevice::with_context(context, TOKEN) {
        Ok(device) => device,
        Err(error) => panic!("expected a device, got: {error}"),
    }
}

fn surface_desc() -> SurfaceDesc {
    SurfaceDesc {
        native_token: TOKEN.0,
        width: 800,
        height: 600,
    }
}

fn surface(device: &mut VulkanDevice) -> SurfaceId {
    match device.create_surface(surface_desc()) {
        Ok(id) => id,
        Err(error) => panic!("expected a surface, got: {error:?}"),
    }
}

fn texture(device: &mut VulkanDevice, format: TextureFormat) -> TextureId {
    let desc = TextureDesc {
        width: 16,
        height: 16,
        mip_count: 1,
        format,
    };
    match device.create_texture(desc) {
        Ok(id) => id,
        Err(error) => panic!("expected a texture, got: {error:?}"),
    }
}

fn pass(target: RenderTarget, clear: Option<[f32; 4]>) -> PassDesc {
    PassDesc {
        target,
        clear_color: clear,
        clear_depth: None,
        depth: None,
    }
}

/// Runs `stream` in a frame of its own and returns the result of the submit.
fn run(
    device: &mut VulkanDevice,
    surface: SurfaceId,
    stream: &CommandStream,
) -> Result<SubmitStats, RenderError> {
    let frame = match device.begin_frame(surface) {
        Ok(frame) => frame,
        Err(error) => panic!("begin_frame failed: {error:?}"),
    };
    let result = device.submit(frame, stream);
    assert_eq!(device.end_frame(frame), Ok(()));
    result
}

fn clear_stream(target: RenderTarget, color: [f32; 4]) -> CommandStream {
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(target, Some(color)));
    stream.end_pass();
    stream
}

fn log_of(names: &[&str]) -> Vec<&'static str> {
    fake::log()
        .into_iter()
        .filter(|call| names.contains(call))
        .collect()
}

#[test]
fn the_null_token_names_no_window() {
    let result = VulkanDevice::create(NativeToken::NULL, VulkanOptions::default());
    assert!(matches!(result, Err(VulkanError::Unsupported(_))));
}

#[test]
fn the_device_reports_the_api_name_and_a_texture_size_the_port_allows() {
    let device = device();
    let caps = device.caps();
    assert_eq!(caps.device_name, "Vulkan 1.3");
    assert!(caps.depth_zero_to_one);
    assert!(caps.max_texture_size >= 1 && caps.max_texture_size <= 4096);
    assert_eq!(device.gpu_name(), "Fake GPU");
}

#[test]
fn a_surface_of_another_window_is_refused_and_leaves_no_trace() {
    let mut device = device();
    let other = SurfaceDesc {
        native_token: TOKEN.0 + 1,
        ..surface_desc()
    };
    assert_eq!(
        device.create_surface(other).map(|_| ()),
        Err(RenderError::Unsupported)
    );
    assert_eq!(fake::with(|s| s.swapchains_created), 0);
    // The bookkeeping side gave the slot back: a good surface still works.
    let id = surface(&mut device);
    device.destroy_surface(id);
}

#[test]
fn a_surface_makes_a_swapchain_and_destroying_it_takes_it_away() {
    let mut device = device();
    let id = surface(&mut device);
    assert_eq!(fake::with(|s| s.swapchains_created), 1);
    device.destroy_surface(id);
    let live = fake::live_objects();
    assert!(
        live.iter().all(|o| o.0 != "swapchain" && o.0 != "surface"),
        "{live:?}"
    );
    assert_eq!(
        device.begin_frame(id).map(|_| ()),
        Err(RenderError::InvalidHandle)
    );
}

#[test]
fn resizing_a_surface_rebuilds_its_swapchain() {
    let mut device = device();
    let id = surface(&mut device);
    assert_eq!(device.resize_surface(id, 640, 480), Ok(()));
    assert_eq!(fake::with(|s| s.swapchains_created), 2);
    assert_eq!(
        device.resize_surface(id, 0, 480),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn buffers_textures_and_samplers_are_made_written_and_destroyed() {
    let mut device = device();
    let Ok(buffer) = device.create_buffer(BufferDesc {
        size: 64,
        usage: BufferUsage::Vertex,
    }) else {
        panic!("a buffer should be made");
    };
    assert_eq!(device.write_buffer(buffer, 0, &[1; 64]), Ok(()));
    assert_eq!(
        device.write_buffer(buffer, 32, &[1; 64]),
        Err(RenderError::InvalidDescription)
    );
    let colour = texture(&mut device, TextureFormat::Rgba8);
    let area = Rect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };
    assert_eq!(
        device.write_texture(colour, area, &[7; 16 * 16 * 4]),
        Ok(())
    );
    let mut out = vec![0_u8; 16 * 16 * 4];
    assert_eq!(device.read_back(colour, &mut out), Ok(()));
    assert_eq!(
        device.read_back(colour, &mut out[..8]),
        Err(RenderError::InvalidDescription)
    );
    let Ok(sampler) = device.create_sampler(SamplerDesc {
        min_filter: Filter::Linear,
        mag_filter: Filter::Linear,
        address: AddressMode::Clamp,
    }) else {
        panic!("a sampler should be made");
    };
    device.destroy_buffer(buffer);
    device.destroy_texture(colour);
    device.destroy_sampler(sampler);
    assert_eq!(
        device.write_buffer(buffer, 0, &[1]),
        Err(RenderError::InvalidHandle)
    );
    assert_eq!(
        device.read_back(colour, &mut out),
        Err(RenderError::InvalidHandle)
    );
    drop(device);
    let left: Vec<_> = fake::live_objects().into_iter().map(|o| o.0).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_destroy_waits_for_the_device_before_it_frees() {
    let mut device = device();
    let colour = texture(&mut device, TextureFormat::Rgba8);
    fake::clear_log();
    device.destroy_texture(colour);
    let calls = log_of(&["vkDeviceWaitIdle", "vkDestroyImage"]);
    assert_eq!(calls, ["vkDeviceWaitIdle", "vkDestroyImage"]);
}

#[test]
fn a_clear_of_the_backbuffer_goes_from_undefined_to_colour_to_present() {
    let mut device = device();
    let id = surface(&mut device);
    fake::with(|s| s.image_barriers.clear());
    let stream = clear_stream(RenderTarget::Backbuffer, [0.0, 0.5, 1.0, 1.0]);
    assert_eq!(run(&mut device, id, &stream).map(|s| s.draws), Ok(0));
    let barriers = fake::with(|s| s.image_barriers.clone());
    let layouts: Vec<(u32, u32)> = barriers.iter().map(|b| (b.old, b.new)).collect();
    assert_eq!(
        layouts,
        [
            (LAYOUT_UNDEFINED, LAYOUT_COLOR_ATTACHMENT),
            (LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC),
        ]
    );
    assert_eq!(barriers[0].dst_stage, STAGE_COLOR_ATTACHMENT_OUTPUT);
    let rendering = fake::with(|s| s.renderings.clone());
    assert_eq!(rendering.len(), 1);
    assert_eq!(rendering[0].load_op, LOAD_OP_CLEAR);
    assert!(
        rendering[0]
            .clear
            .iter()
            .zip([0.0_f32, 0.5, 1.0, 1.0])
            .all(|(got, want)| (got - want).abs() < f32::EPSILON)
    );
    assert_eq!(rendering[0].extent, (800, 600));
    assert_eq!(fake::presents_so_far(), 1);
}

#[test]
fn a_frame_submits_once_and_presents_once_however_many_streams_it_holds() {
    let mut device = device();
    let id = surface(&mut device);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    let stream = clear_stream(RenderTarget::Backbuffer, [1.0; 4]);
    assert!(device.submit(frame, &stream).is_ok());
    assert!(device.submit(frame, &stream).is_ok());
    assert_eq!(device.end_frame(frame), Ok(()));
    assert_eq!(fake::with(|s| s.submits.len()), 1);
    assert_eq!(fake::presents_so_far(), 1);
    assert_eq!(fake::with(|s| s.renderings.len()), 2);
}

#[test]
fn a_pass_without_a_clear_loads_what_is_there() {
    let mut device = device();
    let id = surface(&mut device);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, None));
    stream.end_pass();
    assert!(run(&mut device, id, &stream).is_ok());
    let rendering = fake::with(|s| s.renderings.clone());
    assert_eq!(rendering[0].load_op, 0, "load, not clear");
}

#[test]
fn a_pass_into_a_texture_moves_it_to_the_attachment_layout_on_its_own() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    fake::with(|s| s.image_barriers.clear());
    let stream = clear_stream(RenderTarget::Texture(target), [1.0, 0.0, 0.0, 1.0]);
    assert!(run(&mut device, id, &stream).is_ok());
    let first = fake::with(|s| s.image_barriers[0]);
    assert_eq!(
        (first.old, first.new),
        (LAYOUT_UNDEFINED, LAYOUT_COLOR_ATTACHMENT)
    );
    // Drawn again: the barrier starts from the layout the first pass left it in.
    assert!(run(&mut device, id, &stream).is_ok());
    let layouts: Vec<(u32, u32)> = fake::with(|s| {
        s.image_barriers
            .iter()
            .filter(|b| b.image == first.image)
            .map(|b| (b.old, b.new))
            .collect()
    });
    assert_eq!(
        layouts,
        [
            (LAYOUT_UNDEFINED, LAYOUT_COLOR_ATTACHMENT),
            (LAYOUT_COLOR_ATTACHMENT, LAYOUT_COLOR_ATTACHMENT)
        ]
    );
    let mut out = vec![0_u8; 16 * 16 * 4];
    assert_eq!(device.read_back(target, &mut out), Ok(()));
}

#[test]
fn a_barrier_command_moves_a_texture_to_the_state_it_names() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Texture(target), Some([0.0; 4])));
    stream.end_pass();
    stream.barrier(
        RenderTarget::Texture(target),
        ResourceState::ColorTarget,
        ResourceState::ShaderRead,
    );
    fake::with(|s| s.image_barriers.clear());
    assert!(run(&mut device, id, &stream).is_ok());
    let last = fake::with(|s| {
        s.image_barriers
            .iter()
            .rfind(|b| b.new == LAYOUT_SHADER_READ)
            .copied()
    });
    let Some(last) = last else {
        panic!("a barrier to the shader read layout");
    };
    assert_eq!(
        (last.old, last.new, last.dst_stage),
        (
            LAYOUT_COLOR_ATTACHMENT,
            LAYOUT_SHADER_READ,
            STAGE_FRAGMENT_SHADER
        )
    );
}

#[test]
fn a_texture_cannot_be_presented() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    let mut stream = CommandStream::new();
    stream.barrier(
        RenderTarget::Texture(target),
        ResourceState::Undefined,
        ResourceState::Present,
    );
    assert_eq!(
        run(&mut device, id, &stream).map(|_| ()),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn a_depth_pass_attaches_the_depth_texture_and_clears_it() {
    let mut device = device();
    let id = surface(&mut device);
    let colour = texture(&mut device, TextureFormat::Rgba8);
    let depth = texture(&mut device, TextureFormat::Depth32F);
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(colour),
        clear_color: Some([0.0, 0.0, 0.0, 1.0]),
        clear_depth: Some(0.25),
        depth: Some(depth),
    });
    stream.end_pass();
    fake::with(|s| s.image_barriers.clear());
    assert!(run(&mut device, id, &stream).is_ok());
    let rendering = fake::with(|s| s.renderings[0].clone());
    let Some(attachment) = rendering.depth else {
        panic!("a depth attachment");
    };
    assert_eq!(attachment.layout, LAYOUT_DEPTH_ATTACHMENT);
    assert_eq!(attachment.load_op, LOAD_OP_CLEAR);
    assert!((attachment.clear - 0.25).abs() < f32::EPSILON);
    let barrier = fake::with(|s| {
        s.image_barriers
            .iter()
            .find(|b| b.aspect == ASPECT_DEPTH)
            .copied()
    });
    let Some(barrier) = barrier else {
        panic!("a barrier on the depth aspect");
    };
    assert_eq!(
        (barrier.old, barrier.new, barrier.dst_stage),
        (
            LAYOUT_UNDEFINED,
            LAYOUT_DEPTH_ATTACHMENT,
            STAGE_EARLY_FRAGMENT_TESTS | STAGE_LATE_FRAGMENT_TESTS
        )
    );
}

#[test]
fn a_depth_texture_smaller_than_the_colour_target_is_refused() {
    let mut device = device();
    let id = surface(&mut device);
    let colour = texture(&mut device, TextureFormat::Rgba8);
    let Ok(small) = device.create_texture(TextureDesc {
        width: 8,
        height: 8,
        mip_count: 1,
        format: TextureFormat::Depth32F,
    }) else {
        panic!("a depth texture");
    };
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(colour),
        clear_color: None,
        clear_depth: Some(1.0),
        depth: Some(small),
    });
    stream.end_pass();
    let result = run(&mut device, id, &stream);
    assert_eq!(result.map(|_| ()), Err(RenderError::InvalidDescription));
    assert!(fake::with(|s| s.renderings.is_empty()));
}

#[test]
fn a_stream_the_bookkeeping_side_refuses_records_nothing() {
    let mut device = device();
    let id = surface(&mut device);
    let mut unclosed = CommandStream::new();
    unclosed.begin_pass(pass(RenderTarget::Backbuffer, Some([1.0; 4])));
    fake::with(|s| s.renderings.clear());
    assert_eq!(
        run(&mut device, id, &unclosed).map(|_| ()),
        Err(RenderError::InvalidDescription)
    );
    assert!(fake::with(|s| s.renderings.is_empty()));
}

#[test]
fn a_window_without_area_gives_a_frame_that_executes_nothing() {
    let mut device = device();
    let id = surface(&mut device);
    fake::with(|s| {
        s.surface_caps.current_extent = crate::vk_present::Extent2D {
            width: 0,
            height: 0,
        };
    });
    assert_eq!(device.resize_surface(id, 800, 600), Ok(()));
    fake::with(|s| s.renderings.clear());
    let stream = clear_stream(RenderTarget::Backbuffer, [1.0; 4]);
    assert!(run(&mut device, id, &stream).is_ok());
    assert!(fake::with(|s| s.renderings.is_empty()));
    assert_eq!(fake::presents_so_far(), 0);
}

#[test]
fn one_frame_at_a_time_and_a_frame_ends_once() {
    let mut device = device();
    let id = surface(&mut device);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    assert!(device.begin_frame(id).is_err());
    assert_eq!(device.end_frame(frame), Ok(()));
    assert_eq!(device.end_frame(frame), Err(RenderError::InvalidHandle));
    assert_eq!(
        device.submit(frame, &CommandStream::new()).map(|_| ()),
        Err(RenderError::InvalidHandle)
    );
}

#[test]
fn a_lost_surface_at_the_acquire_is_reported_and_the_frame_does_not_stay_open() {
    let mut device = device();
    let id = surface(&mut device);
    fake::with(|s| s.acquire_results.push_back(ERROR_SURFACE_LOST));
    assert_eq!(
        device.begin_frame(id).map(|_| ()),
        Err(RenderError::SurfaceLost)
    );
    assert!(device.begin_frame(id).is_ok());
}

#[test]
fn a_failed_submit_at_the_end_of_a_frame_is_a_port_error() {
    let mut device = device();
    let id = surface(&mut device);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    fake::with(|s| s.submit_result = -1);
    assert_eq!(device.end_frame(frame), Err(RenderError::OutOfMemory));
}

#[test]
fn everything_the_device_made_is_freed_with_it() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    let stream = clear_stream(RenderTarget::Texture(target), [1.0; 4]);
    assert!(run(&mut device, id, &stream).is_ok());
    drop(device);
    let left: Vec<_> = fake::live_objects().into_iter().map(|o| o.0).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_swapchain_that_is_out_of_date_at_the_acquire_is_rebuilt_and_the_frame_still_draws() {
    let mut device = device();
    let id = surface(&mut device);
    fake::with(|s| {
        s.acquire_results
            .push_back(crate::vk_present::ERROR_OUT_OF_DATE);
    });
    let stream = clear_stream(RenderTarget::Backbuffer, [1.0; 4]);
    assert!(run(&mut device, id, &stream).is_ok());
    assert_eq!(fake::with(|s| s.swapchains_created), 2);
    assert_eq!(fake::with(|s| s.renderings.len()), 1);
    assert_eq!(fake::presents_so_far(), 1);
}

#[test]
fn a_texture_destroyed_in_a_frame_that_recorded_it_is_freed_after_the_frame_is_on_the_queue() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    let stream = clear_stream(RenderTarget::Texture(target), [1.0; 4]);
    assert!(device.submit(frame, &stream).is_ok());
    fake::clear_log();
    device.destroy_texture(target);
    assert!(log_of(&["vkDestroyImage"]).is_empty(), "freed too early");
    assert_eq!(device.end_frame(frame), Ok(()));
    let calls = log_of(&["vkQueueSubmit2", "vkDeviceWaitIdle", "vkDestroyImage"]);
    assert_eq!(calls.first(), Some(&"vkQueueSubmit2"));
    assert_eq!(calls.last(), Some(&"vkDestroyImage"));
}

#[test]
fn texture_writes_and_read_backs_wait_for_a_frame_with_recorded_commands() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    let stream = clear_stream(RenderTarget::Texture(target), [1.0; 4]);
    assert!(device.submit(frame, &stream).is_ok());
    let area = Rect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    };
    assert_eq!(
        device.write_texture(target, area, &[0; 4]),
        Err(RenderError::Unsupported)
    );
    let mut out = vec![0_u8; 16 * 16 * 4];
    assert_eq!(
        device.read_back(target, &mut out),
        Err(RenderError::Unsupported)
    );
    assert_eq!(device.end_frame(frame), Ok(()));
    assert_eq!(device.read_back(target, &mut out), Ok(()));
}

#[test]
fn a_failed_submit_drops_the_presenter_so_the_next_frame_cannot_wait_for_ever() {
    let mut device = device();
    let id = surface(&mut device);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    fake::with(|s| s.submit_result = -1);
    assert_eq!(device.end_frame(frame), Err(RenderError::OutOfMemory));
    assert_eq!(
        device.begin_frame(id).map(|_| ()),
        Err(RenderError::SurfaceLost)
    );
}

#[test]
fn the_backbuffer_and_depth_textures_cannot_be_read_by_a_shader() {
    let mut device = device();
    let id = surface(&mut device);
    let depth = texture(&mut device, TextureFormat::Depth32F);
    for target in [RenderTarget::Backbuffer, RenderTarget::Texture(depth)] {
        let mut stream = CommandStream::new();
        stream.barrier(target, ResourceState::Undefined, ResourceState::ShaderRead);
        assert_eq!(
            run(&mut device, id, &stream).map(|_| ()),
            Err(RenderError::Unsupported)
        );
    }
}

#[test]
fn a_depth_texture_cannot_be_the_colour_target_of_a_pass() {
    let mut device = device();
    let id = surface(&mut device);
    let depth = texture(&mut device, TextureFormat::Depth32F);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Texture(depth), None));
    stream.end_pass();
    assert_eq!(
        run(&mut device, id, &stream).map(|_| ()),
        Err(RenderError::Unsupported)
    );
}

// ----- shaders, pipelines, binds and draws (card 6-14) -----

/// A SPIR-V module with a header and nothing else: it passes the adapter's checks.
fn spirv_bytes() -> Vec<u8> {
    [0x0723_0203_u32, 0x0001_0600, 0, 1, 0]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect()
}

fn shader(device: &mut VulkanDevice, stage: ShaderStage) -> ShaderId {
    let code = spirv_bytes();
    let desc = ShaderDesc {
        stage,
        code: &code,
        entry: "main",
    };
    match device.create_shader(desc) {
        Ok(id) => id,
        Err(error) => panic!("expected a shader, got: {error:?}"),
    }
}

fn pipeline(device: &mut VulkanDevice) -> PipelineId {
    let vertex_shader = shader(device, ShaderStage::Vertex);
    let fragment_shader = shader(device, ShaderStage::Fragment);
    let attributes = [VertexAttribute {
        location: 0,
        format: VertexFormat::Float2,
        offset: 0,
    }];
    let desc = PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: 8,
        attributes: &attributes,
        blend: BlendMode::Alpha,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: None,
    };
    match device.create_pipeline(desc) {
        Ok(id) => id,
        Err(error) => panic!("expected a pipeline, got: {error:?}"),
    }
}

fn buffer(device: &mut VulkanDevice, usage: BufferUsage) -> BufferId {
    match device.create_buffer(BufferDesc { size: 64, usage }) {
        Ok(id) => id,
        Err(error) => panic!("expected a buffer, got: {error:?}"),
    }
}

fn sampler(device: &mut VulkanDevice) -> SamplerId {
    let desc = SamplerDesc {
        min_filter: Filter::Linear,
        mag_filter: Filter::Linear,
        address: AddressMode::Clamp,
    };
    match device.create_sampler(desc) {
        Ok(id) => id,
        Err(error) => panic!("expected a sampler, got: {error:?}"),
    }
}

/// A stream that draws three vertices to the backbuffer with `pipeline`.
fn triangle(pipeline: PipelineId, vertices: BufferId) -> CommandStream {
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, Some([0.0; 4])));
    stream.set_pipeline(pipeline);
    stream.bind_vertex_buffer(0, vertices, 16);
    stream.draw(3, 1);
    stream.end_pass();
    stream
}

#[test]
fn a_shader_that_is_not_spirv_or_has_another_entry_is_refused_and_leaves_no_handle() {
    let mut device = device();
    let bad = [
        (vec![0_u8; 8], "main"),
        (spirv_bytes()[..7].to_vec(), "main"),
        (spirv_bytes(), "other"),
    ];
    for (code, entry) in &bad {
        let desc = ShaderDesc {
            stage: ShaderStage::Vertex,
            code,
            entry,
        };
        assert_eq!(
            device.create_shader(desc).map(|_| ()),
            Err(RenderError::InvalidDescription),
            "{entry} {code:?}"
        );
    }
    assert_eq!(fake::with(|s| s.draw.modules.len()), 0);
    let good = shader(&mut device, ShaderStage::Vertex);
    device.destroy_shader(good);
    assert_eq!(fake::with(|s| s.draw.modules.len()), 0);
}

#[test]
fn a_storage_buffer_in_a_shader_is_unsupported() {
    let mut device = device();
    // OpVariable (id 5, class StorageBuffer) and OpDecorate DescriptorSet on it.
    let words = [
        0x0723_0203_u32,
        0x0001_0600,
        0,
        6,
        0,
        0x0004_003B,
        1,
        5,
        12,
        0x0004_0047,
        5,
        34,
        0,
    ];
    let code: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let desc = ShaderDesc {
        stage: ShaderStage::Fragment,
        code: &code,
        entry: "main",
    };
    assert_eq!(
        device.create_shader(desc).map(|_| ()),
        Err(RenderError::Unsupported)
    );
}

#[test]
fn a_pipeline_is_made_for_each_target_format_and_destroyed_with_its_handle() {
    let mut device = device();
    let id = pipeline(&mut device);
    let formats = fake::with(|s| {
        let mut formats: Vec<u32> = s.draw.pipelines.values().map(|p| p.color_format).collect();
        formats.sort_unstable();
        formats
    });
    assert_eq!(formats, [37, 44]);
    device.destroy_pipeline(id);
    assert_eq!(fake::with(|s| s.draw.pipelines.len()), 0);
}

#[test]
fn a_refused_pipeline_leaves_nothing_behind_and_the_handle_goes_back() {
    let mut device = device();
    let vertex_shader = shader(&mut device, ShaderStage::Vertex);
    let fragment_shader = shader(&mut device, ShaderStage::Fragment);
    let attributes = [VertexAttribute {
        location: 0,
        format: VertexFormat::Float2,
        offset: 0,
    }];
    let desc = PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: 8,
        attributes: &attributes,
        blend: BlendMode::Opaque,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: None,
    };
    fake::with(|s| s.fail_call = Some("vkCreateGraphicsPipelines"));
    assert_eq!(
        device.create_pipeline(desc).map(|_| ()),
        Err(RenderError::OutOfMemory)
    );
    assert_eq!(fake::with(|s| s.draw.pipelines.len()), 0);
    fake::with(|s| s.fail_call = None);
    assert!(device.create_pipeline(desc).is_ok());
}

#[test]
fn a_draw_binds_the_variant_for_the_target_sets_the_whole_target_and_draws() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    assert_eq!(
        run(&mut device, id, &triangle(pipe, vertices)).map(|_| ()),
        Ok(())
    );
    let commands = fake::with(|s| s.draw.commands.clone());
    let bgra = fake::with(|s| {
        s.draw
            .pipelines
            .iter()
            .find(|(_, p)| p.color_format == 44)
            .map(|(handle, _)| *handle)
    });
    let Some(bgra) = bgra else {
        panic!("a BGRA variant");
    };
    let whole = Viewport {
        x: 0.0,
        y: 600.0,
        width: 800.0,
        height: -600.0,
        min_depth: 0.0,
        max_depth: 1.0,
    };
    assert_eq!(commands[0], DrawCommand::Viewport(whole));
    assert!(matches!(commands[1], DrawCommand::Scissor(_)));
    assert_eq!(commands[2], DrawCommand::BindPipeline(bgra));
    assert!(matches!(
        commands[3],
        DrawCommand::BindVertex { offset: 16, .. }
    ));
    assert!(matches!(
        commands[4],
        DrawCommand::BindSets { first: 0, .. }
    ));
    assert_eq!(
        commands[5],
        DrawCommand::Draw {
            vertices: 3,
            instances: 1
        }
    );
}

#[test]
fn the_sets_are_made_again_only_when_a_bind_changed_them() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    let uniforms = buffer(&mut device, BufferUsage::Uniform);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, None));
    stream.set_pipeline(pipe);
    stream.bind_vertex_buffer(0, vertices, 0);
    stream.bind_uniforms(0, uniforms);
    stream.draw(3, 1);
    stream.draw(3, 1);
    stream.bind_uniforms(1, uniforms);
    stream.draw(3, 1);
    stream.end_pass();
    assert!(run(&mut device, id, &stream).is_ok());
    let sets = fake::with(|s| {
        s.draw
            .commands
            .iter()
            .filter(|c| matches!(c, DrawCommand::BindSets { .. }))
            .count()
    });
    assert_eq!(sets, 2);
    let written = fake::with(|s| s.draw.writes.iter().map(|w| w.binding).collect::<Vec<_>>());
    assert_eq!(written, [0, 0, 1]);
}

#[test]
fn a_viewport_command_is_flipped_for_vulkan() {
    let mut device = device();
    let id = surface(&mut device);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, None));
    stream.set_viewport(Rect {
        x: 10,
        y: 20,
        width: 100,
        height: 50,
    });
    stream.end_pass();
    assert!(run(&mut device, id, &stream).is_ok());
    let viewports: Vec<Viewport> = fake::with(|s| {
        s.draw
            .commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::Viewport(v) => Some(*v),
                _ => None,
            })
            .collect()
    });
    assert_eq!(viewports.len(), 2);
    assert_eq!(
        (viewports[1].x, viewports[1].y, viewports[1].height),
        (10.0, 580.0, -50.0)
    );
}

#[test]
fn an_index_buffer_is_bound_with_its_width() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    let indices = buffer(&mut device, BufferUsage::Index);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, None));
    stream.set_pipeline(pipe);
    stream.bind_vertex_buffer(0, vertices, 0);
    stream.bind_index_buffer(indices, 4, IndexType::U32);
    stream.draw_indexed(6, 2);
    stream.end_pass();
    assert!(run(&mut device, id, &stream).is_ok());
    let commands = fake::with(|s| s.draw.commands.clone());
    assert!(commands.iter().any(|c| matches!(
        c,
        DrawCommand::BindIndex {
            offset: 4,
            index_type: 1,
            ..
        }
    )));
    assert!(commands.contains(&DrawCommand::DrawIndexed {
        indices: 6,
        instances: 2
    }));
}

#[test]
fn a_texture_bound_for_a_draw_must_be_readable_by_a_shader_already() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    let tex = texture(&mut device, TextureFormat::Rgba8);
    let samp = sampler(&mut device);
    let draw_with = |stream: &mut CommandStream| {
        stream.begin_pass(pass(RenderTarget::Backbuffer, None));
        stream.set_pipeline(pipe);
        stream.bind_vertex_buffer(0, vertices, 0);
        stream.bind_texture(0, tex, samp);
        stream.draw(3, 1);
        stream.end_pass();
    };
    // A written texture is readable by a shader without any barrier.
    let area = Rect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };
    assert_eq!(device.write_texture(tex, area, &[0; 16 * 16 * 4]), Ok(()));
    let mut ready = CommandStream::new();
    draw_with(&mut ready);
    assert!(run(&mut device, id, &ready).is_ok());
    let writes = fake::with(|s| s.draw.writes.clone());
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].layout, LAYOUT_SHADER_READ);
}

#[test]
fn a_texture_in_another_layout_is_refused_and_the_rendering_scope_is_closed() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    let tex = texture(&mut device, TextureFormat::Rgba8);
    let samp = sampler(&mut device);
    // The texture is a colour target now, so a shader cannot read it.
    let mut target = CommandStream::new();
    target.begin_pass(pass(RenderTarget::Texture(tex), Some([1.0; 4])));
    target.end_pass();
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, None));
    stream.set_pipeline(pipe);
    stream.bind_vertex_buffer(0, vertices, 0);
    stream.bind_texture(0, tex, samp);
    stream.draw(3, 1);
    stream.end_pass();
    assert!(run(&mut device, id, &target).is_ok());
    fake::clear_log();
    assert_eq!(
        run(&mut device, id, &stream).map(|_| ()),
        Err(RenderError::InvalidDescription)
    );
    // The scope opened by the failed stream was closed, so the buffer could be ended.
    assert_eq!(
        log_of(&[
            "vkCmdBeginRendering",
            "vkCmdEndRendering",
            "vkEndCommandBuffer"
        ]),
        [
            "vkCmdBeginRendering",
            "vkCmdEndRendering",
            "vkEndCommandBuffer"
        ]
    );
    assert!(
        run(
            &mut device,
            id,
            &clear_stream(RenderTarget::Backbuffer, [0.0; 4])
        )
        .is_ok()
    );
}

#[test]
fn a_frame_resets_the_pool_of_its_slot_and_a_full_pool_is_out_of_memory() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    let uniforms = buffer(&mut device, BufferUsage::Uniform);
    assert!(
        run(
            &mut device,
            id,
            &clear_stream(RenderTarget::Backbuffer, [0.0; 4])
        )
        .is_ok()
    );
    assert_eq!(fake::with(|s| s.draw.pool_resets), 1);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass(RenderTarget::Backbuffer, None));
    stream.set_pipeline(pipe);
    stream.bind_vertex_buffer(0, vertices, 0);
    for _ in 0..1030 {
        stream.bind_uniforms(0, uniforms);
        stream.draw(3, 1);
    }
    stream.end_pass();
    assert_eq!(
        run(&mut device, id, &stream).map(|_| ()),
        Err(RenderError::OutOfMemory)
    );
    // The next frame on the same slot has its pool back.
    let calm = triangle(pipe, vertices);
    assert!(run(&mut device, id, &calm).is_ok());
    assert!(run(&mut device, id, &calm).is_ok());
}

#[test]
fn a_pipeline_destroyed_in_a_frame_that_recorded_it_is_freed_after_the_frame_is_on_the_queue() {
    let mut device = device();
    let id = surface(&mut device);
    let pipe = pipeline(&mut device);
    let vertices = buffer(&mut device, BufferUsage::Vertex);
    let Ok(frame) = device.begin_frame(id) else {
        panic!("a frame");
    };
    assert!(device.submit(frame, &triangle(pipe, vertices)).is_ok());
    fake::clear_log();
    device.destroy_pipeline(pipe);
    assert!(log_of(&["vkDestroyPipeline"]).is_empty(), "freed too early");
    assert_eq!(device.end_frame(frame), Ok(()));
    let calls = log_of(&["vkQueueSubmit2", "vkDestroyPipeline"]);
    assert_eq!(calls.first(), Some(&"vkQueueSubmit2"));
    assert_eq!(
        calls.iter().filter(|c| **c == "vkDestroyPipeline").count(),
        2
    );
}

#[test]
fn the_pools_of_a_surface_go_with_it_and_everything_is_freed_with_the_device() {
    let mut device = device();
    let id = surface(&mut device);
    assert_eq!(fake::with(|s| s.draw.pools.len()), 2);
    device.destroy_surface(id);
    assert_eq!(fake::with(|s| s.draw.pools.len()), 0);
    let again = surface(&mut device);
    let pipe = pipeline(&mut device);
    let _ = (again, pipe);
    drop(device);
    let left: Vec<_> = fake::live_objects().into_iter().map(|o| o.0).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_pass_into_a_texture_is_not_flipped_and_one_into_the_backbuffer_is() {
    let mut device = device();
    let id = surface(&mut device);
    let target = texture(&mut device, TextureFormat::Rgba8);
    for (render_target, expected_y, expected_height) in [
        (RenderTarget::Texture(target), 0.0, 16.0),
        (RenderTarget::Backbuffer, 600.0, -600.0),
    ] {
        fake::with(|s| s.draw.commands.clear());
        let mut stream = CommandStream::new();
        stream.begin_pass(pass(render_target, None));
        stream.end_pass();
        assert!(run(&mut device, id, &stream).is_ok());
        let first = fake::with(|s| s.draw.commands.first().copied());
        let Some(DrawCommand::Viewport(viewport)) = first else {
            panic!("a pass sets its viewport first");
        };
        assert_eq!((viewport.y, viewport.height), (expected_y, expected_height));
    }
}
