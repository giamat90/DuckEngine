//! The Vulkan context on a real device (card 6-6).
//!
//! The tests need a Vulkan 1.3 loader and device (a GPU driver, or a software driver such as
//! lavapipe); without them they print `not run: device not set up (...)` and pass. A job that is
//! meant to have a device sets `DUCK_REQUIRE_VK=1`, and then a missing device fails the test
//! instead. The validation layer is switched on when it is installed, and a message from it fails
//! the test.

use adapter_render_vk::{BASELINE, VulkanContext, VulkanError, VulkanOptions};

/// Whether `DUCK_REQUIRE_VK` asks for a device: set to anything but empty or `0`.
fn device_required() -> bool {
    std::env::var("DUCK_REQUIRE_VK").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Reports why a device test cannot run: a failure when a device is required, else a note.
fn not_run(reason: &str) {
    assert!(
        !device_required(),
        "a Vulkan device is required here but is not set up: {reason}"
    );
    eprintln!("not run: device not set up ({reason})");
}

/// Starts a context, or says why there is none on this machine. A failure that is not "no
/// loader" or "no suitable device" is a real error and fails the test.
fn start() -> Result<VulkanContext, String> {
    match VulkanContext::create(VulkanOptions { validation: true }) {
        Ok(context) => Ok(context),
        Err(error @ (VulkanError::NoLoader | VulkanError::NoSuitableDevice)) => {
            Err(error.to_string())
        }
        Err(error) => panic!("starting Vulkan failed: {error}"),
    }
}

#[test]
fn a_context_starts_on_this_machines_device_and_validation_stays_quiet() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let info = context.info();
    eprintln!(
        "device: {} ({:?}), device API {}, loader {}, driver {:#x}, queue family {}, validation {}",
        info.name,
        info.device_type,
        info.api_version,
        info.instance_version,
        info.driver_version,
        info.queue_family,
        if info.validation_active {
            "on"
        } else {
            "off (layer not installed)"
        },
    );
    assert!(BASELINE.is_met_by(info.api_version), "{}", info.api_version);
    assert!(BASELINE.is_met_by(info.instance_version));
    assert!(!info.name.is_empty());
    assert_eq!(context.wait_idle(), Ok(()));
    let report = context.validation_report();
    assert!(report.is_clean(), "validation said: {:#?}", report.messages);
}

#[test]
fn contexts_can_be_made_and_dropped_again_and_again() {
    if let Err(reason) = start() {
        return not_run(&reason);
    }
    for round in 0..5 {
        let Ok(context) = start() else {
            panic!("round {round}: the first start worked, the next must too");
        };
        assert_eq!(context.wait_idle(), Ok(()), "round {round}");
        let report = context.validation_report();
        assert!(report.is_clean(), "round {round}: {:#?}", report.messages);
        drop(context);
    }
}
