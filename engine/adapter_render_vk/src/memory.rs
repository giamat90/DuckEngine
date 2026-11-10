//! Device memory, sub-allocated: pools of blocks, each block one `vkAllocateMemory` call that
//! many buffers and images share (card 6-8, choice O2).
//!
//! A pool is one memory type and one kind of resource (linear buffers, or optimal-tiling images):
//! keeping the kinds apart means `bufferImageGranularity` never matters. A request larger than a
//! quarter of a block gets a block of its own. Host-visible blocks are mapped once, whole, and
//! stay mapped. An emptied block goes back to the driver unless it is the last of its pool.
#![expect(
    unsafe_code,
    reason = "device memory is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::error::{VulkanError, check_created};
use crate::gear::{MemoryFns, load_memory_fns, ok};
use crate::plan::pick_memory_type;
use crate::suballoc::FreeList;
use crate::vk::{DeviceHandle, ERROR_INITIALIZATION_FAILED};
use crate::vk_present::{
    MEMORY_HOST_COHERENT, MEMORY_HOST_VISIBLE, MemoryAllocateInfo, MemoryHandle,
    MemoryRequirements, PhysicalDeviceMemoryProperties, STYPE_MEMORY_ALLOCATE_INFO,
};
use crate::vk_resource::MEMORY_DEVICE_LOCAL;
use core::ffi::c_void;
use core::ptr::NonNull;
use std::rc::Rc;

/// The size of an ordinary block: 32 MiB.
pub const BLOCK_SIZE: u64 = 32 * 1024 * 1024;

/// `VK_WHOLE_SIZE`.
const WHOLE_SIZE: u64 = u64::MAX;

/// Which kind of resource a pool serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Buffers.
    Linear,
    /// Images in optimal tiling.
    Optimal,
}

/// Where the memory should live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Fast for the GPU; the CPU does not touch it.
    DeviceLocal,
    /// Visible to the CPU and coherent: staging and readback.
    Upload,
}

/// A range of device memory handed out by the manager.
#[derive(Debug)]
pub struct Allocation {
    /// The `VkDeviceMemory` the range lies in.
    pub memory: MemoryHandle,
    /// Where the range starts in that memory.
    pub offset: u64,
    /// The length.
    pub size: u64,
    mapped: Option<NonNull<u8>>,
    pool: usize,
    block: usize,
}

impl Allocation {
    /// The start of the range in the address space of the CPU, for host-visible memory.
    #[must_use]
    pub fn mapped(&self) -> Option<NonNull<u8>> {
        self.mapped
    }

    /// Gives the handle up: memory, offset, size, pool and block index.
    fn into_parts(self) -> (MemoryHandle, u64, u64, usize, usize) {
        (self.memory, self.offset, self.size, self.pool, self.block)
    }

    fn new(block: &Block, offset: u64, size: u64, pool: usize, index: usize) -> Self {
        let mapped = block.base.and_then(|base| {
            let at = usize::try_from(offset).ok()?;
            // SAFETY: `offset` lies inside the block, which is mapped whole.
            NonNull::new(unsafe { base.as_ptr().add(at) })
        });
        Self {
            memory: block.memory,
            offset,
            size,
            mapped,
            pool,
            block: index,
        }
    }
}

/// One `vkAllocateMemory` block and which of its bytes are free.
struct Block {
    memory: MemoryHandle,
    list: FreeList,
    /// The start of the whole block in the address space of the CPU, for host-visible memory.
    base: Option<NonNull<u8>>,
    /// Made for one oversized request: freed as soon as it is empty.
    dedicated: bool,
}

/// The blocks of one memory type and one kind of resource. A freed slot is `None`, so the index
/// of a block stays valid for the allocations that name it.
struct Pool {
    type_index: u32,
    kind: Kind,
    blocks: Vec<Option<Block>>,
}

/// The pools of one device.
pub struct MemoryManager {
    _core: Rc<Core>,
    fns: MemoryFns,
    device: DeviceHandle,
    properties: PhysicalDeviceMemoryProperties,
    block_size: u64,
    pools: Vec<Pool>,
}

impl MemoryManager {
    /// A manager with blocks of `block_size` bytes.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] when the driver lacks a memory function.
    pub fn new(core: &Rc<Core>, block_size: u64) -> Result<Self, VulkanError> {
        Ok(Self {
            _core: Rc::clone(core),
            fns: load_memory_fns(core)?,
            device: core.device.handle(),
            properties: core.instance.memory_properties(core.gpu),
            block_size,
            pools: Vec::new(),
        })
    }

    /// Hands out a range that meets `requirements`.
    ///
    /// # Errors
    /// [`VulkanError::Unsupported`] when no memory type fits; [`VulkanError::Call`] when the
    /// driver refuses a new block (out of memory, or the mapping of a host-visible one).
    pub fn allocate(
        &mut self,
        requirements: &MemoryRequirements,
        kind: Kind,
        placement: Placement,
    ) -> Result<Allocation, VulkanError> {
        let alignment = requirements.alignment.max(1);
        if requirements.size == 0 || !alignment.is_power_of_two() {
            return Err(VulkanError::Invalid(
                "the memory requirements cannot be met",
            ));
        }
        let type_index = self.choose_type(requirements.memory_type_bits, placement)?;
        let pool = self.pool_for(type_index, kind);
        let size = requirements.size;
        let dedicated = size > self.block_size / 4;
        if !dedicated && let Some(found) = self.find_room(pool, size, alignment) {
            return Ok(found);
        }
        let capacity = if dedicated { size } else { self.block_size };
        let block = self.make_block(type_index, capacity, dedicated)?;
        self.place_in_new_block(pool, block, size, alignment)
    }

    /// Takes a range back (the allocation is consumed, so it cannot be freed twice); an emptied
    /// block that is not the last of its pool is returned to the driver.
    pub fn free(&mut self, allocation: Allocation) {
        // The handle is spent here: nothing can name this range again.
        let (memory, offset, size, pool, index) = allocation.into_parts();
        let Some(pool) = self.pools.get_mut(pool) else {
            return;
        };
        let ordinary = pool
            .blocks
            .iter()
            .flatten()
            .filter(|block| !block.dedicated)
            .count();
        let Some(slot) = pool.blocks.get_mut(index) else {
            return;
        };
        let Some(block) = slot else {
            return;
        };
        let released = block.memory == memory && block.list.release(offset, size);
        if released
            && block.list.is_unused()
            && (block.dedicated || ordinary > 1)
            && let Some(emptied) = slot.take()
        {
            self.release_block(&emptied);
        }
    }

    /// How many `vkAllocateMemory` blocks are alive.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.pools
            .iter()
            .map(|pool| pool.blocks.iter().flatten().count())
            .sum()
    }

    /// Frees every block. The device must be idle, and nothing may use an allocation afterwards.
    pub fn release_all(&mut self) {
        let pools = core::mem::take(&mut self.pools);
        for block in pools.iter().flat_map(|pool| pool.blocks.iter().flatten()) {
            self.release_block(block);
        }
    }

    /// The memory type for `placement` among those `type_bits` allows.
    fn choose_type(&self, type_bits: u32, placement: Placement) -> Result<u32, VulkanError> {
        let properties = &self.properties;
        let found = match placement {
            Placement::DeviceLocal => pick_memory_type(properties, type_bits, MEMORY_DEVICE_LOCAL)
                .or_else(|| pick_memory_type(properties, type_bits, 0)),
            Placement::Upload => pick_memory_type(
                properties,
                type_bits,
                MEMORY_HOST_VISIBLE | MEMORY_HOST_COHERENT,
            ),
        };
        found.ok_or(VulkanError::Unsupported(
            "no memory type fits the resource and the placement",
        ))
    }

    fn is_host_visible(&self, type_index: u32) -> bool {
        usize::try_from(type_index)
            .ok()
            .and_then(|i| self.properties.memory_types.get(i))
            .is_some_and(|memory| memory.property_flags & MEMORY_HOST_VISIBLE != 0)
    }

    /// The index of the pool for this type and kind, made on first use.
    fn pool_for(&mut self, type_index: u32, kind: Kind) -> usize {
        let found = self
            .pools
            .iter()
            .position(|pool| pool.type_index == type_index && pool.kind == kind);
        found.unwrap_or_else(|| {
            self.pools.push(Pool {
                type_index,
                kind,
                blocks: Vec::new(),
            });
            self.pools.len() - 1
        })
    }

    /// A range in a block the pool already has, if one has room.
    fn find_room(&mut self, pool: usize, size: u64, alignment: u64) -> Option<Allocation> {
        let this = self.pools.get_mut(pool)?;
        this.blocks
            .iter_mut()
            .enumerate()
            .find_map(|(index, slot)| {
                let block = slot.as_mut().filter(|block| !block.dedicated)?;
                let offset = block.list.allocate(size, alignment)?;
                Some(Allocation::new(block, offset, size, pool, index))
            })
    }

    /// One `vkAllocateMemory` block of `capacity` bytes, mapped whole when host-visible. Nothing
    /// is left behind when it fails.
    fn make_block(
        &self,
        type_index: u32,
        capacity: u64,
        dedicated: bool,
    ) -> Result<Block, VulkanError> {
        let info = MemoryAllocateInfo {
            s_type: STYPE_MEMORY_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            allocation_size: capacity,
            memory_type_index: type_index,
        };
        let mut memory: MemoryHandle = 0;
        // SAFETY: `info` is valid for the call; `memory` is a valid output.
        let result = unsafe {
            (self.fns.allocate)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut memory,
            )
        };
        check_created("vkAllocateMemory", result, memory == 0)?;
        let base = self.map_if_visible(memory, type_index)?;
        Ok(Block {
            memory,
            list: FreeList::new(capacity),
            base,
            dedicated,
        })
    }

    /// Maps a host-visible block whole; frees the block again when the mapping fails.
    fn map_if_visible(
        &self,
        memory: MemoryHandle,
        type_index: u32,
    ) -> Result<Option<NonNull<u8>>, VulkanError> {
        if !self.is_host_visible(type_index) {
            return Ok(None);
        }
        let mapped = self.map(memory);
        if mapped.is_err() {
            // SAFETY: allocated by the caller on this device and not in use.
            unsafe { (self.fns.free)(self.device, memory, core::ptr::null()) };
        }
        mapped.map(Some)
    }

    fn map(&self, memory: MemoryHandle) -> Result<NonNull<u8>, VulkanError> {
        let mut address: *mut c_void = core::ptr::null_mut();
        // SAFETY: the memory is host-visible and not mapped; `address` is a valid output.
        let result =
            unsafe { (self.fns.map)(self.device, memory, 0, WHOLE_SIZE, 0, &raw mut address) };
        ok("vkMapMemory", result)?;
        NonNull::new(address.cast::<u8>()).ok_or(VulkanError::Call {
            call: "vkMapMemory",
            result: ERROR_INITIALIZATION_FAILED,
        })
    }

    /// The first range of a fresh block, which is stored in the pool.
    fn place_in_new_block(
        &mut self,
        pool: usize,
        mut block: Block,
        size: u64,
        alignment: u64,
    ) -> Result<Allocation, VulkanError> {
        let Some(offset) = block.list.allocate(size, alignment) else {
            // A size the block cannot hold (or an alignment that is not a power of two).
            self.release_block(&block);
            return Err(VulkanError::Invalid(
                "the memory requirements cannot be met",
            ));
        };
        let Some(this) = self.pools.get_mut(pool) else {
            self.release_block(&block);
            return Err(VulkanError::Invalid("the pool does not exist"));
        };
        let index = this
            .blocks
            .iter()
            .position(Option::is_none)
            .unwrap_or(this.blocks.len());
        let allocation = Allocation::new(&block, offset, size, pool, index);
        if let Some(slot) = this.blocks.get_mut(index) {
            *slot = Some(block);
        } else {
            this.blocks.push(Some(block));
        }
        Ok(allocation)
    }

    /// Unmaps and frees one block.
    fn release_block(&self, block: &Block) {
        if block.base.is_some() {
            // SAFETY: the block was mapped when it was made and is unmapped once, here.
            unsafe { (self.fns.unmap)(self.device, block.memory) };
        }
        // SAFETY: allocated on this device and freed once, here; the caller guarantees nothing
        // uses it any more.
        unsafe { (self.fns.free)(self.device, block.memory, core::ptr::null()) };
    }
}

impl Drop for MemoryManager {
    fn drop(&mut self) {
        self.release_all();
    }
}

#[cfg(test)]
mod tests;
