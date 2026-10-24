//! Unaligned little-endian reads from a byte slice.
#![allow(unsafe_code)]

/// Reads a little-endian `u32` at `offset`, or `None` when fewer than four
/// bytes remain there.
#[must_use]
pub fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(WIDTH)?;
    if end > bytes.len() {
        return None;
    }
    // SAFETY: `offset + WIDTH <= bytes.len()` was checked above, so the
    // `WIDTH` bytes starting at `offset` are inside `bytes`.
    Some(unsafe { read_u32_le_unchecked(bytes, offset) })
}

/// Width in bytes of the value read.
const WIDTH: usize = size_of::<u32>();

/// Reads a little-endian `u32` at `offset` without a bounds check.
///
/// # Safety
///
/// `offset + WIDTH` must not exceed `bytes.len()`.
unsafe fn read_u32_le_unchecked(bytes: &[u8], offset: usize) -> u32 {
    // SAFETY: the caller guarantees `WIDTH` readable bytes at `offset`; the
    // read is unaligned, so no alignment is required of the pointer.
    let raw = unsafe { bytes.as_ptr().add(offset).cast::<u32>().read_unaligned() };
    u32::from_le(raw)
}

#[cfg(test)]
mod tests {
    use super::read_u32_le;

    const BYTES: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

    #[test]
    fn reads_little_endian_at_start() {
        assert_eq!(read_u32_le(&BYTES, 0), Some(0x0403_0201));
    }

    #[test]
    fn reads_at_an_unaligned_offset() {
        assert_eq!(read_u32_le(&BYTES, 1), Some(0x0504_0302));
    }

    #[test]
    fn reads_at_the_last_valid_offset() {
        assert_eq!(read_u32_le(&BYTES, 4), Some(0x0807_0605));
    }

    #[test]
    fn one_past_the_last_valid_offset_is_none() {
        assert_eq!(read_u32_le(&BYTES, 5), None);
    }

    #[test]
    fn offset_at_the_end_is_none() {
        assert_eq!(read_u32_le(&BYTES, 8), None);
    }

    #[test]
    fn offset_past_the_end_is_none() {
        assert_eq!(read_u32_le(&BYTES, 100), None);
    }

    #[test]
    fn offset_that_overflows_is_none() {
        assert_eq!(read_u32_le(&BYTES, usize::MAX), None);
    }

    #[test]
    fn empty_slice_is_none() {
        assert_eq!(read_u32_le(&[], 0), None);
    }

    #[test]
    fn slice_of_exactly_four_bytes_reads() {
        assert_eq!(read_u32_le(&[0xff, 0, 0, 0x80], 0), Some(0x8000_00ff));
    }
}
