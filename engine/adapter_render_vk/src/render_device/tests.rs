//! The render device against the fake driver: no GPU, no window. The fake checks that every
//! image barrier starts from the layout the image is in, so a wrong layout track fails here.

use super::*;
use crate::fake::{self, FakeGpu, Script};
use crate::vk_draw::{
    STAGE_EARLY_FRAGMENT_TESTS, STAGE_FRAGMENT_SHADER, STAGE_LATE_FRAGMENT_TESTS,
};
use crate::vk_present::{
    ERROR_SURFACE_LOST, LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC, LAYOUT_UNDEFINED,
    LOAD_OP_CLEAR, STAGE_COLOR_ATTACHMENT_OUTPUT,
};
use crate::vk_resource::{ASPECT_DEPTH, LAYOUT_DEPTH_ATTACHMENT, LAYOUT_SHADER_READ};
use render::desc::{
    AddressMode, BufferUsage, Filter, PassDesc, RenderTarget, ResourceState, ShaderStage,
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
fn shaders_and_pipelines_are_not_there_yet() {
    let mut device = device();
    let shader = ShaderDesc {
        stage: ShaderStage::Vertex,
        code: &[0; 8],
        entry: "main",
    };
    assert_eq!(
        device.create_shader(shader).map(|_| ()),
        Err(RenderError::Unsupported)
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
