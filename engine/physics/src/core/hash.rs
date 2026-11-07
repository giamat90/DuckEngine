//! The state hash: FNV-1a (64 bit) over the bytes of the values written, so two worlds that were
//! stepped from the same inputs can be compared with one number.
//!
//! Floats are hashed by their bit patterns (`to_bits`), little-endian, so `0.0` and `-0.0` differ
//! and the hash does not depend on the platform byte order. The hash is a replay check, not a
//! cryptographic one.

/// FNV-1a 64-bit state, fed value by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateHasher {
    state: u64,
}

/// The FNV-1a 64-bit offset basis.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// The FNV-1a 64-bit prime.
const PRIME: u64 = 0x0000_0100_0000_01b3;

impl StateHasher {
    /// A hasher that has seen no byte.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: OFFSET_BASIS,
        }
    }

    /// Mixes one byte in.
    pub fn write_u8(&mut self, byte: u8) {
        self.state = (self.state ^ u64::from(byte)).wrapping_mul(PRIME);
    }

    /// Mixes the four little-endian bytes of `value` in.
    pub fn write_u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.write_u8(byte);
        }
    }

    /// Mixes the eight little-endian bytes of `value` in.
    pub fn write_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.write_u8(byte);
        }
    }

    /// Mixes the bit pattern of `value` in.
    pub fn write_f32(&mut self, value: f32) {
        self.write_u32(value.to_bits());
    }

    /// The hash of everything written so far.
    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.state
    }
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

/// A value that can feed its bit pattern into a [`StateHasher`].
pub trait HashBits {
    /// Writes every field, in declaration order.
    fn write_bits(&self, hasher: &mut StateHasher);
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

    fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut hasher = StateHasher::new();
        for &byte in bytes {
            hasher.write_u8(byte);
        }
        hasher.finish()
    }

    #[test]
    fn empty_input_hashes_to_the_fnv_offset_basis() {
        assert_eq!(StateHasher::new().finish(), OFFSET_BASIS);
        assert_eq!(StateHasher::default().finish(), OFFSET_BASIS);
    }

    #[test]
    fn matches_the_published_fnv1a_64_test_vectors() {
        assert_eq!(hash_bytes(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hash_bytes(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_u32_is_hashed_as_four_little_endian_bytes() {
        let mut hasher = StateHasher::new();
        hasher.write_u32(0x0403_0201);
        assert_eq!(hasher.finish(), hash_bytes(&[1, 2, 3, 4]));
    }

    #[test]
    fn a_u64_is_hashed_as_eight_little_endian_bytes() {
        let mut hasher = StateHasher::new();
        hasher.write_u64(0x0807_0605_0403_0201);
        assert_eq!(hasher.finish(), hash_bytes(&[1, 2, 3, 4, 5, 6, 7, 8]));
    }

    #[test]
    fn a_float_is_hashed_by_its_bit_pattern() {
        let mut one = StateHasher::new();
        one.write_f32(1.0);
        let mut bits = StateHasher::new();
        bits.write_u32(1.0_f32.to_bits());
        assert_eq!(one.finish(), bits.finish());
    }

    #[test]
    fn positive_and_negative_zero_hash_differently() {
        let mut positive = StateHasher::new();
        positive.write_f32(0.0);
        let mut negative = StateHasher::new();
        negative.write_f32(-0.0);
        assert_ne!(positive.finish(), negative.finish());
    }

    #[test]
    fn the_order_of_writes_matters() {
        let mut ab = StateHasher::new();
        ab.write_u32(1);
        ab.write_u32(2);
        let mut ba = StateHasher::new();
        ba.write_u32(2);
        ba.write_u32(1);
        assert_ne!(ab.finish(), ba.finish());
    }
}
