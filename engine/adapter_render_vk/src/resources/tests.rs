//! Buffers, textures and samplers against the fake driver.

use super::{
    AddressMode, BufferDesc, BufferId, BufferUsage, Filter, Region, Resources, SamplerDesc,
    SamplerId, TextureDesc, TextureFormat, TextureId, check_region, check_texture, mip_limit,
};
use crate::VulkanError;
use crate::fake::resource::Op;
use crate::fake::{self, FakeGpu, Script};
use crate::vk_present::{LAYOUT_TRANSFER_SRC, LAYOUT_UNDEFINED};
use crate::vk_resource::{
    ACCESS_HOST_READ, ACCESS_TRANSFER_WRITE, FEATURE_DEPTH_ATTACHMENT, STAGE_ALL_TRANSFER,
    STAGE_HOST,
};
use crate::vk_resource::{
    ADDRESS_CLAMP_TO_EDGE, ADDRESS_REPEAT, FILTER_LINEAR, FILTER_NEAREST, LAYOUT_SHADER_READ,
    LAYOUT_TRANSFER_DST, LOD_CLAMP_NONE, MIPMAP_LINEAR, MIPMAP_NEAREST,
};

const SMALL_BLOCK: u64 = 1 << 20;

fn context() -> crate::VulkanContext {
    fake::started(Script::with_gpus(vec![FakeGpu::good("fake", 2)]))
}

fn resources(context: &crate::VulkanContext) -> Resources {
    match Resources::new(context.core(), SMALL_BLOCK) {
        Ok(resources) => resources,
        Err(error) => panic!("the resources should start: {error}"),
    }
}

fn desc(width: u32, height: u32) -> TextureDesc {
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

fn buffer(resources: &mut Resources, size: u32) -> BufferId {
    match resources.create_buffer(BufferDesc {
        size,
        usage: BufferUsage::Vertex,
    }) {
        Ok(id) => id,
        Err(error) => panic!("the buffer should be made: {error}"),
    }
}

fn texture(resources: &mut Resources, description: TextureDesc) -> TextureId {
    match resources.create_texture(description) {
        Ok(id) => id,
        Err(error) => panic!("the texture should be made: {error}"),
    }
}

fn sampler(resources: &mut Resources, description: SamplerDesc) -> SamplerId {
    match resources.create_sampler(description) {
        Ok(id) => id,
        Err(error) => panic!("the sampler should be made: {error}"),
    }
}

fn pattern(length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect()
}

fn memory_of(handle: u64) -> Vec<u8> {
    fake::with(|s| {
        let (memory, offset) = s.world.buffers[&handle].bound.unwrap_or((0, 0));
        let start = usize::try_from(offset).unwrap_or(0);
        let size = usize::try_from(s.world.buffers[&handle].size).unwrap_or(0);
        s.world.memories[&memory].bytes[start..start + size].to_vec()
    })
}

/// The live objects that are not device memory (a pool keeps one empty block, by design).
fn live_without_memory() -> usize {
    fake::live_objects()
        .iter()
        .filter(|(kind, _)| *kind != "memory")
        .count()
}

// ----- pure checks -----

#[test]
fn the_mip_chain_of_a_size_has_the_expected_length() {
    assert_eq!(mip_limit(1, 1), 1);
    assert_eq!(mip_limit(2, 1), 2);
    assert_eq!(mip_limit(256, 256), 9);
    assert_eq!(mip_limit(300, 20), 9);
}

#[test]
fn a_texture_description_is_checked_against_the_limits() {
    assert!(check_texture(&desc(4, 4), 16).is_ok());
    assert!(check_texture(&desc(0, 4), 16).is_err());
    assert!(check_texture(&desc(4, 0), 16).is_err());
    assert!(check_texture(&desc(17, 4), 16).is_err());
    assert!(check_texture(&desc(4, 17), 16).is_err());
    let mut mips = desc(4, 4);
    mips.mip_count = 3;
    assert!(check_texture(&mips, 16).is_ok());
    mips.mip_count = 4;
    assert!(check_texture(&mips, 16).is_err());
    mips.mip_count = 0;
    assert!(check_texture(&mips, 16).is_err());
}

#[test]
fn a_region_must_lie_inside_and_match_the_byte_count() {
    let region = Region {
        x: 1,
        y: 1,
        width: 2,
        height: 2,
    };
    assert!(check_region(4, 4, region, 16).is_ok());
    assert!(check_region(4, 4, region, 15).is_err());
    assert!(check_region(4, 4, region, 17).is_err());
    assert!(check_region(4, 4, Region { x: -1, ..region }, 16).is_err());
    assert!(check_region(4, 4, Region { y: -1, ..region }, 16).is_err());
    assert!(check_region(4, 4, Region { x: 3, ..region }, 16).is_err());
    assert!(check_region(4, 4, Region { y: 3, ..region }, 16).is_err());
    let huge = Region {
        x: i32::MAX,
        y: 0,
        width: u32::MAX,
        height: 1,
    };
    assert!(check_region(4, 4, huge, 16).is_err(), "no overflow");
}

// ----- buffers -----

#[test]
fn a_buffer_is_made_bound_and_counted() {
    let context = context();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 256);
    assert_eq!(resources.counts(), (1, 0, 0));
    let handle = resources.buffer_handle(id).unwrap_or(0);
    let bound = fake::with(|s| s.world.buffers[&handle].bound);
    assert!(bound.is_some(), "the buffer has memory");
    assert_eq!(fake::with(|s| s.world.buffers[&handle].size), 256);
}

#[test]
fn a_buffer_of_size_zero_is_refused() {
    let context = context();
    let mut resources = resources(&context);
    let result = resources.create_buffer(BufferDesc {
        size: 0,
        usage: BufferUsage::Index,
    });
    assert!(matches!(result, Err(VulkanError::Invalid(_))));
    assert_eq!(resources.counts(), (0, 0, 0));
}

#[test]
fn two_buffers_share_a_block_without_overlap() {
    let context = context();
    let mut resources = resources(&context);
    let _a = buffer(&mut resources, 100);
    let _b = buffer(&mut resources, 100);
    assert_eq!(resources.memory_blocks(), 1);
}

#[test]
fn written_bytes_arrive_at_the_offset() {
    let context = context();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 64);
    let data = pattern(16);
    assert!(resources.write_buffer(id, 8, &data).is_ok());
    let raw = resources.buffer_handle(id).unwrap_or(0);
    let stored = memory_of(raw);
    assert_eq!(&stored[8..24], data.as_slice());
    assert!(stored[..8].iter().all(|b| *b == 0));
    assert!(stored[24..].iter().all(|b| *b == 0));
}

#[test]
fn a_buffer_write_is_wrapped_in_barriers_in_the_right_order() {
    let context = context();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 64);
    fake::with(|s| s.world.ops.clear());
    assert!(resources.write_buffer(id, 0, &pattern(8)).is_ok());
    let ops = fake::with(|s| s.world.ops.clone());
    assert!(matches!(
        ops.as_slice(),
        [
            Op::GlobalBarrier,
            Op::CopyBuffer { size: 8, .. },
            Op::GlobalBarrier
        ]
    ));
}

#[test]
fn a_buffer_write_outside_the_buffer_is_refused_and_changes_nothing() {
    let context = context();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 16);
    let handle = resources.buffer_handle(id).unwrap_or(0);
    assert!(matches!(
        resources.write_buffer(id, 8, &pattern(16)),
        Err(VulkanError::Invalid(_))
    ));
    assert!(matches!(
        resources.write_buffer(id, u32::MAX, &pattern(16)),
        Err(VulkanError::Invalid(_))
    ));
    assert!(memory_of(handle).iter().all(|b| *b == 0));
}

#[test]
fn an_empty_buffer_write_does_nothing() {
    let context = context();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 16);
    fake::with(|s| s.world.ops.clear());
    assert!(resources.write_buffer(id, 16, &[]).is_ok());
    assert!(fake::with(|s| s.world.ops.is_empty()));
}

#[test]
fn a_write_larger_than_the_staging_buffer_grows_it() {
    let context = context();
    let mut resources = resources(&context);
    let size = 200_000_u32;
    let id = buffer(&mut resources, size);
    let data = pattern(usize::try_from(size).unwrap_or(0));
    assert!(resources.write_buffer(id, 0, &data).is_ok());
    let handle = resources.buffer_handle(id).unwrap_or(0);
    assert_eq!(memory_of(handle), data);
    // A second, smaller write reuses it.
    assert!(resources.write_buffer(id, 0, &pattern(4)).is_ok());
}

#[test]
fn a_destroyed_buffer_is_gone_and_its_id_stays_stale() {
    let context = context();
    let before = fake::live_objects();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 64);
    resources.destroy_buffer(id);
    assert_eq!(resources.counts(), (0, 0, 0));
    assert!(matches!(
        resources.write_buffer(id, 0, &[1]),
        Err(VulkanError::InvalidHandle)
    ));
    assert!(matches!(
        resources.buffer_handle(id),
        Err(VulkanError::InvalidHandle)
    ));
    resources.destroy_buffer(id);
    let again = buffer(&mut resources, 64);
    assert_ne!(again, id, "a reused slot has a new generation");
    assert!(matches!(
        resources.buffer_handle(id),
        Err(VulkanError::InvalidHandle)
    ));
    drop(resources);
    assert_eq!(fake::live_objects().len(), before.len());
}

#[test]
fn an_id_survives_the_trip_through_a_number() {
    let context = context();
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 64);
    assert_eq!(BufferId::from_bits(id.to_bits()), id);
    let t = texture(&mut resources, desc(2, 2));
    assert_eq!(TextureId::from_bits(t.to_bits()), t);
    let s = sampler(&mut resources, plain());
    assert_eq!(SamplerId::from_bits(s.to_bits()), s);
}

#[test]
fn a_failing_buffer_creation_leaves_nothing_behind() {
    for call in ["vkCreateBuffer", "vkAllocateMemory", "vkBindBufferMemory"] {
        let context = context();
        let mut resources = resources(&context);
        let before = live_without_memory();
        fake::with(|s| s.fail_call = Some(call));
        let result = resources.create_buffer(BufferDesc {
            size: 64,
            usage: BufferUsage::Uniform,
        });
        assert!(matches!(result, Err(VulkanError::Call { .. })), "{call}");
        assert_eq!(resources.counts(), (0, 0, 0), "{call}");
        assert_eq!(live_without_memory(), before, "{call}");
        // The range went back: a block that stays is empty and is used again.
        fake::with(|s| s.fail_call = None);
        let _ = buffer(&mut resources, 64);
        assert!(resources.memory_blocks() <= 1, "{call}");
    }
}

// ----- textures -----

#[test]
fn a_texture_is_made_with_an_image_a_view_and_memory() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(8, 4));
    assert_eq!(resources.counts(), (0, 1, 0));
    let handles = resources.texture_handles(id);
    let Ok(handles) = handles else {
        panic!("the handles should exist");
    };
    assert_ne!(handles.image, 0);
    assert_ne!(handles.view, 0);
    assert_eq!(handles.layout, LAYOUT_UNDEFINED);
    assert!(fake::with(|s| s.world.images[&handles.image]
        .bound
        .is_some()));
}

#[test]
fn texture_descriptions_that_do_not_fit_are_refused() {
    // The limit is read when the context starts, so it is set in the script.
    let context = fake::started({
        let mut script = Script::with_gpus(vec![FakeGpu::good("fake", 2)]);
        script.world.max_extent = 64;
        script
    });
    let mut resources = resources(&context);
    assert!(matches!(
        resources.create_texture(desc(65, 1)),
        Err(VulkanError::Invalid(_))
    ));
    assert!(matches!(
        resources.create_texture(desc(0, 1)),
        Err(VulkanError::Invalid(_))
    ));
    let mut too_many = desc(4, 4);
    too_many.mip_count = 9;
    assert!(matches!(
        resources.create_texture(too_many),
        Err(VulkanError::Invalid(_))
    ));
    assert_eq!(resources.counts(), (0, 0, 0));
}

#[test]
fn texels_written_to_a_texture_can_be_read_back() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(4, 2));
    let data = pattern(4 * 2 * 4);
    assert!(resources.write_texture(id, whole(4, 2), &data).is_ok());
    let mut out = vec![0_u8; data.len()];
    assert!(resources.read_back(id, &mut out).is_ok());
    assert_eq!(out, data);
}

#[test]
fn a_partial_write_changes_only_its_region() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(4, 4));
    let base = vec![9_u8; 4 * 4 * 4];
    assert!(resources.write_texture(id, whole(4, 4), &base).is_ok());
    let patch = vec![7_u8; 2 * 2 * 4];
    let region = Region {
        x: 1,
        y: 2,
        width: 2,
        height: 2,
    };
    assert!(resources.write_texture(id, region, &patch).is_ok());
    let mut out = vec![0_u8; base.len()];
    assert!(resources.read_back(id, &mut out).is_ok());
    for (index, texel) in out.chunks_exact(4).enumerate() {
        let (x, y) = (index % 4, index / 4);
        let inside = (1..3).contains(&x) && (2..4).contains(&y);
        let expected = if inside { 7 } else { 9 };
        assert_eq!(texel, &[expected; 4], "texel {x},{y}");
    }
}

#[test]
fn the_first_write_starts_from_undefined_and_ends_ready_to_sample() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    let handles = resources.texture_handles(id).map(|h| h.image).unwrap_or(0);
    fake::with(|s| s.world.ops.clear());
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
    let ops = fake::with(|s| s.world.ops.clone());
    assert_eq!(
        ops,
        vec![
            Op::ImageBarrier {
                image: handles,
                old: LAYOUT_UNDEFINED,
                new: LAYOUT_TRANSFER_DST
            },
            Op::BufferToImage {
                image: handles,
                width: 2,
                height: 2
            },
            Op::ImageBarrier {
                image: handles,
                old: LAYOUT_TRANSFER_DST,
                new: LAYOUT_SHADER_READ
            },
        ]
    );
    assert_eq!(
        resources.texture_handles(id).map(|h| h.layout).unwrap_or(0),
        LAYOUT_SHADER_READ
    );
}

#[test]
fn a_later_write_starts_from_the_tracked_layout() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
    let image = resources.texture_handles(id).map(|h| h.image).unwrap_or(0);
    fake::with(|s| s.world.ops.clear());
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
    let first = fake::with(|s| s.world.ops[0].clone());
    assert_eq!(
        first,
        Op::ImageBarrier {
            image,
            old: LAYOUT_SHADER_READ,
            new: LAYOUT_TRANSFER_DST
        }
    );
}

#[test]
fn read_back_returns_the_texture_to_its_layout() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
    let image = resources.texture_handles(id).map(|h| h.image).unwrap_or(0);
    fake::with(|s| s.world.ops.clear());
    let mut out = vec![0_u8; 16];
    assert!(resources.read_back(id, &mut out).is_ok());
    let ops = fake::with(|s| s.world.ops.clone());
    assert_eq!(
        ops,
        vec![
            Op::ImageBarrier {
                image,
                old: LAYOUT_SHADER_READ,
                new: LAYOUT_TRANSFER_SRC
            },
            Op::ImageToBuffer {
                image,
                width: 2,
                height: 2
            },
            Op::GlobalBarrier,
            Op::ImageBarrier {
                image,
                old: LAYOUT_TRANSFER_SRC,
                new: LAYOUT_SHADER_READ
            },
        ]
    );
}

#[test]
fn read_back_makes_the_copy_visible_to_the_host_before_the_cpu_reads() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    fake::with(|s| s.world.global_masks.clear());
    let mut out = vec![0_u8; 16];
    assert!(resources.read_back(id, &mut out).is_ok());
    let masks = fake::with(|s| s.world.global_masks.clone());
    assert_eq!(
        masks,
        vec![(
            STAGE_ALL_TRANSFER,
            ACCESS_TRANSFER_WRITE,
            STAGE_HOST,
            ACCESS_HOST_READ
        )]
    );
}

#[test]
fn reading_a_never_written_texture_settles_it_in_its_resting_layout() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    let mut out = vec![1_u8; 16];
    assert!(resources.read_back(id, &mut out).is_ok());
    assert_eq!(
        resources.texture_handles(id).map(|h| h.layout).unwrap_or(0),
        LAYOUT_SHADER_READ
    );
}

#[test]
fn bad_texture_writes_are_refused_before_anything_is_recorded() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    fake::with(|s| s.world.ops.clear());
    assert!(matches!(
        resources.write_texture(id, whole(3, 2), &pattern(24)),
        Err(VulkanError::Invalid(_))
    ));
    assert!(matches!(
        resources.write_texture(id, whole(2, 2), &pattern(15)),
        Err(VulkanError::Invalid(_))
    ));
    assert!(fake::with(|s| s.world.ops.is_empty()));
    let mut short = vec![0_u8; 3];
    assert!(matches!(
        resources.read_back(id, &mut short),
        Err(VulkanError::Invalid(_))
    ));
}

#[test]
fn a_depth_texture_cannot_be_written_but_can_be_read_back() {
    let context = context();
    let mut resources = resources(&context);
    let depth = TextureDesc {
        format: TextureFormat::Depth32F,
        ..desc(2, 2)
    };
    let id = texture(&mut resources, depth);
    assert!(matches!(
        resources.write_texture(id, whole(2, 2), &pattern(16)),
        Err(VulkanError::Unsupported(_))
    ));
    let mut out = vec![0_u8; 16];
    assert!(resources.read_back(id, &mut out).is_ok());
}

#[test]
fn a_depth_texture_the_device_cannot_copy_from_is_not_readable() {
    let context = context();
    fake::with(|s| s.world.depth_features = FEATURE_DEPTH_ATTACHMENT);
    let mut resources = resources(&context);
    let depth = TextureDesc {
        format: TextureFormat::Depth32F,
        ..desc(2, 2)
    };
    let id = texture(&mut resources, depth);
    let mut out = vec![0_u8; 16];
    assert!(matches!(
        resources.read_back(id, &mut out),
        Err(VulkanError::Unsupported(_))
    ));
}

#[test]
fn a_destroyed_texture_is_gone_and_everything_it_held_is_freed() {
    let context = context();
    let mut resources = resources(&context);
    let before = fake::live_objects().len();
    let id = texture(&mut resources, desc(4, 4));
    assert!(fake::live_objects().len() > before);
    resources.destroy_texture(id);
    assert_eq!(resources.counts(), (0, 0, 0));
    assert!(matches!(
        resources.texture_handles(id),
        Err(VulkanError::InvalidHandle)
    ));
    assert!(matches!(
        resources.write_texture(id, whole(4, 4), &pattern(64)),
        Err(VulkanError::InvalidHandle)
    ));
    let mut out = vec![0_u8; 64];
    assert!(matches!(
        resources.read_back(id, &mut out),
        Err(VulkanError::InvalidHandle)
    ));
    assert!(matches!(
        resources.note_layout(id, LAYOUT_SHADER_READ),
        Err(VulkanError::InvalidHandle)
    ));
    resources.destroy_texture(id);
}

#[test]
fn a_layout_noted_by_the_renderer_is_the_one_the_next_write_starts_from() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    assert!(resources.note_layout(id, LAYOUT_TRANSFER_SRC).is_ok());
    let image = resources.texture_handles(id).map(|h| h.image).unwrap_or(0);
    fake::with(|s| s.world.ops.clear());
    // The fake image still holds the layout the barriers set, so reflect the note there too.
    fake::with(|s| {
        if let Some(i) = s.world.images.get_mut(&image) {
            i.layout = LAYOUT_TRANSFER_SRC;
        }
    });
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
    let first = fake::with(|s| s.world.ops[0].clone());
    assert_eq!(
        first,
        Op::ImageBarrier {
            image,
            old: LAYOUT_TRANSFER_SRC,
            new: LAYOUT_TRANSFER_DST
        }
    );
}

#[test]
fn a_failing_texture_creation_leaves_nothing_behind() {
    for call in [
        "vkCreateImage",
        "vkAllocateMemory",
        "vkBindImageMemory",
        "vkCreateImageView",
    ] {
        let context = context();
        let mut resources = resources(&context);
        let before = live_without_memory();
        fake::with(|s| s.fail_call = Some(call));
        let result = resources.create_texture(desc(4, 4));
        assert!(matches!(result, Err(VulkanError::Call { .. })), "{call}");
        assert_eq!(resources.counts(), (0, 0, 0), "{call}");
        assert_eq!(live_without_memory(), before, "{call}");
        fake::with(|s| s.fail_call = None);
        let _ = texture(&mut resources, desc(4, 4));
        assert!(resources.memory_blocks() <= 1, "{call}");
    }
}

#[test]
fn a_failing_transfer_is_an_error_and_the_next_one_works() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    fake::with(|s| s.fail_call = Some("vkBeginCommandBuffer"));
    assert!(matches!(
        resources.write_texture(id, whole(2, 2), &pattern(16)),
        Err(VulkanError::Call { .. })
    ));
    assert_eq!(
        resources.texture_handles(id).map(|h| h.layout).unwrap_or(1),
        LAYOUT_UNDEFINED,
        "the layout did not change"
    );
    fake::with(|s| s.fail_call = None);
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
}

// ----- samplers -----

fn plain() -> SamplerDesc {
    SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }
}

#[test]
fn a_sampler_carries_the_requested_filters_and_address_mode() {
    let context = context();
    let mut resources = resources(&context);
    let id = sampler(&mut resources, plain());
    let handle = resources.sampler_handle(id).unwrap_or(0);
    let record = fake::with(|s| s.world.samplers[&handle].clone());
    assert_eq!(record.min_filter, FILTER_NEAREST);
    assert_eq!(record.mag_filter, FILTER_NEAREST);
    assert_eq!(record.mipmap_mode, MIPMAP_NEAREST);
    assert_eq!(record.address, ADDRESS_CLAMP_TO_EDGE);
    assert!((record.max_lod - LOD_CLAMP_NONE).abs() < f32::EPSILON);

    let id = sampler(
        &mut resources,
        SamplerDesc {
            min_filter: Filter::Linear,
            mag_filter: Filter::Nearest,
            address: AddressMode::Repeat,
        },
    );
    let handle = resources.sampler_handle(id).unwrap_or(0);
    let record = fake::with(|s| s.world.samplers[&handle].clone());
    assert_eq!(record.min_filter, FILTER_LINEAR);
    assert_eq!(record.mag_filter, FILTER_NEAREST);
    assert_eq!(record.mipmap_mode, MIPMAP_LINEAR);
    assert_eq!(record.address, ADDRESS_REPEAT);
}

#[test]
fn a_destroyed_sampler_is_gone() {
    let context = context();
    let mut resources = resources(&context);
    let id = sampler(&mut resources, plain());
    assert_eq!(resources.counts(), (0, 0, 1));
    resources.destroy_sampler(id);
    resources.destroy_sampler(id);
    assert_eq!(resources.counts(), (0, 0, 0));
    assert!(matches!(
        resources.sampler_handle(id),
        Err(VulkanError::InvalidHandle)
    ));
    assert_eq!(fake::with(|s| s.world.samplers.len()), 0);
}

#[test]
fn a_refused_sampler_is_an_error_value() {
    let context = context();
    let mut resources = resources(&context);
    fake::with(|s| s.fail_call = Some("vkCreateSampler"));
    assert!(matches!(
        resources.create_sampler(plain()),
        Err(VulkanError::Call { .. })
    ));
    assert_eq!(resources.counts(), (0, 0, 0));
}

// ----- the whole -----

#[test]
fn dropping_the_resources_frees_everything_including_staging() {
    let context = context();
    let before = fake::live_objects().len();
    {
        let mut resources = resources(&context);
        let b = buffer(&mut resources, 128);
        assert!(resources.write_buffer(b, 0, &pattern(32)).is_ok());
        let t = texture(&mut resources, desc(4, 4));
        assert!(
            resources
                .write_texture(t, whole(4, 4), &pattern(64))
                .is_ok()
        );
        let _s = sampler(&mut resources, plain());
        assert!(fake::live_objects().len() > before);
    }
    assert_eq!(fake::live_objects().len(), before);
    assert_eq!(fake::with(|s| s.world.memories.len()), 0);
}

#[test]
fn resources_start_on_a_device_without_the_swapchain_extension() {
    let mut gpu = FakeGpu::good("headless", 2);
    gpu.swapchain_extension = false;
    let context = fake::started(Script::with_gpus(vec![gpu]));
    let mut resources = resources(&context);
    let id = buffer(&mut resources, 16);
    assert!(resources.write_buffer(id, 0, &pattern(16)).is_ok());
}

#[test]
fn a_failing_start_leaves_no_object_behind() {
    for call in [
        "vkCreateCommandPool",
        "vkAllocateCommandBuffers",
        "vkCreateFence",
    ] {
        let context = context();
        let before = fake::live_objects().len();
        fake::with(|s| s.fail_call = Some(call));
        assert!(
            matches!(
                Resources::new(context.core(), SMALL_BLOCK),
                Err(VulkanError::Call { .. })
            ),
            "{call}"
        );
        assert_eq!(fake::live_objects().len(), before, "{call}");
        fake::with(|s| s.fail_call = None);
    }
}

#[test]
fn the_context_makes_resources_with_the_default_block() {
    let context = context();
    let mut resources = match context.create_resources() {
        Ok(resources) => resources,
        Err(error) => panic!("resources should start: {error}"),
    };
    let _ = buffer(&mut resources, 64);
    assert_eq!(
        fake::with(|s| s.world.allocations.clone()),
        vec![(0, crate::memory::BLOCK_SIZE)]
    );
}

#[test]
fn a_depth_format_the_device_cannot_attach_is_refused_at_creation() {
    let context = context();
    fake::with(|s| s.world.depth_features = 0);
    let mut resources = resources(&context);
    let depth = TextureDesc {
        format: TextureFormat::Depth32F,
        ..desc(2, 2)
    };
    assert!(matches!(
        resources.create_texture(depth),
        Err(VulkanError::Unsupported(_))
    ));
    assert_eq!(resources.counts(), (0, 0, 0));
}

#[test]
fn a_wait_that_fails_after_the_submit_still_tracks_the_new_layout() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    fake::with(|s| s.fail_call = Some("vkWaitForFences"));
    assert!(matches!(
        resources.write_texture(id, whole(2, 2), &pattern(16)),
        Err(VulkanError::Call { .. })
    ));
    let layout = resources.texture_handles(id).map(|h| h.layout).unwrap_or(0);
    assert_eq!(layout, LAYOUT_SHADER_READ, "the GPU got the work");
    // The next transfer waits for the pending one first, then starts from that layout.
    fake::with(|s| s.fail_call = None);
    let image = resources.texture_handles(id).map(|h| h.image).unwrap_or(0);
    fake::with(|s| s.world.ops.clear());
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_ok()
    );
    let first = fake::with(|s| s.world.ops[0].clone());
    assert_eq!(
        first,
        Op::ImageBarrier {
            image,
            old: LAYOUT_SHADER_READ,
            new: LAYOUT_TRANSFER_DST
        }
    );
}

#[test]
fn a_failed_submit_before_the_gpu_got_the_work_keeps_the_old_layout() {
    let context = context();
    let mut resources = resources(&context);
    let id = texture(&mut resources, desc(2, 2));
    fake::with(|s| s.submit_result = -4);
    assert!(
        resources
            .write_texture(id, whole(2, 2), &pattern(16))
            .is_err()
    );
    let layout = resources.texture_handles(id).map(|h| h.layout).unwrap_or(1);
    assert_eq!(layout, LAYOUT_UNDEFINED);
}
