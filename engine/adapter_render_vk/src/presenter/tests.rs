//! The presenter against the fake driver: no GPU, no window. The fake records every call and
//! every live object, so order, pairing and clean-up can be asserted.

use super::*;
use crate::fake::{self, FakeGpu, Script};
use crate::vk_present::{
    ERROR_OUT_OF_DATE, ERROR_SURFACE_LOST, FORMAT_B8G8R8A8_UNORM, IMAGE_USAGE_COLOR_ATTACHMENT,
    IMAGE_USAGE_TRANSFER_SRC, LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC, LAYOUT_TRANSFER_SRC,
    LAYOUT_UNDEFINED, PRESENT_MODE_FIFO, STAGE_COLOR_ATTACHMENT_OUTPUT, SUBOPTIMAL,
};
use platform::window::NativeToken;

const TOKEN: NativeToken = NativeToken(0x5EED);
const DISCRETE: u32 = 2;

fn script() -> Script {
    Script::with_gpus(vec![FakeGpu::good("Fake GPU", DISCRETE)])
}

fn presenter_of(script: Script) -> (VulkanContext, Presenter) {
    let context = fake::started(script);
    let presenter = match context.create_presenter(TOKEN, 800, 600) {
        Ok(presenter) => presenter,
        Err(error) => panic!("expected a presenter, got: {error}"),
    };
    (context, presenter)
}

fn presenter() -> (VulkanContext, Presenter) {
    presenter_of(script())
}

fn refused(script: Script) -> VulkanError {
    let context = fake::started(script);
    match context.create_presenter(TOKEN, 800, 600) {
        Err(error) => error,
        Ok(_) => panic!("expected an error, got a presenter"),
    }
}

fn frame(presenter: &mut Presenter) -> FrameOutcome {
    match presenter.present_clear([0.0, 0.5, 1.0, 1.0]) {
        Ok(outcome) => outcome,
        Err(error) => panic!("expected a frame, got: {error}"),
    }
}

fn only(calls: &[&'static str], names: &[&str]) -> Vec<&'static str> {
    calls
        .iter()
        .copied()
        .filter(|call| names.contains(call))
        .collect()
}

#[test]
fn a_presenter_makes_a_bgra8_fifo_swapchain_sized_by_the_surface() {
    let (_context, presenter) = presenter();
    let request = fake::with(|s| s.swapchain_request.clone());
    let Some(request) = request else {
        panic!("no swapchain was asked for");
    };
    assert_eq!(request.format, FORMAT_B8G8R8A8_UNORM);
    assert_eq!(request.present_mode, PRESENT_MODE_FIFO);
    assert_eq!(request.min_image_count, 3);
    assert_eq!(
        request.usage,
        IMAGE_USAGE_COLOR_ATTACHMENT | IMAGE_USAGE_TRANSFER_SRC
    );
    assert_eq!((request.width, request.height), (800, 600));
    assert_eq!(request.old_swapchain, 0);
    assert_eq!(presenter.extent(), (800, 600));
    assert_eq!(presenter.image_count(), 3);
}

#[test]
fn the_queue_family_is_asked_whether_it_can_present_to_the_surface() {
    let (_context, _presenter) = presenter();
    assert_eq!(fake::with(|s| s.support_queries.clone()), [0]);
}

#[test]
fn a_null_window_token_is_unsupported() {
    let context = fake::started(script());
    let Err(error) = context.create_presenter(NativeToken::NULL, 800, 600) else {
        panic!("a null token names no window");
    };
    assert!(matches!(error, VulkanError::Unsupported(_)));
    assert!(fake::live_objects().is_empty());
}

#[test]
fn a_system_without_surface_extensions_cannot_present() {
    let mut system = script();
    system.instance_extensions.clear();
    assert!(matches!(refused(system), VulkanError::Unsupported(_)));
}

#[test]
fn a_device_without_the_swapchain_extension_cannot_present() {
    let mut gpu = FakeGpu::good("No swapchain", DISCRETE);
    gpu.swapchain_extension = false;
    let error = refused(Script::with_gpus(vec![gpu]));
    assert!(matches!(error, VulkanError::Unsupported(_)));
}

#[test]
fn a_queue_family_that_cannot_present_is_refused_and_the_surface_is_destroyed() {
    let mut system = script();
    system.present_support = false;
    let context = fake::started(system);
    let Err(error) = context.create_presenter(TOKEN, 800, 600) else {
        panic!("the family cannot present");
    };
    assert!(matches!(error, VulkanError::Unsupported(_)));
    assert!(
        fake::live_objects().is_empty(),
        "{:?}",
        fake::live_objects()
    );
}

#[test]
fn a_surface_without_bgra8_unorm_is_refused_and_the_surface_is_destroyed() {
    let mut system = script();
    system.surface_formats.clear();
    let context = fake::started(system);
    let Err(error) = context.create_presenter(TOKEN, 800, 600) else {
        panic!("no usable format");
    };
    assert!(matches!(error, VulkanError::Unsupported(_)));
    assert!(fake::live_objects().iter().all(|o| o.0 != "surface"));
}

#[test]
fn a_presenter_frees_everything_it_made() {
    let (context, mut presenter) = presenter();
    frame(&mut presenter);
    frame(&mut presenter);
    drop(presenter);
    let left: Vec<_> = fake::live_objects().into_iter().map(|o| o.0).collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    drop(context);
}

#[test]
fn a_failed_swapchain_leaves_nothing_behind() {
    let mut system = script();
    system.create_swapchain_result = -4;
    let context = fake::started(system);
    let Err(error) = context.create_presenter(TOKEN, 800, 600) else {
        panic!("the driver refuses");
    };
    assert_eq!(
        error,
        VulkanError::Call {
            call: "vkCreateSwapchainKHR",
            result: -4
        }
    );
    assert!(fake::live_objects().is_empty());
}

#[test]
fn a_frame_waits_acquires_records_submits_and_presents_in_that_order() {
    let (_context, mut presenter) = presenter();
    fake::clear_log();
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
    let calls = fake::log();
    assert_eq!(
        only(
            &calls,
            &[
                "vkWaitForFences",
                "vkAcquireNextImageKHR",
                "vkResetFences",
                "vkResetCommandPool",
                "vkBeginCommandBuffer",
                "vkCmdPipelineBarrier2",
                "vkCmdBeginRendering",
                "vkCmdEndRendering",
                "vkEndCommandBuffer",
                "vkQueueSubmit2",
                "vkQueuePresentKHR",
            ]
        ),
        [
            "vkWaitForFences",
            "vkAcquireNextImageKHR",
            "vkResetCommandPool",
            "vkBeginCommandBuffer",
            "vkCmdPipelineBarrier2",
            "vkCmdBeginRendering",
            "vkCmdEndRendering",
            "vkCmdPipelineBarrier2",
            "vkEndCommandBuffer",
            "vkResetFences",
            "vkQueueSubmit2",
            "vkQueuePresentKHR",
        ]
    );
}

#[test]
fn the_image_goes_from_undefined_to_attachment_to_present() {
    let (_context, mut presenter) = presenter();
    frame(&mut presenter);
    assert_eq!(
        fake::with(|s| s.barriers.clone()),
        [
            (LAYOUT_UNDEFINED, LAYOUT_COLOR_ATTACHMENT),
            (LAYOUT_COLOR_ATTACHMENT, LAYOUT_PRESENT_SRC)
        ]
    );
}

#[test]
fn the_clear_colour_reaches_the_render_pass() {
    let (_context, mut presenter) = presenter();
    frame(&mut presenter);
    assert_eq!(fake::with(|s| s.clears.clone()), [[0.0, 0.5, 1.0, 1.0]]);
}

#[test]
fn the_submit_waits_for_the_acquire_at_the_colour_output_stage_and_the_present_waits_for_the_submit()
 {
    let (_context, mut presenter) = presenter();
    frame(&mut presenter);
    let (submits, presents) = fake::with(|s| (s.submits.clone(), s.presents.clone()));
    assert_eq!(submits.len(), 1);
    assert_eq!(presents.len(), 1);
    assert_ne!(submits[0].wait_semaphore, 0);
    assert_eq!(submits[0].wait_stage, STAGE_COLOR_ATTACHMENT_OUTPUT);
    assert_ne!(submits[0].signal_semaphore, 0);
    assert_ne!(submits[0].fence, 0);
    assert_eq!(presents[0].wait_semaphore, submits[0].signal_semaphore);
}

#[test]
fn two_frames_in_flight_use_two_acquire_semaphores_and_two_fences() {
    let (_context, mut presenter) = presenter();
    for _ in 0..4 {
        frame(&mut presenter);
    }
    let submits = fake::with(|s| s.submits.clone());
    assert_eq!(submits.len(), 4);
    assert_eq!(submits[0].wait_semaphore, submits[2].wait_semaphore);
    assert_eq!(submits[1].wait_semaphore, submits[3].wait_semaphore);
    assert_ne!(submits[0].wait_semaphore, submits[1].wait_semaphore);
    assert_eq!(submits[0].fence, submits[2].fence);
    assert_ne!(submits[0].fence, submits[1].fence);
}

#[test]
fn every_swapchain_image_has_its_own_render_finished_semaphore() {
    let (_context, mut presenter) = presenter();
    for _ in 0..6 {
        frame(&mut presenter);
    }
    let (submits, presents) = fake::with(|s| (s.submits.clone(), s.presents.clone()));
    let mut by_image = std::collections::BTreeMap::new();
    for (submit, present) in submits.iter().zip(&presents) {
        let known = by_image
            .entry(present.image)
            .or_insert(submit.signal_semaphore);
        assert_eq!(*known, submit.signal_semaphore, "one semaphore per image");
    }
    let distinct: std::collections::BTreeSet<_> = by_image.values().collect();
    assert_eq!(
        distinct.len(),
        by_image.len(),
        "no semaphore shared by two images"
    );
    assert_eq!(by_image.len(), 3);
}

#[test]
fn an_out_of_date_acquire_recreates_the_swapchain_and_draws_nothing() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| s.acquire_results.push_back(ERROR_OUT_OF_DATE));
    fake::clear_log();
    assert_eq!(frame(&mut presenter), FrameOutcome::Recreated);
    let calls = fake::log();
    assert!(
        !calls.contains(&"vkResetFences"),
        "the fence must stay signalled"
    );
    assert!(!calls.contains(&"vkQueueSubmit2"));
    assert_eq!(only(&calls, &["vkCreateSwapchainKHR"]).len(), 1);
    assert_eq!(only(&calls, &["vkDestroySwapchainKHR"]).len(), 1);
    let request = fake::with(|s| s.swapchain_request.clone());
    assert_ne!(
        request.map(|r| r.old_swapchain),
        Some(0),
        "the old one is handed over"
    );
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
}

fn the_window_is_resized_to(width: u32, height: u32) {
    fake::with(|s| {
        s.surface_caps.current_extent = crate::vk_present::Extent2D { width, height };
    });
}

#[test]
fn a_suboptimal_present_after_a_resize_still_shows_the_frame_and_then_recreates_the_swapchain() {
    let (_context, mut presenter) = presenter();
    the_window_is_resized_to(640, 480);
    fake::with(|s| s.present_results.push_back(SUBOPTIMAL));
    fake::clear_log();
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
    let calls = fake::log();
    let present = calls.iter().position(|c| *c == "vkQueuePresentKHR");
    let create = calls.iter().position(|c| *c == "vkCreateSwapchainKHR");
    assert!(present < create, "recreate after the present: {calls:?}");
    assert_eq!(presenter.extent(), (640, 480));
}

#[test]
fn a_suboptimal_result_that_is_not_about_the_size_does_not_rebuild() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| {
        s.acquire_results.push_back(SUBOPTIMAL);
        s.present_results.push_back(SUBOPTIMAL);
    });
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
    assert_eq!(
        fake::with(|s| s.swapchains_created),
        1,
        "no rebuild per frame"
    );
}

#[test]
fn a_suboptimal_acquire_after_a_resize_draws_the_frame_and_then_recreates() {
    let (_context, mut presenter) = presenter();
    the_window_is_resized_to(640, 480);
    fake::with(|s| s.acquire_results.push_back(SUBOPTIMAL));
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
    assert_eq!(fake::presents_so_far(), 1, "the frame was shown");
    assert_eq!(fake::with(|s| s.swapchains_created), 2);
}

#[test]
fn a_failed_rebuild_retires_the_old_swapchain_and_the_next_one_starts_from_nothing() {
    let (_context, mut presenter) = presenter();
    the_window_is_resized_to(640, 480);
    fake::with(|s| s.create_swapchain_result = -1);
    assert!(presenter.resize(640, 480).is_err());
    assert!(fake::live_objects().iter().all(|o| o.0 != "swapchain"));
    fake::with(|s| s.create_swapchain_result = 0);
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
    let request = fake::with(|s| s.swapchain_request.clone());
    assert_eq!(
        request.map(|r| r.old_swapchain),
        Some(0),
        "a retired swapchain is never handed over again"
    );
}

#[test]
fn a_failure_while_recording_leaves_the_fence_signalled() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| s.fail_call = Some("vkBeginCommandBuffer"));
    fake::clear_log();
    assert!(presenter.present_clear([0.0; 4]).is_err());
    assert!(!fake::log().contains(&"vkResetFences"), "{:?}", fake::log());
    assert!(!fake::log().contains(&"vkQueueSubmit2"));
}

#[test]
fn a_failing_create_call_leaves_nothing_behind() {
    for call in [
        "vkCreateSemaphore",
        "vkCreateFence",
        "vkCreateCommandPool",
        "vkAllocateCommandBuffers",
        "vkCreateImageView",
        "vkGetSwapchainImagesKHR",
    ] {
        let mut system = script();
        system.fail_call = Some(call);
        let error = refused(system);
        assert!(
            matches!(error, VulkanError::Call { call: failed, .. } if failed == call),
            "{call}: {error:?}"
        );
        assert!(
            fake::live_objects().is_empty(),
            "{call} left {:?}",
            fake::live_objects()
        );
    }
}

#[test]
fn a_failing_capture_call_leaves_only_the_presenter_and_frees_it_all_at_the_end() {
    for call in [
        "vkCreateBuffer",
        "vkAllocateMemory",
        "vkBindBufferMemory",
        "vkMapMemory",
    ] {
        let (context, mut presenter) = presenter();
        let before = fake::live_objects();
        fake::with(|s| s.fail_call = Some(call));
        assert!(presenter.enable_capture().is_err(), "{call}");
        assert_eq!(fake::live_objects(), before, "{call} left objects");
        drop(presenter);
        drop(context);
        assert!(fake::live_objects().is_empty(), "{call}");
    }
}

#[test]
fn an_out_of_date_present_recreates_the_swapchain() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| s.present_results.push_back(ERROR_OUT_OF_DATE));
    assert_eq!(frame(&mut presenter), FrameOutcome::Recreated);
    assert_eq!(fake::with(|s| s.swapchains_created), 2);
}

#[test]
fn a_lost_surface_is_an_error_value_at_acquire_and_at_present() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| s.acquire_results.push_back(ERROR_SURFACE_LOST));
    assert_eq!(
        presenter.present_clear([0.0; 4]),
        Err(VulkanError::SurfaceLost)
    );
    fake::with(|s| s.present_results.push_back(ERROR_SURFACE_LOST));
    assert_eq!(
        presenter.present_clear([0.0; 4]),
        Err(VulkanError::SurfaceLost)
    );
}

#[test]
fn another_acquire_error_is_a_call_error_and_leaves_the_fence_signalled() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| s.acquire_results.push_back(-4));
    fake::clear_log();
    assert_eq!(
        presenter.present_clear([0.0; 4]),
        Err(VulkanError::Call {
            call: "vkAcquireNextImageKHR",
            result: -4
        })
    );
    assert!(!fake::log().contains(&"vkResetFences"));
}

#[test]
fn a_resize_makes_a_swapchain_of_the_new_size_and_frees_the_old_one() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| {
        s.surface_caps.current_extent = crate::vk_present::Extent2D {
            width: 1024,
            height: 768,
        };
    });
    assert_eq!(presenter.resize(1024, 768), Ok(()));
    assert_eq!(presenter.extent(), (1024, 768));
    let swapchains: Vec<_> = fake::live_objects()
        .into_iter()
        .filter(|o| o.0 == "swapchain")
        .collect();
    assert_eq!(swapchains.len(), 1);
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
}

#[test]
fn a_window_with_no_area_has_no_swapchain_until_it_has_one_again() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| {
        s.surface_caps.current_extent = crate::vk_present::Extent2D {
            width: 0,
            height: 0,
        };
    });
    assert_eq!(presenter.resize(0, 0), Ok(()));
    assert_eq!(frame(&mut presenter), FrameOutcome::Minimized);
    assert!(fake::live_objects().iter().all(|o| o.0 != "swapchain"));
    fake::with(|s| {
        s.surface_caps.current_extent = crate::vk_present::Extent2D {
            width: 640,
            height: 480,
        };
    });
    assert_eq!(presenter.resize(640, 480), Ok(()));
    assert_eq!(frame(&mut presenter), FrameOutcome::Presented);
    assert_eq!(presenter.extent(), (640, 480));
}

#[test]
fn a_captured_frame_gives_back_the_pixel_in_the_middle() {
    let (_context, mut presenter) = presenter();
    assert_eq!(presenter.enable_capture(), Ok(()));
    frame(&mut presenter);
    // Blue, green, red, alpha: the order of Bgra8.
    assert_eq!(presenter.captured_pixel(), Ok([255, 128, 0, 255]));
    assert_eq!(
        fake::with(|s| s.barriers.clone()),
        [
            (LAYOUT_UNDEFINED, LAYOUT_COLOR_ATTACHMENT),
            (LAYOUT_COLOR_ATTACHMENT, LAYOUT_TRANSFER_SRC),
            (LAYOUT_TRANSFER_SRC, LAYOUT_PRESENT_SRC)
        ]
    );
}

#[test]
fn without_capture_there_is_no_pixel() {
    let (_context, presenter) = presenter();
    assert!(matches!(
        presenter.captured_pixel(),
        Err(VulkanError::Unsupported(_))
    ));
}

#[test]
fn capture_is_unsupported_when_the_surface_cannot_copy_from_its_images() {
    let mut system = script();
    system.surface_caps.supported_usage_flags = IMAGE_USAGE_COLOR_ATTACHMENT;
    let (_context, mut presenter) = presenter_of(system);
    assert!(matches!(
        presenter.enable_capture(),
        Err(VulkanError::Unsupported(_))
    ));
}

#[test]
fn dropping_the_presenter_waits_idle_and_destroys_the_swapchain_objects_before_the_surface() {
    let (_context, presenter) = presenter();
    fake::clear_log();
    drop(presenter);
    let calls = only(
        &fake::log(),
        &[
            "vkDeviceWaitIdle",
            "vkDestroyFence",
            "vkDestroySemaphore",
            "vkDestroyCommandPool",
            "vkDestroyImageView",
            "vkDestroySwapchainKHR",
            "vkDestroySurfaceKHR",
        ],
    );
    assert_eq!(calls.first(), Some(&"vkDeviceWaitIdle"));
    assert_eq!(calls.last(), Some(&"vkDestroySurfaceKHR"));
    let swapchain = calls.iter().position(|c| *c == "vkDestroySwapchainKHR");
    let last_view = calls.iter().rposition(|c| *c == "vkDestroyImageView");
    let surface = calls.iter().position(|c| *c == "vkDestroySurfaceKHR");
    assert!(last_view.is_some() && swapchain.is_some() && surface.is_some());
    assert!(
        last_view < swapchain,
        "views before the swapchain: {calls:?}"
    );
    assert!(swapchain < surface);
}

#[test]
fn the_device_outlives_a_presenter_whose_context_was_dropped_first() {
    let (context, presenter) = presenter();
    drop(context);
    assert!(
        !fake::log().contains(&"vkDestroyDevice"),
        "the presenter still needs it"
    );
    fake::clear_log();
    drop(presenter);
    let calls = only(
        &fake::log(),
        &[
            "vkDestroySurfaceKHR",
            "vkDestroyDevice",
            "vkDestroyInstance",
        ],
    );
    assert_eq!(
        calls,
        [
            "vkDestroySurfaceKHR",
            "vkDestroyDevice",
            "vkDestroyInstance"
        ]
    );
    assert!(fake::live_objects().is_empty());
}

fn ready(presenter: &mut Presenter) -> Backbuffer {
    match presenter.begin_frame() {
        Ok(Begin::Ready(back)) => back,
        Ok(Begin::Skipped(outcome)) => panic!("expected a frame, it was skipped: {outcome:?}"),
        Err(error) => panic!("expected a frame, got: {error}"),
    }
}

#[test]
fn a_device_frame_waits_and_acquires_then_leaves_the_buffer_open_for_commands() {
    let (_context, mut presenter) = presenter();
    fake::clear_log();
    let back = ready(&mut presenter);
    assert_eq!((back.extent.width, back.extent.height), (800, 600));
    assert_ne!(back.image, 0);
    assert_ne!(back.view, 0);
    assert_eq!(
        only(
            &fake::log(),
            &[
                "vkWaitForFences",
                "vkAcquireNextImageKHR",
                "vkResetCommandPool",
                "vkBeginCommandBuffer",
                "vkEndCommandBuffer",
                "vkQueueSubmit2",
                "vkQueuePresentKHR",
            ]
        ),
        [
            "vkWaitForFences",
            "vkAcquireNextImageKHR",
            "vkResetCommandPool",
            "vkBeginCommandBuffer",
        ]
    );
}

#[test]
fn ending_a_device_frame_closes_the_buffer_submits_and_presents() {
    let (_context, mut presenter) = presenter();
    let back = ready(&mut presenter);
    fake::clear_log();
    assert_eq!(presenter.end_frame(back), Ok(FrameOutcome::Presented));
    assert_eq!(
        only(
            &fake::log(),
            &[
                "vkEndCommandBuffer",
                "vkResetFences",
                "vkQueueSubmit2",
                "vkQueuePresentKHR"
            ]
        ),
        [
            "vkEndCommandBuffer",
            "vkResetFences",
            "vkQueueSubmit2",
            "vkQueuePresentKHR"
        ]
    );
    let submits = fake::with(|s| s.submits.clone());
    assert_eq!(submits.len(), 1);
    assert_eq!(submits[0].wait_stage, STAGE_COLOR_ATTACHMENT_OUTPUT);
}

#[test]
fn device_frames_take_turns_between_the_two_frames_in_flight() {
    let (_context, mut presenter) = presenter();
    let mut fences = Vec::new();
    for _ in 0..4 {
        let back = ready(&mut presenter);
        assert_eq!(presenter.end_frame(back), Ok(FrameOutcome::Presented));
        fences.push(fake::with(|s| s.submits.last().map(|r| r.fence)));
    }
    assert_eq!(fences[0], fences[2]);
    assert_eq!(fences[1], fences[3]);
    assert_ne!(fences[0], fences[1]);
}

#[test]
fn a_device_frame_on_an_out_of_date_swapchain_is_skipped_and_the_swapchain_rebuilt() {
    let (_context, mut presenter) = presenter();
    fake::with(|s| s.acquire_results.push_back(ERROR_OUT_OF_DATE));
    assert!(matches!(
        presenter.begin_frame(),
        Ok(Begin::Skipped(FrameOutcome::Recreated))
    ));
    assert!(!fake::log().contains(&"vkBeginCommandBuffer"));
    let back = ready(&mut presenter);
    assert_eq!(presenter.end_frame(back), Ok(FrameOutcome::Presented));
}

#[test]
fn a_device_frame_on_a_window_without_area_is_skipped() {
    let (_context, mut presenter) = presenter();
    assert!(presenter.resize(0, 0).is_ok());
    the_window_is_resized_to(0, 0);
    assert!(presenter.resize(0, 0).is_ok());
    fake::clear_log();
    assert!(matches!(
        presenter.begin_frame(),
        Ok(Begin::Skipped(FrameOutcome::Minimized))
    ));
    assert!(!fake::log().contains(&"vkAcquireNextImageKHR"));
}

#[test]
fn a_failed_submit_of_a_device_frame_is_an_error_and_nothing_is_presented() {
    let (_context, mut presenter) = presenter();
    let back = ready(&mut presenter);
    fake::with(|s| s.submit_result = -4);
    fake::clear_log();
    assert!(matches!(
        presenter.end_frame(back),
        Err(VulkanError::Call {
            call: "vkQueueSubmit2",
            ..
        })
    ));
    assert!(!fake::log().contains(&"vkQueuePresentKHR"));
}
