//! The memory manager against the fake driver.

use super::{Allocation, Kind, MemoryManager, Placement};
use crate::VulkanError;
use crate::fake::{self, FakeGpu, Script};
use crate::vk_present::MemoryRequirements;

fn context() -> crate::VulkanContext {
    fake::started(Script::with_gpus(vec![FakeGpu::good("fake", 2)]))
}

fn manager(context: &crate::VulkanContext, block: u64) -> MemoryManager {
    match MemoryManager::new(context.core(), block) {
        Ok(manager) => manager,
        Err(error) => panic!("the manager should start: {error}"),
    }
}

fn want(size: u64, alignment: u64) -> MemoryRequirements {
    MemoryRequirements {
        size,
        alignment,
        memory_type_bits: 0b11,
    }
}

fn live(kind: &str) -> usize {
    fake::live_objects()
        .iter()
        .filter(|(name, _)| *name == kind)
        .count()
}

fn linear(manager: &mut MemoryManager, size: u64, placement: Placement) -> Allocation {
    allocate(manager, &want(size, 16), Kind::Linear, placement)
}

fn allocate(
    manager: &mut MemoryManager,
    requirements: &MemoryRequirements,
    kind: Kind,
    placement: Placement,
) -> Allocation {
    match manager.allocate(requirements, kind, placement) {
        Ok(allocation) => allocation,
        Err(error) => panic!("the allocation should work: {error}"),
    }
}

fn type_of(allocation: &Allocation) -> u32 {
    fake::with(|s| s.world.memories[&allocation.memory].type_index)
}

#[test]
fn the_first_allocation_makes_one_block_in_a_device_local_type() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let a = linear(&mut manager, 100, Placement::DeviceLocal);
    assert_eq!((a.offset, a.size), (0, 100));
    assert_eq!(manager.block_count(), 1);
    assert_eq!(fake::with(|s| s.world.allocations.clone()), vec![(0, 4096)]);
    assert!(a.mapped().is_none(), "device-local memory is not mapped");
    assert_eq!(type_of(&a), 0);
}

#[test]
fn small_allocations_share_a_block_without_overlapping() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let a = linear(&mut manager, 100, Placement::DeviceLocal);
    let b = allocate(
        &mut manager,
        &want(100, 64),
        Kind::Linear,
        Placement::DeviceLocal,
    );
    assert_eq!(manager.block_count(), 1);
    assert_eq!(a.memory, b.memory);
    assert_eq!(b.offset % 64, 0);
    assert!(a.offset + a.size <= b.offset || b.offset + b.size <= a.offset);
}

#[test]
fn upload_memory_is_host_visible_and_mapped_at_the_right_address() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let a = linear(&mut manager, 64, Placement::Upload);
    let b = linear(&mut manager, 64, Placement::Upload);
    assert_eq!(type_of(&a), 1);
    let (Some(pa), Some(pb)) = (a.mapped(), b.mapped()) else {
        panic!("upload memory is mapped");
    };
    // SAFETY: both pointers lie in the fake's 4096-byte block, 64 bytes each.
    unsafe {
        pa.as_ptr().write(0xAA);
        pb.as_ptr().write(0xBB);
    }
    let (first, second) = fake::with(|s| {
        let bytes = &s.world.memories[&a.memory].bytes;
        (
            bytes[usize::try_from(a.offset).unwrap_or(0)],
            bytes[usize::try_from(b.offset).unwrap_or(0)],
        )
    });
    assert_eq!((first, second), (0xAA, 0xBB));
}

#[test]
fn buffers_and_images_never_share_a_block() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let buffer = linear(&mut manager, 64, Placement::DeviceLocal);
    let image = allocate(
        &mut manager,
        &want(64, 16),
        Kind::Optimal,
        Placement::DeviceLocal,
    );
    assert_ne!(buffer.memory, image.memory);
    assert_eq!(manager.block_count(), 2);
}

#[test]
fn a_large_request_gets_a_block_of_its_own_with_its_exact_size() {
    let context = context();
    let mut manager = manager(&context, 4096);
    // Over a quarter of the block.
    let big = linear(&mut manager, 2000, Placement::DeviceLocal);
    assert_eq!(fake::with(|s| s.world.allocations.clone()), vec![(0, 2000)]);
    let small = linear(&mut manager, 64, Placement::DeviceLocal);
    assert_ne!(big.memory, small.memory, "the small one has its own block");
    manager.free(big);
    assert_eq!(live("memory"), 1, "the dedicated block went back");
}

#[test]
fn a_full_block_is_followed_by_another() {
    let context = context();
    let mut manager = manager(&context, 4096);
    for _ in 0..5 {
        let _ = linear(&mut manager, 1000, Placement::DeviceLocal);
    }
    assert_eq!(
        manager.block_count(),
        2,
        "four fit in 4096, the fifth does not"
    );
}

#[test]
fn freed_space_is_used_again() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let a = linear(&mut manager, 100, Placement::DeviceLocal);
    let _b = linear(&mut manager, 100, Placement::DeviceLocal);
    let (memory, offset) = (a.memory, a.offset);
    manager.free(a);
    let c = linear(&mut manager, 100, Placement::DeviceLocal);
    assert_eq!((c.memory, c.offset), (memory, offset));
    assert_eq!(manager.block_count(), 1);
}

#[test]
fn an_emptied_extra_block_goes_back_but_the_last_one_stays() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let first: Vec<_> = (0..4)
        .map(|_| linear(&mut manager, 1000, Placement::DeviceLocal))
        .collect();
    let second = linear(&mut manager, 1000, Placement::DeviceLocal);
    assert_eq!(manager.block_count(), 2);
    manager.free(second);
    assert_eq!(manager.block_count(), 1, "the extra block is returned");
    for a in first {
        manager.free(a);
    }
    assert_eq!(manager.block_count(), 1, "the last block of the pool stays");
    assert_eq!(live("memory"), 1);
}

#[test]
fn a_refused_block_is_an_error_and_leaves_nothing_behind() {
    let context = context();
    let mut manager = manager(&context, 4096);
    fake::with(|s| s.fail_call = Some("vkAllocateMemory"));
    let result = manager.allocate(&want(64, 16), Kind::Linear, Placement::DeviceLocal);
    assert!(matches!(
        result,
        Err(VulkanError::Call {
            call: "vkAllocateMemory",
            result: -1
        })
    ));
    assert_eq!(manager.block_count(), 0);
    assert_eq!(live("memory"), 0);
}

#[test]
fn running_out_of_device_memory_is_an_error_value() {
    let context = context();
    let mut manager = manager(&context, 4096);
    fake::with(|s| s.world.allocation_limit = Some(1));
    let _first = linear(&mut manager, 1000, Placement::DeviceLocal);
    let result = manager.allocate(&want(5000, 16), Kind::Linear, Placement::DeviceLocal);
    assert!(matches!(result, Err(VulkanError::Call { result: -2, .. })));
    assert_eq!(manager.block_count(), 1);
}

#[test]
fn a_block_whose_mapping_fails_is_freed_again() {
    let context = context();
    let mut manager = manager(&context, 4096);
    fake::with(|s| s.fail_call = Some("vkMapMemory"));
    let result = manager.allocate(&want(64, 16), Kind::Linear, Placement::Upload);
    assert!(matches!(
        result,
        Err(VulkanError::Call {
            call: "vkMapMemory",
            result: -1
        })
    ));
    assert_eq!(live("memory"), 0);
    assert_eq!(manager.block_count(), 0);
}

#[test]
fn no_memory_type_for_the_request_is_unsupported() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let none = MemoryRequirements {
        size: 64,
        alignment: 16,
        memory_type_bits: 0,
    };
    assert!(matches!(
        manager.allocate(&none, Kind::Linear, Placement::DeviceLocal),
        Err(VulkanError::Unsupported(_))
    ));
    // Only the device-local type is allowed, but the request needs host-visible memory.
    let device_only = MemoryRequirements {
        memory_type_bits: 0b01,
        ..none
    };
    assert!(matches!(
        manager.allocate(&device_only, Kind::Linear, Placement::Upload),
        Err(VulkanError::Unsupported(_))
    ));
    assert_eq!(live("memory"), 0);
}

#[test]
fn a_device_local_request_falls_back_to_any_allowed_type() {
    let context = context();
    let mut manager = manager(&context, 4096);
    let host_only = MemoryRequirements {
        size: 64,
        alignment: 16,
        memory_type_bits: 0b10,
    };
    let a = allocate(
        &mut manager,
        &host_only,
        Kind::Linear,
        Placement::DeviceLocal,
    );
    assert_eq!(type_of(&a), 1);
}

#[test]
fn dropping_the_manager_frees_every_block() {
    let context = context();
    {
        let mut manager = manager(&context, 4096);
        let _ = linear(&mut manager, 64, Placement::DeviceLocal);
        let _ = linear(&mut manager, 64, Placement::Upload);
        let _ = allocate(
            &mut manager,
            &want(3000, 16),
            Kind::Optimal,
            Placement::DeviceLocal,
        );
        assert_eq!(live("memory"), 3);
    }
    assert_eq!(live("memory"), 0);
}

#[test]
fn a_request_of_size_zero_or_an_odd_alignment_is_refused_without_a_block() {
    let context = context();
    let mut manager = manager(&context, 4096);
    for bad in [want(0, 16), want(64, 24)] {
        assert!(matches!(
            manager.allocate(&bad, Kind::Linear, Placement::DeviceLocal),
            Err(VulkanError::Invalid(_))
        ));
    }
    assert_eq!(live("memory"), 0);
    assert!(fake::with(|s| s.world.allocations.is_empty()));
}
