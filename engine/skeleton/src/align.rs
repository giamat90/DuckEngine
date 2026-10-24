//! Alignment arithmetic.

/// Rounds `value` up to the next multiple of `align`.
///
/// Returns `None` when `align` is zero or not a power of two, or when the
/// result does not fit in a `u64`.
#[must_use]
pub fn align_up(value: u64, align: u64) -> Option<u64> {
    if !align.is_power_of_two() {
        return None;
    }
    let mask = align - 1;
    value.checked_add(mask).map(|padded| padded & !mask)
}

#[cfg(test)]
mod tests {
    use super::align_up;

    #[test]
    fn aligned_value_is_unchanged() {
        assert_eq!(align_up(16, 8), Some(16));
    }

    #[test]
    fn unaligned_value_rounds_up_to_next_multiple() {
        assert_eq!(align_up(17, 8), Some(24));
    }

    #[test]
    fn zero_stays_zero() {
        assert_eq!(align_up(0, 64), Some(0));
    }

    #[test]
    fn alignment_of_one_is_identity() {
        assert_eq!(align_up(13, 1), Some(13));
    }

    #[test]
    fn zero_alignment_is_rejected() {
        assert_eq!(align_up(5, 0), None);
    }

    #[test]
    fn non_power_of_two_alignment_is_rejected() {
        assert_eq!(align_up(5, 6), None);
    }

    #[test]
    fn overflow_is_rejected() {
        assert_eq!(align_up(u64::MAX, 2), None);
    }

    #[test]
    fn maximum_value_with_alignment_of_one_fits() {
        assert_eq!(align_up(u64::MAX, 1), Some(u64::MAX));
    }
}
