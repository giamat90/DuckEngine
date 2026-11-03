//! The statistics the report needs.

/// Minimum, median and 90th percentile of a set of samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Smallest sample.
    pub min: f64,
    /// Median sample.
    pub median: f64,
    /// 90th percentile (nearest rank), the spread above the median.
    pub p90: f64,
}

/// Median of the samples (sorted in place); `None` when there are none.
#[must_use]
pub fn median(samples: &mut [f64]) -> Option<f64> {
    samples.sort_by(f64::total_cmp);
    let middle = samples.len() / 2;
    if samples.len() % 2 == 1 {
        samples.get(middle).copied()
    } else {
        let (low, high) = (samples.get(middle.checked_sub(1)?)?, samples.get(middle)?);
        Some((low + high) / 2.0)
    }
}

/// Minimum, median and 90th percentile (sorted in place); `None` when there are no samples.
#[must_use]
pub fn summarize(samples: &mut [f64]) -> Option<Summary> {
    let median = median(samples)?;
    let rank = (samples.len() * 9).div_ceil(10);
    Some(Summary {
        min: *samples.first()?,
        median,
        p90: *samples.get(rank.checked_sub(1)?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_two() {
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
    }

    #[test]
    fn the_median_of_one_and_of_two_values() {
        assert_eq!(median(&mut [5.0]), Some(5.0));
        assert_eq!(median(&mut [4.0, 2.0]), Some(3.0));
    }

    #[test]
    fn the_median_of_nothing_is_none() {
        assert_eq!(median(&mut []), None);
    }

    #[test]
    fn the_summary_has_min_median_and_nearest_rank_p90() {
        let mut samples: Vec<f64> = (1..=10).rev().map(f64::from).collect();
        let summary = summarize(&mut samples);
        assert_eq!(
            summary,
            Some(Summary {
                min: 1.0,
                median: 5.5,
                p90: 9.0
            })
        );
    }

    #[test]
    fn the_summary_of_one_sample_is_that_sample_and_of_nothing_is_none() {
        let one = Summary {
            min: 7.0,
            median: 7.0,
            p90: 7.0,
        };
        assert_eq!(summarize(&mut [7.0]), Some(one));
        assert_eq!(summarize(&mut []), None);
    }
}
