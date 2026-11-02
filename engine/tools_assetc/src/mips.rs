//! Mip chain generation: a 2x2 box filter on the stored RGBA values. v0 averages the encoded
//! values (also for sRGB textures; gamma-correct averaging is a later refinement) and treats alpha
//! as straight, not premultiplied, so colour at low alpha bleeds into the mean. For an odd source
//! size the last column or row is not read: level sizes are floor-sized, so a 5x3 image gives 2x1
//! from columns 0 to 3 and rows 0 to 1.

use assets::dtex::mip_dimensions;

fn index(width: u32, x: u32, y: u32) -> usize {
    let (w, x, y) = (u64::from(width), u64::from(x), u64::from(y));
    usize::try_from((y * w + x) * 4).unwrap_or(usize::MAX)
}

/// The RGBA pixel at `(x, y)`, clamped to the image; zero when the data is too short.
fn pixel(src: &[u8], (width, height): (u32, u32), x: u32, y: u32) -> [u32; 4] {
    let at = index(
        width,
        x.min(width.saturating_sub(1)),
        y.min(height.saturating_sub(1)),
    );
    let get = |i: usize| src.get(at.saturating_add(i)).copied().map_or(0, u32::from);
    [get(0), get(1), get(2), get(3)]
}

/// Halves `src` (`source` pixels) to `target` pixels. Each target pixel averages the 2x2 block
/// starting at twice its position, edges clamped, rounding to nearest.
#[must_use]
pub fn box_down(src: &[u8], source: (u32, u32), target: (u32, u32)) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..target.1 {
        for x in 0..target.0 {
            let blocks = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .map(|(dx, dy)| pixel(src, source, x * 2 + dx, y * 2 + dy));
            out.extend_from_slice(&average(&blocks));
        }
    }
    out
}

/// The per-channel mean of four pixels, rounded to nearest.
fn average(blocks: &[[u32; 4]; 4]) -> [u8; 4] {
    [0, 1, 2, 3].map(|channel| {
        let sum: u32 = blocks.iter().map(|p| p[channel]).sum();
        u8::try_from((sum + 2) / 4).unwrap_or(u8::MAX)
    })
}

/// All `count` levels, `base` first.
#[must_use]
pub fn build_chain(base: Vec<u8>, size: (u32, u32), count: u32) -> Vec<Vec<u8>> {
    let mut chain = vec![base];
    for level in 1..count {
        let from = mip_dimensions(size.0, size.1, level - 1);
        let to = mip_dimensions(size.0, size.1, level);
        let next = chain.last().map(|prev| box_down(prev, from, to));
        chain.extend(next);
    }
    chain
}

#[cfg(test)]
mod tests;
