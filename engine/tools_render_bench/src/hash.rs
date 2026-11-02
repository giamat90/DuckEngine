//! A small own hash for pinning the bytes of a frame in tests (FNV-1a, 64 bit).

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64-bit hash of `bytes`.
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_input_hashes_to_the_offset() {
        assert_eq!(fnv1a(&[]), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn known_fnv1a_vectors() {
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_changed_byte_changes_the_hash() {
        assert_ne!(fnv1a(&[1, 2, 3]), fnv1a(&[1, 2, 4]));
    }
}
