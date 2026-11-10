//! Buffers, textures and samplers on a real device (card 6-8).
//!
//! Like the context tests, these need a Vulkan 1.3 device; without one they print
//! `not run: device not set up (...)` and pass, and `DUCK_REQUIRE_VK=1` turns a missing device
//! into a failure. The validation layer is switched on when it is installed, and a message from
//! it fails the test.

use adapter_render_vk::{
    AddressMode, BufferDesc, BufferUsage, Filter, Region, Resources, SamplerDesc, TextureDesc,
    TextureFormat, VulkanContext, VulkanError, VulkanOptions,
};

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

/// Starts a context, or says why there is none on this machine.
fn start() -> Result<VulkanContext, String> {
    match VulkanContext::create(VulkanOptions { validation: true }) {
        Ok(context) => Ok(context),
        Err(error @ (VulkanError::NoLoader | VulkanError::NoSuitableDevice)) => {
            Err(error.to_string())
        }
        Err(error) => panic!("starting Vulkan failed: {error}"),
    }
}

fn resources_of(context: &VulkanContext) -> Resources {
    match context.create_resources() {
        Ok(resources) => resources,
        Err(error) => panic!("the resources should start: {error}"),
    }
}

/// The validation layer reported nothing; says so when the layer was not there to report.
fn assert_quiet(context: &VulkanContext) {
    if !context.info().validation_active {
        eprintln!("note: the validation layer is not installed, so this run was not validated");
    }
    let report = context.validation_report();
    assert!(report.is_clean(), "validation said: {:#?}", report.messages);
}

fn pattern(length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect()
}

fn rgba(width: u32, height: u32) -> TextureDesc {
    TextureDesc {
        width,
        height,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }
}

fn whole(width: u32, height: u32) -> Region {
    Region {
        x: 0,
        y: 0,
        width,
        height,
    }
}

#[test]
fn texels_survive_the_trip_to_the_device_and_back() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let mut resources = resources_of(&context);
    for (width, height) in [(1, 1), (7, 5), (64, 64), (300, 17)] {
        let Ok(id) = resources.create_texture(rgba(width, height)) else {
            panic!("{width}x{height}: the texture should be made");
        };
        let size = usize::try_from(width * height * 4).unwrap_or(0);
        let data = pattern(size);
        assert_eq!(
            resources.write_texture(id, whole(width, height), &data),
            Ok(()),
            "{width}x{height}"
        );
        let mut out = vec![0_u8; size];
        assert_eq!(
            resources.read_back(id, &mut out),
            Ok(()),
            "{width}x{height}"
        );
        assert_eq!(out, data, "{width}x{height}");
        resources.destroy_texture(id);
    }
    assert_eq!(resources.counts(), (0, 0, 0));
    assert_eq!(context.wait_idle(), Ok(()));
    assert_quiet(&context);
}

#[test]
fn a_partial_write_changes_only_its_region_on_the_device() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let mut resources = resources_of(&context);
    let Ok(id) = resources.create_texture(rgba(8, 8)) else {
        panic!("the texture should be made");
    };
    assert_eq!(
        resources.write_texture(id, whole(8, 8), &vec![9_u8; 8 * 8 * 4]),
        Ok(())
    );
    let region = Region {
        x: 2,
        y: 3,
        width: 3,
        height: 2,
    };
    assert_eq!(
        resources.write_texture(id, region, &[7_u8; 3 * 2 * 4]),
        Ok(())
    );
    let mut out = vec![0_u8; 8 * 8 * 4];
    assert_eq!(resources.read_back(id, &mut out), Ok(()));
    for (index, texel) in out.chunks_exact(4).enumerate() {
        let (x, y) = (index % 8, index / 8);
        let inside = (2..5).contains(&x) && (3..5).contains(&y);
        let expected = if inside { 7 } else { 9 };
        assert_eq!(texel, &[expected; 4], "texel {x},{y}");
    }
    assert_quiet(&context);
}

#[test]
fn a_buffer_takes_writes_larger_than_the_first_staging_buffer() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let mut resources = resources_of(&context);
    let size = 300_000_u32;
    let Ok(id) = resources.create_buffer(BufferDesc {
        size,
        usage: BufferUsage::Vertex,
    }) else {
        panic!("the buffer should be made");
    };
    let data = pattern(usize::try_from(size).unwrap_or(0));
    assert_eq!(resources.write_buffer(id, 0, &data), Ok(()));
    assert_eq!(resources.write_buffer(id, 100, &data[..64]), Ok(()));
    assert_eq!(
        resources.write_buffer(id, size, &[1]),
        Err(VulkanError::Invalid("the range leaves the buffer"))
    );
    assert_quiet(&context);
}

#[test]
fn many_small_resources_share_a_few_blocks_and_all_go_back() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let mut resources = resources_of(&context);
    let mut buffers = Vec::new();
    let mut textures = Vec::new();
    for round in 0..200_u32 {
        let desc = BufferDesc {
            size: 256 + round,
            usage: BufferUsage::Uniform,
        };
        match resources.create_buffer(desc) {
            Ok(id) => buffers.push(id),
            Err(error) => panic!("buffer {round}: {error}"),
        }
        match resources.create_texture(rgba(16, 16)) {
            Ok(id) => textures.push(id),
            Err(error) => panic!("texture {round}: {error}"),
        }
    }
    assert_eq!(resources.counts(), (200, 200, 0));
    assert!(
        resources.memory_blocks() <= 8,
        "{} blocks for 400 small resources",
        resources.memory_blocks()
    );
    for id in buffers {
        resources.destroy_buffer(id);
    }
    for id in textures {
        resources.destroy_texture(id);
    }
    assert_eq!(resources.counts(), (0, 0, 0));
    assert_quiet(&context);
}

#[test]
fn samplers_are_made_and_destroyed() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let mut resources = resources_of(&context);
    let desc = SamplerDesc {
        min_filter: Filter::Linear,
        mag_filter: Filter::Linear,
        address: AddressMode::Repeat,
    };
    let Ok(id) = resources.create_sampler(desc) else {
        panic!("the sampler should be made");
    };
    assert!(resources.sampler_handle(id).is_ok_and(|handle| handle != 0));
    resources.destroy_sampler(id);
    assert_eq!(
        resources.sampler_handle(id),
        Err(VulkanError::InvalidHandle)
    );
    assert_quiet(&context);
}

#[test]
fn a_depth_texture_is_made_and_a_bad_size_is_refused() {
    let context = match start() {
        Ok(context) => context,
        Err(reason) => return not_run(&reason),
    };
    let mut resources = resources_of(&context);
    let depth = TextureDesc {
        format: TextureFormat::Depth32F,
        ..rgba(64, 64)
    };
    let Ok(id) = resources.create_texture(depth) else {
        panic!("the depth texture should be made");
    };
    let mut texels = vec![0_u8; 64 * 64 * 4];
    assert!(matches!(
        resources.read_back(id, &mut texels),
        Ok(()) | Err(VulkanError::Unsupported(_))
    ));
    resources.destroy_texture(id);
    assert!(matches!(
        resources.create_texture(rgba(0, 4)),
        Err(VulkanError::Invalid(_))
    ));
    assert!(matches!(
        resources.create_texture(rgba(u32::MAX, 4)),
        Err(VulkanError::Invalid(_))
    ));
    assert_quiet(&context);
}
