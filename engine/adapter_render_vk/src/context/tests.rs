//! The start-up path against the fake driver: no GPU, no loader.
#![expect(
    unsafe_code,
    reason = "the tests hand the fake driver's lookup function to the entry point"
)]

use super::*;
use crate::fake::{self, FakeGpu, Script};
use crate::vk::QUEUE_GRAPHICS;

const DISCRETE: u32 = 2;
const INTEGRATED: u32 = 1;
const CPU: u32 = 4;
const TRANSFER_ONLY: u32 = 4;

fn start_with(options: VulkanOptions) -> Result<VulkanContext, VulkanError> {
    // SAFETY: the fake lookup function returns functions with the signatures the adapter expects.
    let entry = unsafe { Entry::from_proc_addr(fake::get_instance_proc_addr, None) }?;
    VulkanContext::create_with_entry(entry, options)
}

fn started_with(options: VulkanOptions) -> VulkanContext {
    match start_with(options) {
        Ok(context) => context,
        Err(error) => panic!("expected a context, got: {error}"),
    }
}

fn refused_with(options: VulkanOptions) -> VulkanError {
    match start_with(options) {
        Err(error) => error,
        Ok(_) => panic!("expected an error, got a context"),
    }
}

fn started(script: Script) -> VulkanContext {
    fake::install(script);
    started_with(VulkanOptions::default())
}

fn refused(script: Script) -> VulkanError {
    fake::install(script);
    refused_with(VulkanOptions::default())
}

fn one_gpu() -> Script {
    Script::with_gpus(vec![FakeGpu::good("Fake GPU", DISCRETE)])
}

fn calls(names: &[&str]) -> Vec<&'static str> {
    fake::log()
        .into_iter()
        .filter(|call| names.contains(call))
        .collect()
}

#[test]
fn a_context_makes_the_instance_then_the_device() {
    let context = started(one_gpu());
    assert_eq!(
        calls(&["vkCreateInstance", "vkCreateDevice", "vkGetDeviceQueue"]),
        ["vkCreateInstance", "vkCreateDevice", "vkGetDeviceQueue"]
    );
    drop(context);
}

#[test]
fn dropping_a_context_destroys_the_device_before_the_instance() {
    let context = started(one_gpu());
    fake::clear_log();
    drop(context);
    assert_eq!(
        fake::log(),
        ["vkDeviceWaitIdle", "vkDestroyDevice", "vkDestroyInstance"]
    );
}

#[test]
fn the_context_says_which_device_it_chose() {
    let context = started(one_gpu());
    let info = context.info();
    assert_eq!(info.name, "Fake GPU");
    assert_eq!(info.device_type, DeviceType::Discrete);
    assert_eq!(info.api_version, ApiVersion::new(1, 3, 280));
    assert_eq!(info.instance_version, ApiVersion::new(1, 4, 300));
    assert_eq!(
        (info.vendor_id, info.device_id, info.driver_version),
        (0x10DE, 0x2500, 7)
    );
    assert_eq!(info.queue_family, 0);
    assert!(!info.validation_active);
}

#[test]
fn a_loader_older_than_1_3_is_refused_before_any_instance_exists() {
    let mut script = one_gpu();
    script.instance_version = ApiVersion::new(1, 2, 198).raw();
    let error = refused(script);
    assert_eq!(
        error,
        VulkanError::InstanceTooOld(ApiVersion::new(1, 2, 198))
    );
    assert!(!fake::log().contains(&"vkCreateInstance"));
}

#[test]
fn a_1_0_loader_without_the_version_query_is_too_old() {
    let mut script = one_gpu();
    script.missing.push("vkEnumerateInstanceVersion");
    let error = refused(script);
    assert_eq!(error, VulkanError::InstanceTooOld(ApiVersion::new(1, 0, 0)));
}

#[test]
fn a_failed_instance_is_an_error_value_and_nothing_is_destroyed() {
    let mut script = one_gpu();
    script.create_instance_result = -9;
    let error = refused(script);
    assert_eq!(
        error,
        VulkanError::Call {
            call: "vkCreateInstance",
            result: -9
        }
    );
    assert!(!fake::log().contains(&"vkDestroyInstance"));
}

#[test]
fn a_loader_without_vkcreateinstance_does_not_start() {
    let mut script = one_gpu();
    script.missing.push("vkCreateInstance");
    fake::install(script);
    // SAFETY: as in `start_with`.
    let Err(error) = (unsafe { Entry::from_proc_addr(fake::get_instance_proc_addr, None) }) else {
        panic!("a loader without vkCreateInstance must be refused");
    };
    assert_eq!(error, VulkanError::MissingFunction("vkCreateInstance"));
}

#[test]
fn a_machine_without_devices_gives_no_suitable_device_and_frees_the_instance() {
    let error = refused(Script::with_gpus(Vec::new()));
    assert_eq!(error, VulkanError::NoSuitableDevice);
    assert_eq!(calls(&["vkDestroyInstance"]), ["vkDestroyInstance"]);
    assert!(!fake::log().contains(&"vkCreateDevice"));
}

#[test]
fn a_device_that_misses_a_requirement_is_not_used() {
    let mut weak = FakeGpu::good("Old", DISCRETE);
    weak.synchronization2 = false;
    let error = refused(Script::with_gpus(vec![weak]));
    assert_eq!(error, VulkanError::NoSuitableDevice);
    assert!(!fake::log().contains(&"vkCreateDevice"));
    assert!(fake::log().contains(&"vkDestroyInstance"));
}

#[test]
fn a_device_with_only_a_transfer_queue_is_not_used() {
    let mut no_graphics = FakeGpu::good("Transfer", DISCRETE);
    no_graphics.families = vec![TRANSFER_ONLY];
    let error = refused(Script::with_gpus(vec![no_graphics]));
    assert_eq!(error, VulkanError::NoSuitableDevice);
}

#[test]
fn the_best_qualifying_device_is_chosen_and_its_graphics_family_is_used() {
    let mut discrete = FakeGpu::good("Discrete", DISCRETE);
    discrete.families = vec![TRANSFER_ONLY, QUEUE_GRAPHICS | TRANSFER_ONLY];
    let mut too_old = FakeGpu::good("Old discrete", DISCRETE);
    too_old.api = ApiVersion::new(1, 2, 0).raw();
    let gpus = vec![
        FakeGpu::good("Software", CPU),
        too_old,
        FakeGpu::good("Integrated", INTEGRATED),
        discrete,
    ];
    let context = started(Script::with_gpus(gpus));
    assert_eq!(context.info().name, "Discrete");
    assert_eq!(context.info().queue_family, 1);
    let Some(request) = fake::with(|script| script.device_request.clone()) else {
        panic!("a device was made");
    };
    assert_eq!(request.gpu, 3);
    assert_eq!((request.queue_family, request.queue_count), (1, 1));
    assert_eq!(fake::with(|script| script.queue_request), Some((1, 0)));
}

#[test]
fn the_device_is_made_with_exactly_the_features_the_baseline_names() {
    let _context = started(one_gpu());
    let Some(request) = fake::with(|script| script.device_request.clone()) else {
        panic!("a device was made");
    };
    assert!(request.anisotropy && request.dynamic_rendering && request.synchronization2);
}

#[test]
fn a_failed_device_frees_the_instance_and_makes_no_device() {
    let mut script = one_gpu();
    script.create_device_result = -8;
    let error = refused(script);
    assert_eq!(
        error,
        VulkanError::Call {
            call: "vkCreateDevice",
            result: -8
        }
    );
    assert!(fake::log().contains(&"vkDestroyInstance"));
    assert!(!fake::log().contains(&"vkDestroyDevice"));
}

#[test]
fn a_missing_instance_function_frees_the_instance() {
    let mut script = one_gpu();
    script.missing.push("vkCreateDevice");
    let error = refused(script);
    assert_eq!(error, VulkanError::MissingFunction("vkCreateDevice"));
    assert_eq!(
        calls(&["vkCreateInstance", "vkDestroyInstance"]),
        ["vkCreateInstance", "vkDestroyInstance"]
    );
}

#[test]
fn a_missing_device_function_frees_the_device_and_the_instance() {
    let mut script = one_gpu();
    script.missing.push("vkGetDeviceQueue");
    let error = refused(script);
    assert_eq!(error, VulkanError::MissingFunction("vkGetDeviceQueue"));
    let tail = calls(&["vkDestroyDevice", "vkDestroyInstance"]);
    assert_eq!(tail, ["vkDestroyDevice", "vkDestroyInstance"]);
}

#[test]
fn waiting_for_the_device_to_be_idle_reports_the_driver_result() {
    let context = started(one_gpu());
    assert_eq!(context.wait_idle(), Ok(()));
    fake::with(|script| script.wait_idle_result = -4);
    assert_eq!(
        context.wait_idle(),
        Err(VulkanError::Call {
            call: "vkDeviceWaitIdle",
            result: -4
        })
    );
}

#[test]
fn validation_is_off_unless_asked_for() {
    let mut script = one_gpu();
    script.validation_layer = true;
    let context = started(script);
    assert_eq!(fake::with(|script| script.layers_requested), 0);
    assert!(!context.info().validation_active);
    assert!(!fake::log().contains(&"vkCreateDebugUtilsMessengerEXT"));
}

#[test]
fn validation_is_turned_on_when_asked_for_and_present() {
    let mut script = one_gpu();
    script.validation_layer = true;
    fake::install(script);
    let context = started_with(VulkanOptions { validation: true });
    assert!(context.info().validation_active);
    assert_eq!(
        fake::with(|script| (script.layers_requested, script.extensions_requested)),
        (1, 1)
    );
    assert_eq!(
        calls(&[
            "vkCreateInstance",
            "vkCreateDebugUtilsMessengerEXT",
            "vkCreateDevice"
        ]),
        [
            "vkCreateInstance",
            "vkCreateDebugUtilsMessengerEXT",
            "vkCreateDevice"
        ]
    );
    fake::clear_log();
    drop(context);
    assert_eq!(
        fake::log(),
        [
            "vkDeviceWaitIdle",
            "vkDestroyDevice",
            "vkDestroyDebugUtilsMessengerEXT",
            "vkDestroyInstance"
        ]
    );
}

#[test]
fn validation_asked_for_but_absent_runs_without_it() {
    fake::install(one_gpu());
    let context = started_with(VulkanOptions { validation: true });
    assert!(!context.info().validation_active);
    assert_eq!(fake::with(|script| script.layers_requested), 0);
    assert!(!fake::log().contains(&"vkCreateDebugUtilsMessengerEXT"));
    assert_eq!(context.validation_report(), ValidationReport::default());
}

#[test]
fn a_failed_start_after_the_messenger_destroys_the_messenger_before_the_instance() {
    let mut script = Script::with_gpus(Vec::new());
    script.validation_layer = true;
    fake::install(script);
    let error = refused_with(VulkanOptions { validation: true });
    assert_eq!(error, VulkanError::NoSuitableDevice);
    assert_eq!(
        calls(&["vkDestroyDebugUtilsMessengerEXT", "vkDestroyInstance"]),
        ["vkDestroyDebugUtilsMessengerEXT", "vkDestroyInstance"]
    );
}

#[test]
fn a_driver_that_gives_no_queue_fails_and_frees_the_device_and_the_instance() {
    let mut script = one_gpu();
    script.null_queue = true;
    let error = refused(script);
    assert_eq!(
        error,
        VulkanError::Call {
            call: "vkGetDeviceQueue",
            result: -3
        }
    );
    assert_eq!(
        calls(&["vkDestroyDevice", "vkDestroyInstance"]),
        ["vkDestroyDevice", "vkDestroyInstance"]
    );
}
