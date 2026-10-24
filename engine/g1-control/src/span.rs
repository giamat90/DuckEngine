//! Interval arithmetic with one happy-path test.

/// Length of the half-open interval `[start, end)`; zero when `end <= start`.
#[must_use]
pub fn length(start: u32, end: u32) -> u32 {
    end.saturating_sub(start)
}

/// Whether `value` lies in the half-open interval `[start, end)`.
#[must_use]
pub fn contains(start: u32, end: u32, value: u32) -> bool {
    value >= start && value < end
}

/// Whether the two half-open intervals share at least one point.
#[must_use]
pub fn overlaps(a_start: u32, a_end: u32, b_start: u32, b_end: u32) -> bool {
    a_start < b_end && b_start < a_end
}

/// Clamps `value` into `[low, high]`.
#[must_use]
pub fn clamp(value: u32, low: u32, high: u32) -> u32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::length;

    #[test]
    fn length_of_a_normal_interval() {
        assert_eq!(length(2, 7), 5);
    }
}
