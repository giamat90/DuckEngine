//! The numbers of a frame-time run (card 7-13): a list of frame times in nanoseconds summarised by
//! the count, the mean, the median, two high percentiles and the worst one.

/// What a list of frame times says. Every time is in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// How many frames.
    pub count: usize,
    /// The mean, rounded down.
    pub mean: u64,
    /// The median (nearest rank).
    pub median: u64,
    /// The 95th percentile (nearest rank).
    pub p95: u64,
    /// The 99th percentile (nearest rank).
    pub p99: u64,
    /// The worst frame.
    pub max: u64,
}

/// The value at the nearest rank of `percent` in a sorted, non-empty list.
fn nearest_rank(sorted: &[u64], percent: usize) -> u64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1]
}

impl Summary {
    /// Summarises `times`, which is sorted by the call; `None` for an empty list.
    #[must_use]
    pub fn of(times: &mut [u64]) -> Option<Self> {
        if times.is_empty() {
            return None;
        }
        times.sort_unstable();
        let total: u128 = times.iter().map(|time| u128::from(*time)).sum();
        let mean = u64::try_from(total / times.len() as u128).unwrap_or(u64::MAX);
        Some(Self {
            count: times.len(),
            mean,
            median: nearest_rank(times, 50),
            p95: nearest_rank(times, 95),
            p99: nearest_rank(times, 99),
            max: times[times.len() - 1],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_has_no_summary() {
        assert_eq!(Summary::of(&mut []), None);
    }

    #[test]
    fn one_frame_is_every_figure() {
        let summary = Summary::of(&mut [7]);
        let want = Summary {
            count: 1,
            mean: 7,
            median: 7,
            p95: 7,
            p99: 7,
            max: 7,
        };
        assert_eq!(summary, Some(want));
    }

    #[test]
    fn the_ranks_of_a_hundred_frames_are_the_frames_themselves() {
        let mut times: Vec<u64> = (1..=100).rev().collect();
        let summary = Summary::of(&mut times);
        let want = Summary {
            count: 100,
            mean: 50,
            median: 50,
            p95: 95,
            p99: 99,
            max: 100,
        };
        assert_eq!(summary, Some(want));
        assert!(times.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn a_rank_rounds_up_so_a_slow_frame_is_not_hidden() {
        // 3 frames: the 95th and 99th percentile are the third; the median is the second.
        let summary = Summary::of(&mut [30, 10, 20]);
        assert_eq!(
            summary.map(|s| (s.median, s.p95, s.p99)),
            Some((20, 30, 30))
        );
        // 4 frames: the median is the second.
        assert_eq!(Summary::of(&mut [4, 3, 2, 1]).map(|s| s.median), Some(2));
    }

    #[test]
    fn the_mean_does_not_overflow() {
        let summary = Summary::of(&mut [u64::MAX, u64::MAX]);
        assert_eq!(summary.map(|s| s.mean), Some(u64::MAX));
    }
}
