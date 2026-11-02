//! The `dtex` v0 runtime texture format and its reader (design: `wiki/architecture/render-port-v0.md`
//! section 8).
//!
//! Layout, all integers little-endian:
//!
//! | offset | size | field |
//! |---|---|---|
//! | 0 | 4 | magic `DTEX` |
//! | 4 | 2 | version (0) |
//! | 6 | 1 | pixel format (0 = RGBA8) |
//! | 7 | 1 | flags (bit 0 = sRGB, the rest reserved, must be 0) |
//! | 8 | 4 | width in pixels |
//! | 12 | 4 | height in pixels |
//! | 16 | 4 | mip count (1 up to the full chain) |
//! | 20 | ... | the pixels of mip 0, then mip 1, and so on, tightly packed, no padding |
//!
//! Mip `n` is `max(width >> n, 1)` by `max(height >> n, 1)` pixels of 4 bytes. The file length must
//! equal the header plus all mips exactly. The reader borrows the bytes and never allocates.

use core::fmt;

/// Size of the fixed header in bytes.
pub const HEADER_LEN: usize = 20;
/// Largest width or height accepted, in pixels.
pub const MAX_DIMENSION: u32 = 16_384;

const MAGIC: [u8; 4] = *b"DTEX";
const VERSION: u16 = 0;
const FORMAT_RGBA8: u8 = 0;
const FLAG_SRGB: u8 = 1;
const BYTES_PER_PIXEL: usize = 4;

/// Why a file was refused, or an encode call was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DtexError {
    /// Fewer bytes than the header or the mips need.
    Truncated,
    /// The first four bytes are not `DTEX`.
    BadMagic,
    /// A version this reader does not know.
    UnsupportedVersion(u16),
    /// A pixel format this reader does not know.
    UnsupportedFormat(u8),
    /// A reserved flag bit is set.
    ReservedFlags,
    /// Width or height is zero or above [`MAX_DIMENSION`].
    BadDimensions,
    /// The mip count is zero or longer than the full chain.
    BadMipCount,
    /// A mip given to the encoder has the wrong byte length.
    BadMipSize,
    /// Bytes remain after the last mip.
    TrailingData,
}

impl fmt::Display for DtexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated"),
            Self::BadMagic => f.write_str("bad magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version {v}"),
            Self::UnsupportedFormat(p) => write!(f, "unsupported pixel format {p}"),
            Self::ReservedFlags => f.write_str("reserved flag bits set"),
            Self::BadDimensions => f.write_str("bad dimensions"),
            Self::BadMipCount => f.write_str("bad mip count"),
            Self::BadMipSize => f.write_str("bad mip size"),
            Self::TrailingData => f.write_str("trailing data"),
        }
    }
}

impl std::error::Error for DtexError {}

/// What the encoder needs to know about a texture besides its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureSpec {
    /// Width of mip 0 in pixels.
    pub width: u32,
    /// Height of mip 0 in pixels.
    pub height: u32,
    /// Whether the colour channels are sRGB encoded.
    pub srgb: bool,
}

/// One mip level, borrowed from the file bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mip<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixels, row by row from the top, `width * height * 4` bytes.
    pub bytes: &'a [u8],
}

/// A validated view over a `dtex` file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Texture<'a> {
    spec: TextureSpec,
    mip_count: u32,
    data: &'a [u8],
}

/// Width and height of mip `level` (never below one pixel).
#[must_use]
pub fn mip_dimensions(width: u32, height: u32, level: u32) -> (u32, u32) {
    let shrink = |v: u32| v.checked_shr(level).unwrap_or(0).max(1);
    (shrink(width), shrink(height))
}

/// Length of the full mip chain of a `width` by `height` texture.
#[must_use]
pub fn max_mip_count(width: u32, height: u32) -> u32 {
    u32::BITS - width.max(height).leading_zeros()
}

fn mip_len(width: u32, height: u32, level: u32) -> Option<usize> {
    let (w, h) = mip_dimensions(width, height, level);
    usize::try_from(w)
        .ok()?
        .checked_mul(usize::try_from(h).ok()?)?
        .checked_mul(BYTES_PER_PIXEL)
}

fn check(spec: TextureSpec, mip_count: u32) -> Result<(), DtexError> {
    let in_range = |v: u32| (1..=MAX_DIMENSION).contains(&v);
    if !in_range(spec.width) || !in_range(spec.height) {
        return Err(DtexError::BadDimensions);
    }
    if mip_count == 0 || mip_count > max_mip_count(spec.width, spec.height) {
        return Err(DtexError::BadMipCount);
    }
    Ok(())
}

fn data_len(spec: TextureSpec, mip_count: u32) -> Result<usize, DtexError> {
    (0..mip_count)
        .try_fold(0usize, |sum, level| {
            sum.checked_add(mip_len(spec.width, spec.height, level)?)
        })
        .ok_or(DtexError::BadDimensions)
}

/// Total file length for a texture of `spec` with `mip_count` mips.
///
/// # Errors
/// [`DtexError::BadDimensions`] or [`DtexError::BadMipCount`] for arguments the format cannot hold.
pub fn encoded_len(spec: TextureSpec, mip_count: u32) -> Result<usize, DtexError> {
    check(spec, mip_count)?;
    Ok(HEADER_LEN + data_len(spec, mip_count)?)
}

fn header(spec: TextureSpec, mip_count: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(&MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = FORMAT_RGBA8;
    h[7] = if spec.srgb { FLAG_SRGB } else { 0 };
    h[8..12].copy_from_slice(&spec.width.to_le_bytes());
    h[12..16].copy_from_slice(&spec.height.to_le_bytes());
    h[16..20].copy_from_slice(&mip_count.to_le_bytes());
    h
}

/// Writes the file for `spec` and the given mips (mip 0 first) into `out`; returns the bytes
/// written. Never allocates.
///
/// # Errors
/// The arguments are checked like [`encoded_len`]; [`DtexError::BadMipSize`] when a mip has the
/// wrong length, [`DtexError::Truncated`] when `out` is too small.
pub fn encode_into(out: &mut [u8], spec: TextureSpec, mips: &[&[u8]]) -> Result<usize, DtexError> {
    let mip_count = u32::try_from(mips.len()).map_err(|_| DtexError::BadMipCount)?;
    let total = encoded_len(spec, mip_count)?;
    for (level, mip) in (0..mip_count).zip(mips) {
        if mip_len(spec.width, spec.height, level) != Some(mip.len()) {
            return Err(DtexError::BadMipSize);
        }
    }
    let dst = out.get_mut(..total).ok_or(DtexError::Truncated)?;
    let (head, mut rest) = dst.split_at_mut(HEADER_LEN);
    head.copy_from_slice(&header(spec, mip_count));
    for mip in mips {
        let (here, after) = rest.split_at_mut(mip.len());
        here.copy_from_slice(mip);
        rest = after;
    }
    Ok(total)
}

fn le_u32(h: &[u8; HEADER_LEN], at: usize) -> u32 {
    u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]])
}

impl<'a> Texture<'a> {
    /// Validates `bytes` completely (header, sizes against the length) and returns a view.
    ///
    /// # Errors
    /// The first problem found, see [`DtexError`]. Hostile input yields an error, never a panic.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, DtexError> {
        let head = bytes
            .first_chunk::<HEADER_LEN>()
            .ok_or(DtexError::Truncated)?;
        if head[0..4] != MAGIC {
            return Err(DtexError::BadMagic);
        }
        let version = u16::from_le_bytes([head[4], head[5]]);
        if version != VERSION {
            return Err(DtexError::UnsupportedVersion(version));
        }
        if head[6] != FORMAT_RGBA8 {
            return Err(DtexError::UnsupportedFormat(head[6]));
        }
        if head[7] & !FLAG_SRGB != 0 {
            return Err(DtexError::ReservedFlags);
        }
        let spec = TextureSpec {
            width: le_u32(head, 8),
            height: le_u32(head, 12),
            srgb: head[7] & FLAG_SRGB != 0,
        };
        let mip_count = le_u32(head, 16);
        check(spec, mip_count)?;
        let data = bytes.get(HEADER_LEN..).ok_or(DtexError::Truncated)?;
        let expected = data_len(spec, mip_count)?;
        match data.len().cmp(&expected) {
            core::cmp::Ordering::Less => Err(DtexError::Truncated),
            core::cmp::Ordering::Greater => Err(DtexError::TrailingData),
            core::cmp::Ordering::Equal => Ok(Self {
                spec,
                mip_count,
                data,
            }),
        }
    }

    /// Width of mip 0.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.spec.width
    }

    /// Height of mip 0.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.spec.height
    }

    /// Whether the colour channels are sRGB encoded.
    #[must_use]
    pub fn is_srgb(&self) -> bool {
        self.spec.srgb
    }

    /// Number of mip levels (at least one).
    #[must_use]
    pub fn mip_count(&self) -> u32 {
        self.mip_count
    }

    /// The pixels of mip `level`, or `None` past the last level.
    #[must_use]
    pub fn mip(&self, level: u32) -> Option<Mip<'a>> {
        if level >= self.mip_count {
            return None;
        }
        let start = (0..level).try_fold(0usize, |sum, l| {
            sum.checked_add(mip_len(self.spec.width, self.spec.height, l)?)
        })?;
        let len = mip_len(self.spec.width, self.spec.height, level)?;
        let (width, height) = mip_dimensions(self.spec.width, self.spec.height, level);
        Some(Mip {
            width,
            height,
            bytes: self.data.get(start..start.checked_add(len)?)?,
        })
    }
}

#[cfg(test)]
mod tests;
