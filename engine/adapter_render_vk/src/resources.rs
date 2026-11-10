//! Buffers, textures and samplers on a Vulkan device (card 6-8, part 3 of the Vulkan adapter).
//!
//! [`Resources`] owns the memory manager, the uploader and the tables of live resources. Each
//! resource is named by a generational id: a destroyed or never-made id is refused with
//! [`VulkanError::InvalidHandle`], and a slot that is reused does not answer to the old id.
//! Buffers and textures live in device-local memory and are written through a staging buffer; a
//! write returns when the transfer has finished. The types here mirror the descriptions of the
//! render port, which the adapter does not depend on yet (card 6-9 maps one onto the other).
//!
//! Destroying a resource frees it at once. The caller must know that no submitted work still uses
//! it (wait for the frame, or [`Resources::wait_idle`]).
#![expect(
    unsafe_code,
    reason = "buffers, images and samplers are a C interface; every unsafe block has a SAFETY comment"
)]

use crate::context::Core;
use crate::error::{VulkanError, check_created};
use crate::gear::{ResourceFns, load_resource_fns, ok};
use crate::memory::{Allocation, BLOCK_SIZE, Kind, MemoryManager, Placement};
use crate::slab::{Key, Slab};
use crate::upload::{EVERYTHING, ImageTransition, NOTHING, StageAccess, Uploader, tight_region};
use crate::vk_present::{
    ASPECT_COLOR, BufferCreateInfo, BufferHandle, FORMAT_B8G8R8A8_UNORM,
    IMAGE_USAGE_COLOR_ATTACHMENT, IMAGE_VIEW_TYPE_2D, ImageHandle, ImageViewCreateInfo,
    ImageViewHandle, LAYOUT_TRANSFER_SRC, LAYOUT_UNDEFINED, MemoryRequirements, SHARING_EXCLUSIVE,
    STYPE_BUFFER_CREATE_INFO, STYPE_IMAGE_VIEW_CREATE_INFO, SubresourceRange,
};
use crate::vk_resource::{ACCESS_HOST_READ, FEATURE_DEPTH_ATTACHMENT, STAGE_HOST};
use crate::vk_resource::{
    ACCESS_TRANSFER_READ, ACCESS_TRANSFER_WRITE, ADDRESS_CLAMP_TO_EDGE, ADDRESS_REPEAT,
    ASPECT_DEPTH, BUFFER_USAGE_INDEX, BUFFER_USAGE_TRANSFER_DST, BUFFER_USAGE_UNIFORM,
    BUFFER_USAGE_VERTEX, BufferCopy, FEATURE_TRANSFER_SRC, FILTER_LINEAR, FILTER_NEAREST,
    FORMAT_D32_SFLOAT, FORMAT_R8G8B8A8_UNORM, IMAGE_TYPE_2D, IMAGE_USAGE_DEPTH_ATTACHMENT,
    IMAGE_USAGE_SAMPLED, IMAGE_USAGE_TRANSFER_DST, IMAGE_USAGE_TRANSFER_SRC, ImageCreateInfo,
    LAYOUT_DEPTH_ATTACHMENT, LAYOUT_SHADER_READ, LAYOUT_TRANSFER_DST, LOD_CLAMP_NONE,
    MIPMAP_LINEAR, MIPMAP_NEAREST, SAMPLES_1, STAGE_ALL_TRANSFER, STYPE_IMAGE_CREATE_INFO,
    STYPE_SAMPLER_CREATE_INFO, SamplerCreateInfo, SamplerHandle, TILING_OPTIMAL,
};
use std::rc::Rc;

/// What a buffer is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferUsage {
    /// Vertex data.
    Vertex,
    /// Index data.
    Index,
    /// Uniform (constant) data.
    Uniform,
}

/// Buffer creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc {
    /// Size in bytes; must be non-zero.
    pub size: u32,
    /// Intended use.
    pub usage: BufferUsage,
}

/// Texel format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    /// 8-bit RGBA, unsigned normalised.
    Rgba8,
    /// 8-bit BGRA, unsigned normalised.
    Bgra8,
    /// 32-bit float depth.
    Depth32F,
}

impl TextureFormat {
    fn vk(self) -> u32 {
        match self {
            Self::Rgba8 => FORMAT_R8G8B8A8_UNORM,
            Self::Bgra8 => FORMAT_B8G8R8A8_UNORM,
            Self::Depth32F => FORMAT_D32_SFLOAT,
        }
    }

    fn aspect(self) -> u32 {
        match self {
            Self::Rgba8 | Self::Bgra8 => ASPECT_COLOR,
            Self::Depth32F => ASPECT_DEPTH,
        }
    }

    /// Every format of v0 has four bytes per texel.
    const BYTES_PER_TEXEL: u64 = 4;

    /// The layout a never-written or just-read texture settles in.
    fn resting_layout(self) -> u32 {
        match self {
            Self::Rgba8 | Self::Bgra8 => LAYOUT_SHADER_READ,
            Self::Depth32F => LAYOUT_DEPTH_ATTACHMENT,
        }
    }
}

/// 2D texture creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureDesc {
    /// Width in texels; must be non-zero.
    pub width: u32,
    /// Height in texels; must be non-zero.
    pub height: u32,
    /// Number of mip levels; at least 1, at most the length of the chain.
    pub mip_count: u32,
    /// Texel format.
    pub format: TextureFormat,
}

/// A rectangle of texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Texture filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Nearest texel.
    Nearest,
    /// Linear blend.
    Linear,
}

/// What happens outside `0..1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressMode {
    /// Clamp to the edge texel.
    Clamp,
    /// Repeat.
    Repeat,
}

/// Sampler creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplerDesc {
    /// Filter when minifying (also picks the mip filter).
    pub min_filter: Filter,
    /// Filter when magnifying.
    pub mag_filter: Filter,
    /// Address mode in both directions.
    pub address: AddressMode,
}

macro_rules! resource_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(Key);

        impl $name {
            /// The id as one number, for handles that cross a boundary.
            #[must_use]
            pub fn to_bits(self) -> u64 {
                self.0.to_bits()
            }

            /// The id a number from `to_bits` stands for.
            #[must_use]
            pub fn from_bits(bits: u64) -> Self {
                Self(Key::from_bits(bits))
            }
        }
    };
}

resource_id!(
    /// Names a buffer of a [`Resources`].
    BufferId
);
resource_id!(
    /// Names a texture of a [`Resources`].
    TextureId
);
resource_id!(
    /// Names a sampler of a [`Resources`].
    SamplerId
);

/// The Vulkan objects of a texture, for the code that draws with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureHandles {
    /// The `VkImage`.
    pub image: u64,
    /// A view of all mip levels.
    pub view: u64,
    /// The layout the image is in now, as the adapter tracks it.
    pub layout: u32,
}

struct Buffer {
    handle: BufferHandle,
    allocation: Option<Allocation>,
    size: u32,
}

struct Texture {
    image: ImageHandle,
    view: ImageViewHandle,
    allocation: Option<Allocation>,
    desc: TextureDesc,
    layout: u32,
    /// Whether the image can be read back (always for colour; for depth, if the format allows).
    readable: bool,
}

/// The resources of one device.
pub struct Resources {
    core: Rc<Core>,
    fns: ResourceFns,
    device: crate::vk::DeviceHandle,
    memory: MemoryManager,
    uploader: Uploader,
    buffers: Slab<Buffer>,
    textures: Slab<Texture>,
    samplers: Slab<SamplerHandle>,
    max_extent: u32,
    depth_usable: bool,
    depth_readable: bool,
}

/// How many mip levels a chain of an image of this size has.
fn mip_limit(width: u32, height: u32) -> u32 {
    32 - width.max(height).leading_zeros()
}

/// Checks a texture description against the device limit.
fn check_texture(desc: &TextureDesc, max_extent: u32) -> Result<(), VulkanError> {
    if desc.width == 0 || desc.height == 0 {
        return Err(VulkanError::Invalid("a texture needs a width and a height"));
    }
    if desc.width > max_extent || desc.height > max_extent {
        return Err(VulkanError::Invalid(
            "the texture is larger than the device allows",
        ));
    }
    if desc.mip_count == 0 || desc.mip_count > mip_limit(desc.width, desc.height) {
        return Err(VulkanError::Invalid("the mip count does not fit the size"));
    }
    Ok(())
}

/// Checks that `region` lies inside a `width` by `height` texture and that `length` bytes are
/// exactly its texels.
fn check_region(width: u32, height: u32, region: Region, length: usize) -> Result<(), VulkanError> {
    let (Ok(x), Ok(y)) = (u32::try_from(region.x), u32::try_from(region.y)) else {
        return Err(VulkanError::Invalid("the region starts before the texture"));
    };
    let inside = x.checked_add(region.width).is_some_and(|end| end <= width)
        && y.checked_add(region.height)
            .is_some_and(|end| end <= height);
    if !inside {
        return Err(VulkanError::Invalid("the region leaves the texture"));
    }
    let texels = u64::from(region.width) * u64::from(region.height);
    let expected = texels * TextureFormat::BYTES_PER_TEXEL;
    if u64::try_from(length).ok() != Some(expected) {
        return Err(VulkanError::Invalid(
            "the byte count does not match the region",
        ));
    }
    Ok(())
}

impl Resources {
    /// Makes the tables, the memory manager and the uploader on `core`'s device.
    ///
    /// # Errors
    /// [`VulkanError::MissingFunction`] or [`VulkanError::Call`] while setting up.
    pub(crate) fn new(core: &Rc<Core>, block_size: u64) -> Result<Self, VulkanError> {
        let fns = load_resource_fns(core)?;
        let depth = core
            .instance
            .format_properties(core.gpu, FORMAT_D32_SFLOAT)
            .optimal;
        let memory = MemoryManager::new(core, block_size)?;
        let uploader = Uploader::new(core, fns)?;
        Ok(Self {
            core: Rc::clone(core),
            fns,
            device: core.device.handle(),
            memory,
            uploader,
            buffers: Slab::new(),
            textures: Slab::new(),
            samplers: Slab::new(),
            max_extent: core.max_texture_extent,
            depth_usable: depth & FEATURE_DEPTH_ATTACHMENT != 0,
            depth_readable: depth & FEATURE_TRANSFER_SRC != 0,
        })
    }

    /// The default block size of the memory manager.
    pub(crate) const DEFAULT_BLOCK: u64 = BLOCK_SIZE;

    /// Waits until the device has finished everything submitted to it.
    ///
    /// # Errors
    /// [`VulkanError::Call`] with the driver result.
    pub fn wait_idle(&self) -> Result<(), VulkanError> {
        self.core.device.wait_idle()
    }

    /// How many buffers, textures and samplers are alive.
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.buffers.len(), self.textures.len(), self.samplers.len())
    }

    /// How many device-memory blocks the manager holds.
    #[must_use]
    pub fn memory_blocks(&self) -> usize {
        self.memory.block_count()
    }

    // ----- buffers -----

    /// Makes a device-local buffer.
    ///
    /// # Errors
    /// [`VulkanError::Invalid`] for a size of zero; [`VulkanError::Call`] when the driver
    /// refuses; [`VulkanError::Unsupported`] when no memory fits.
    pub fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId, VulkanError> {
        if desc.size == 0 {
            return Err(VulkanError::Invalid("a buffer needs a size"));
        }
        let mut buffer = Buffer {
            handle: 0,
            allocation: None,
            size: desc.size,
        };
        if let Err(error) = self.build_buffer(&mut buffer, desc) {
            self.release_buffer(buffer);
            return Err(error);
        }
        match self.buffers.insert(buffer) {
            Ok(key) => Ok(BufferId(key)),
            Err(buffer) => {
                self.release_buffer(buffer);
                Err(VulkanError::Invalid("too many buffers"))
            }
        }
    }

    fn build_buffer(&mut self, buffer: &mut Buffer, desc: BufferDesc) -> Result<(), VulkanError> {
        let usage = match desc.usage {
            BufferUsage::Vertex => BUFFER_USAGE_VERTEX,
            BufferUsage::Index => BUFFER_USAGE_INDEX,
            BufferUsage::Uniform => BUFFER_USAGE_UNIFORM,
        };
        let info = BufferCreateInfo {
            s_type: STYPE_BUFFER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            size: u64::from(desc.size),
            usage: usage | BUFFER_USAGE_TRANSFER_DST,
            sharing_mode: SHARING_EXCLUSIVE,
            queue_family_index_count: 0,
            queue_family_indices: core::ptr::null(),
        };
        let mut handle: BufferHandle = 0;
        // SAFETY: `info` is valid for the call; `handle` is a valid output.
        let result = unsafe {
            (self.fns.create_buffer)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut handle,
            )
        };
        check_created("vkCreateBuffer", result, handle == 0)?;
        buffer.handle = handle;
        let mut required = MemoryRequirements::default();
        // SAFETY: the buffer is live; `required` is a valid output.
        unsafe {
            (self.fns.get_buffer_memory_requirements)(self.device, handle, &raw mut required);
        }
        let allocation = self
            .memory
            .allocate(&required, Kind::Linear, Placement::DeviceLocal)?;
        let (memory, offset) = (allocation.memory, allocation.offset);
        buffer.allocation = Some(allocation);
        // SAFETY: the buffer and the memory are live and the buffer is not bound yet.
        ok("vkBindBufferMemory", unsafe {
            (self.fns.bind_buffer_memory)(self.device, handle, memory, offset)
        })
    }

    fn release_buffer(&mut self, buffer: Buffer) {
        if buffer.handle != 0 {
            // SAFETY: made on this device and destroyed once, here, before its memory is freed.
            unsafe { (self.fns.destroy_buffer)(self.device, buffer.handle, core::ptr::null()) };
        }
        if let Some(allocation) = buffer.allocation {
            self.memory.free(allocation);
        }
    }

    /// Copies `bytes` into a buffer at `offset`; the data is there when this returns.
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`]; [`VulkanError::Invalid`] when the range leaves the buffer;
    /// [`VulkanError::Call`] when the transfer fails.
    pub fn write_buffer(
        &mut self,
        id: BufferId,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), VulkanError> {
        let buffer = self.buffers.get(id.0).ok_or(VulkanError::InvalidHandle)?;
        let (target, size) = (buffer.handle, u64::from(buffer.size));
        let length = u64::try_from(bytes.len()).map_err(|_| VulkanError::Invalid("too large"))?;
        if u64::from(offset) + length > size {
            return Err(VulkanError::Invalid("the range leaves the buffer"));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let source = self.uploader.stage(&mut self.memory, bytes)?;
        let region = BufferCopy {
            src_offset: 0,
            dst_offset: u64::from(offset),
            size: length,
        };
        let written: StageAccess = (STAGE_ALL_TRANSFER, ACCESS_TRANSFER_WRITE);
        self.uploader.submit(|recorder| {
            recorder.memory_barrier(EVERYTHING, written);
            recorder.copy_buffer(source, target, region);
            recorder.memory_barrier(written, EVERYTHING);
        })
    }

    /// Destroys a buffer; a stale id is ignored.
    pub fn destroy_buffer(&mut self, id: BufferId) {
        if let Some(buffer) = self.buffers.remove(id.0) {
            self.release_buffer(buffer);
        }
    }

    /// The `VkBuffer` of a buffer, for the code that draws with it.
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`].
    pub fn buffer_handle(&self, id: BufferId) -> Result<u64, VulkanError> {
        self.buffers
            .get(id.0)
            .map(|buffer| buffer.handle)
            .ok_or(VulkanError::InvalidHandle)
    }

    // ----- textures -----

    /// Makes a device-local 2D texture with a view of all its levels.
    ///
    /// # Errors
    /// [`VulkanError::Invalid`] for a size or mip count that does not fit;
    /// [`VulkanError::Call`] when the driver refuses; [`VulkanError::Unsupported`] when no memory
    /// fits.
    pub fn create_texture(&mut self, desc: TextureDesc) -> Result<TextureId, VulkanError> {
        check_texture(&desc, self.max_extent)?;
        if desc.format == TextureFormat::Depth32F && !self.depth_usable {
            return Err(VulkanError::Unsupported("a 32-bit float depth attachment"));
        }
        let mut texture = Texture {
            image: 0,
            view: 0,
            allocation: None,
            desc,
            layout: LAYOUT_UNDEFINED,
            readable: desc.format != TextureFormat::Depth32F || self.depth_readable,
        };
        if let Err(error) = self.build_texture(&mut texture) {
            self.release_texture(texture);
            return Err(error);
        }
        match self.textures.insert(texture) {
            Ok(key) => Ok(TextureId(key)),
            Err(texture) => {
                self.release_texture(texture);
                Err(VulkanError::Invalid("too many textures"))
            }
        }
    }

    fn build_texture(&mut self, texture: &mut Texture) -> Result<(), VulkanError> {
        let desc = texture.desc;
        let usage = match desc.format {
            TextureFormat::Rgba8 | TextureFormat::Bgra8 => {
                IMAGE_USAGE_SAMPLED
                    | IMAGE_USAGE_TRANSFER_DST
                    | IMAGE_USAGE_TRANSFER_SRC
                    | IMAGE_USAGE_COLOR_ATTACHMENT
            }
            TextureFormat::Depth32F if texture.readable => {
                IMAGE_USAGE_DEPTH_ATTACHMENT | IMAGE_USAGE_TRANSFER_SRC
            }
            TextureFormat::Depth32F => IMAGE_USAGE_DEPTH_ATTACHMENT,
        };
        let info = ImageCreateInfo {
            s_type: STYPE_IMAGE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            image_type: IMAGE_TYPE_2D,
            format: desc.format.vk(),
            extent: [desc.width, desc.height, 1],
            mip_levels: desc.mip_count,
            array_layers: 1,
            samples: SAMPLES_1,
            tiling: TILING_OPTIMAL,
            usage,
            sharing_mode: SHARING_EXCLUSIVE,
            queue_family_index_count: 0,
            queue_family_indices: core::ptr::null(),
            initial_layout: LAYOUT_UNDEFINED,
        };
        let mut image: ImageHandle = 0;
        // SAFETY: `info` is valid for the call; `image` is a valid output.
        let result = unsafe {
            (self.fns.create_image)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut image,
            )
        };
        check_created("vkCreateImage", result, image == 0)?;
        texture.image = image;
        self.bind_texture_memory(texture)?;
        texture.view = self.make_view(texture)?;
        Ok(())
    }

    fn bind_texture_memory(&mut self, texture: &mut Texture) -> Result<(), VulkanError> {
        let mut required = MemoryRequirements::default();
        // SAFETY: the image is live; `required` is a valid output.
        unsafe {
            (self.fns.get_image_memory_requirements)(self.device, texture.image, &raw mut required);
        }
        let allocation = self
            .memory
            .allocate(&required, Kind::Optimal, Placement::DeviceLocal)?;
        let (memory, offset) = (allocation.memory, allocation.offset);
        texture.allocation = Some(allocation);
        // SAFETY: the image and the memory are live and the image is not bound yet.
        ok("vkBindImageMemory", unsafe {
            (self.fns.bind_image_memory)(self.device, texture.image, memory, offset)
        })
    }

    fn make_view(&self, texture: &Texture) -> Result<ImageViewHandle, VulkanError> {
        let info = ImageViewCreateInfo {
            s_type: STYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            image: texture.image,
            view_type: IMAGE_VIEW_TYPE_2D,
            format: texture.desc.format.vk(),
            components: [0; 4],
            subresource_range: SubresourceRange::all_levels(
                texture.desc.format.aspect(),
                texture.desc.mip_count,
            ),
        };
        let mut view: ImageViewHandle = 0;
        // SAFETY: `info` is valid for the call; `view` is a valid output.
        let result = unsafe {
            (self.fns.create_image_view)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut view,
            )
        };
        check_created("vkCreateImageView", result, view == 0)?;
        Ok(view)
    }

    fn release_texture(&mut self, texture: Texture) {
        if texture.view != 0 {
            // SAFETY: made on this device and destroyed once, here, before its image.
            unsafe { (self.fns.destroy_image_view)(self.device, texture.view, core::ptr::null()) };
        }
        if texture.image != 0 {
            // SAFETY: made on this device and destroyed once, here, before its memory is freed.
            unsafe { (self.fns.destroy_image)(self.device, texture.image, core::ptr::null()) };
        }
        if let Some(allocation) = texture.allocation {
            self.memory.free(allocation);
        }
    }

    /// Copies tightly packed texels into a region of mip level 0 and leaves the texture ready to
    /// be sampled; the data is there when this returns. The other mip levels are not written.
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`]; [`VulkanError::Invalid`] when the region or the byte count
    /// does not fit; [`VulkanError::Unsupported`] for a depth texture; [`VulkanError::Call`]
    /// when the transfer fails.
    pub fn write_texture(
        &mut self,
        id: TextureId,
        region: Region,
        bytes: &[u8],
    ) -> Result<(), VulkanError> {
        let texture = self.textures.get(id.0).ok_or(VulkanError::InvalidHandle)?;
        if texture.desc.format == TextureFormat::Depth32F {
            return Err(VulkanError::Unsupported(
                "writing texels of a depth texture",
            ));
        }
        check_region(texture.desc.width, texture.desc.height, region, bytes.len())?;
        if bytes.is_empty() {
            return Ok(());
        }
        let (image, old, levels) = (texture.image, texture.layout, texture.desc.mip_count);
        let source = self.uploader.stage(&mut self.memory, bytes)?;
        let range = SubresourceRange::all_levels(ASPECT_COLOR, levels);
        let copy = tight_region(
            ASPECT_COLOR,
            region.x,
            region.y,
            region.width,
            region.height,
        );
        let written: StageAccess = (STAGE_ALL_TRANSFER, ACCESS_TRANSFER_WRITE);
        let before = if old == LAYOUT_UNDEFINED {
            NOTHING
        } else {
            EVERYTHING
        };
        let sent = self.uploader.submit(|recorder| {
            recorder.image_barrier(&ImageTransition {
                image,
                range,
                old_layout: old,
                new_layout: LAYOUT_TRANSFER_DST,
                src: before,
                dst: written,
            });
            recorder.copy_buffer_to_image(source, image, &copy);
            recorder.image_barrier(&ImageTransition {
                image,
                range,
                old_layout: LAYOUT_TRANSFER_DST,
                new_layout: LAYOUT_SHADER_READ,
                src: written,
                dst: EVERYTHING,
            });
        });
        self.settle(id, LAYOUT_SHADER_READ, sent)
    }

    /// Copies mip level 0 into `out`, tightly packed. The texture keeps the layout it had (a
    /// texture never written settles in the layout of its format).
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`]; [`VulkanError::Invalid`] when `out` has the wrong length;
    /// [`VulkanError::Unsupported`] for a depth format the device cannot copy from;
    /// [`VulkanError::Call`] when the transfer fails.
    pub fn read_back(&mut self, id: TextureId, out: &mut [u8]) -> Result<(), VulkanError> {
        let texture = self.textures.get(id.0).ok_or(VulkanError::InvalidHandle)?;
        let desc = texture.desc;
        let expected =
            u64::from(desc.width) * u64::from(desc.height) * TextureFormat::BYTES_PER_TEXEL;
        if u64::try_from(out.len()).ok() != Some(expected) {
            return Err(VulkanError::Invalid(
                "the output length does not match the texture",
            ));
        }
        if !texture.readable {
            return Err(VulkanError::Unsupported("reading back this texture format"));
        }
        let (image, old) = (texture.image, texture.layout);
        let back = if old == LAYOUT_UNDEFINED {
            desc.format.resting_layout()
        } else {
            old
        };
        let (target, base) = self.uploader.reserve(&mut self.memory, expected)?;
        let range = SubresourceRange::all_levels(desc.format.aspect(), desc.mip_count);
        let copy = tight_region(desc.format.aspect(), 0, 0, desc.width, desc.height);
        let read: StageAccess = (STAGE_ALL_TRANSFER, ACCESS_TRANSFER_READ);
        let before = if old == LAYOUT_UNDEFINED {
            NOTHING
        } else {
            EVERYTHING
        };
        let sent = self.uploader.submit(|recorder| {
            recorder.image_barrier(&ImageTransition {
                image,
                range,
                old_layout: old,
                new_layout: LAYOUT_TRANSFER_SRC,
                src: before,
                dst: read,
            });
            recorder.copy_image_to_buffer(image, target, &copy);
            recorder.memory_barrier(
                (STAGE_ALL_TRANSFER, ACCESS_TRANSFER_WRITE),
                (STAGE_HOST, ACCESS_HOST_READ),
            );
            recorder.image_barrier(&ImageTransition {
                image,
                range,
                old_layout: LAYOUT_TRANSFER_SRC,
                new_layout: back,
                src: read,
                dst: EVERYTHING,
            });
        });
        self.settle(id, back, sent)?;
        // SAFETY: the mapping holds at least `expected` bytes (reserve grew it), `out` has
        // exactly that length, and the transfer has finished (submit waited for its fence).
        unsafe { core::ptr::copy_nonoverlapping(base.as_ptr(), out.as_mut_ptr(), out.len()) };
        Ok(())
    }

    /// Destroys a texture; a stale id is ignored.
    pub fn destroy_texture(&mut self, id: TextureId) {
        if let Some(texture) = self.textures.remove(id.0) {
            self.release_texture(texture);
        }
    }

    /// The Vulkan objects of a texture and the layout it is in.
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`].
    pub fn texture_handles(&self, id: TextureId) -> Result<TextureHandles, VulkanError> {
        self.textures
            .get(id.0)
            .map(|texture| TextureHandles {
                image: texture.image,
                view: texture.view,
                layout: texture.layout,
            })
            .ok_or(VulkanError::InvalidHandle)
    }

    /// Records the layout a texture is in after commands the render device recorded itself.
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`].
    pub fn note_layout(&mut self, id: TextureId, layout: u32) -> Result<(), VulkanError> {
        let texture = self
            .textures
            .get_mut(id.0)
            .ok_or(VulkanError::InvalidHandle)?;
        texture.layout = layout;
        Ok(())
    }

    /// After a transfer: the texture is in `layout` whenever the GPU got the work, even when the
    /// wait for it failed, so the next barrier starts from the right layout.
    fn settle(
        &mut self,
        id: TextureId,
        layout: u32,
        sent: Result<(), VulkanError>,
    ) -> Result<(), VulkanError> {
        if sent.is_ok() || self.uploader.work_pending() {
            self.set_layout(id, layout);
        }
        sent
    }

    fn set_layout(&mut self, id: TextureId, layout: u32) {
        if let Some(texture) = self.textures.get_mut(id.0) {
            texture.layout = layout;
        }
    }

    // ----- samplers -----

    /// Makes a sampler.
    ///
    /// # Errors
    /// [`VulkanError::Call`] when the driver refuses.
    pub fn create_sampler(&mut self, desc: SamplerDesc) -> Result<SamplerId, VulkanError> {
        let filter = |f: Filter| match f {
            Filter::Nearest => FILTER_NEAREST,
            Filter::Linear => FILTER_LINEAR,
        };
        let address = match desc.address {
            AddressMode::Clamp => ADDRESS_CLAMP_TO_EDGE,
            AddressMode::Repeat => ADDRESS_REPEAT,
        };
        let info = SamplerCreateInfo {
            s_type: STYPE_SAMPLER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            mag_filter: filter(desc.mag_filter),
            min_filter: filter(desc.min_filter),
            mipmap_mode: if desc.min_filter == Filter::Linear {
                MIPMAP_LINEAR
            } else {
                MIPMAP_NEAREST
            },
            address_mode_u: address,
            address_mode_v: address,
            address_mode_w: address,
            mip_lod_bias: 0.0,
            anisotropy_enable: 0,
            max_anisotropy: 1.0,
            compare_enable: 0,
            compare_op: 0,
            min_lod: 0.0,
            max_lod: LOD_CLAMP_NONE,
            border_color: 0,
            unnormalized_coordinates: 0,
        };
        let mut handle: SamplerHandle = 0;
        // SAFETY: `info` is valid for the call; `handle` is a valid output.
        let result = unsafe {
            (self.fns.create_sampler)(
                self.device,
                &raw const info,
                core::ptr::null(),
                &raw mut handle,
            )
        };
        check_created("vkCreateSampler", result, handle == 0)?;
        match self.samplers.insert(handle) {
            Ok(key) => Ok(SamplerId(key)),
            Err(handle) => {
                // SAFETY: made above on this device and never shared.
                unsafe { (self.fns.destroy_sampler)(self.device, handle, core::ptr::null()) };
                Err(VulkanError::Invalid("too many samplers"))
            }
        }
    }

    /// Destroys a sampler; a stale id is ignored.
    pub fn destroy_sampler(&mut self, id: SamplerId) {
        if let Some(handle) = self.samplers.remove(id.0) {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (self.fns.destroy_sampler)(self.device, handle, core::ptr::null()) };
        }
    }

    /// The `VkSampler` of a sampler.
    ///
    /// # Errors
    /// [`VulkanError::InvalidHandle`].
    pub fn sampler_handle(&self, id: SamplerId) -> Result<u64, VulkanError> {
        self.samplers
            .get(id.0)
            .copied()
            .ok_or(VulkanError::InvalidHandle)
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        // Nothing may still be using what is freed below.
        let _ = self.core.device.wait_idle();
        for sampler in self.samplers.drain() {
            // SAFETY: made on this device and destroyed once, here.
            unsafe { (self.fns.destroy_sampler)(self.device, sampler, core::ptr::null()) };
        }
        for texture in self.textures.drain() {
            self.release_texture(texture);
        }
        for buffer in self.buffers.drain() {
            self.release_buffer(buffer);
        }
        self.uploader.destroy(&mut self.memory);
        self.memory.release_all();
    }
}

#[cfg(test)]
mod tests;
