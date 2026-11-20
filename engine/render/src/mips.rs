//! Mip levels of a texture as plain functions (card 8-35): the size of a level, the length of the
//! full chain, and a box-filtered chain for 8-bit RGBA texels made on the CPU. The port does not
//! make levels (`IRenderDevice::write_texture_level` takes them one by one, the shape of Vulkan
//! and D3D12); this is the helper a sample or a loader uses when its texture has only level 0. The
//! size rule is the one of the `.dtex` texture format.

/// Bytes per texel of the 8-bit formats.
const TEXEL: usize = 4;

/// Width and height of mip level `level` of a `width` by `height` texture: half of the level above
/// (rounded down), never below one texel.
#[must_use]
pub fn level_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    let shrink = |side: u32| side.checked_shr(level).unwrap_or(0).max(1);
    (shrink(width), shrink(height))
}

/// Number of levels of the full chain, down to one texel (at least 1).
#[must_use]
pub fn level_count(width: u32, height: u32) -> u32 {
    u32::BITS - width.max(height).leading_zeros().min(u32::BITS - 1)
}

/// The next level of 8-bit RGBA `texels` (`width` by `height`): each texel is the average of a 2x2
/// block of the level above, channel by channel and rounded down (a side of one texel is not
/// halved; of an odd side above one, the last row or column is left out), with no gamma curve applied. `None` for a size of zero, one texel
/// (it has no next level) or a byte count that is not `width * height * 4`.
#[must_use]
pub fn downsample(texels: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let (w, h) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    if w == 0
        || h == 0
        || (w == 1 && h == 1)
        || texels.len() != w.checked_mul(h)?.checked_mul(TEXEL)?
    {
        return None;
    }
    let (next_w, next_h) = level_size(width, height, 1);
    let (next_w, next_h) = (usize::try_from(next_w).ok()?, usize::try_from(next_h).ok()?);
    let mut next = Vec::with_capacity(next_w * next_h * TEXEL);
    for y in 0..next_h {
        for x in 0..next_w {
            next.extend_from_slice(&average_block(texels, w, h, x, y));
        }
    }
    Some(next)
}

/// Adds the channels of one texel to `sum`.
fn add_texel(sum: &mut [u32; TEXEL], texel: &[u8]) {
    for (total, channel) in sum.iter_mut().zip(texel) {
        *total += u32::from(*channel);
    }
}

/// The average of the (up to) 2x2 texels of the level above that make texel `(x, y)`.
fn average_block(texels: &[u8], w: usize, h: usize, x: usize, y: usize) -> [u8; TEXEL] {
    let mut sum = [0_u32; TEXEL];
    let mut count = 0_u32;
    for sy in (2 * y)..(2 * y + 2).min(h) {
        for sx in (2 * x)..(2 * x + 2).min(w) {
            add_texel(&mut sum, &texels[(sy * w + sx) * TEXEL..][..TEXEL]);
            count += 1;
        }
    }
    sum.map(|total| u8::try_from(total / count).unwrap_or(u8::MAX))
}

/// Every level of the full chain from `texels` (level 0, `width` by `height`, 8-bit RGBA): the
/// first element is a copy of the input. `None` when `texels` is not `width * height * 4` bytes or
/// a size is zero.
#[must_use]
pub fn mip_chain(texels: &[u8], width: u32, height: u32) -> Option<Vec<Vec<u8>>> {
    let expected = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(TEXEL)?;
    if width == 0 || height == 0 || texels.len() != expected {
        return None;
    }
    let mut chain = vec![texels.to_vec()];
    let (mut w, mut h) = (width, height);
    while (w, h) != (1, 1) {
        let next = downsample(chain.last()?, w, h)?;
        (w, h) = level_size(w, h, 1);
        chain.push(next);
    }
    Some(chain)
}

#[cfg(test)]
mod tests;
