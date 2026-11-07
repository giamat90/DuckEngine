//! The statistics the report needs: minimum, median and 95th percentile (nearest rank), the
//! values the measurement protocol of `docs/budgets/reference-machine-and-workloads.md` asks for.

/// Minimum, median and 95th percentile of a set of samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Smallest sample.
    pub min: f64,
    /// Median sample.
    pub median: f64,
    /// 95th percentile (nearest rank), the spread above the median.
    pub p95: f64,
}

/// Percent of the samples at or below the 95th percentile.
const P95: usize = 95;

/// Samples at or below the 95th percentile are `PERCENT_BASE` out of this many.
const PERCENT_BASE: usize = 100;

/// Minimum, median and 95th percentile (the samples are sorted in place); `None` when there are
/// no samples or one is not a number.
#[must_use]
pub fn summarize(samples: &mut [f64]) -> Option<Summary> {
    if samples.is_empty() || samples.iter().any(|sample| sample.is_nan()) {
        return None;
    }
    samples.sort_by(f64::total_cmp);
    let middle = samples.len() / 2;
    let median = if samples.len() % 2 == 1 {
        *samples.get(middle)?
    } else {
        (samples.get(middle.checked_sub(1)?)? + samples.get(middle)?) / 2.0
    };
    let rank = (samples.len() * P95).div_ceil(PERCENT_BASE);
    Some(Summary {
        min: *samples.first()?,
        median,
        p95: *samples.get(rank.checked_sub(1)?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        assert_eq!(summarize(&mut [3.0, 1.0, 2.0]).map(|s| s.median), Some(2.0));
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_two() {
        let mut samples = [4.0, 1.0, 2.0, 3.0];
        assert_eq!(summarize(&mut samples).map(|s| s.median), Some(2.5));
    }

    #[test]
    fn the_p95_of_twenty_samples_is_the_nineteenth() {
        let mut samples: Vec<f64> = (1..=20).rev().map(f64::from).collect();
        let summary = summarize(&mut samples);
        assert_eq!(
            summary,
            Some(Summary {
                min: 1.0,
                median: 10.5,
                p95: 19.0
            })
        );
    }

    #[test]
    fn the_p95_of_thirty_samples_is_the_twenty_ninth() {
        let mut samples: Vec<f64> = (1..=30).map(f64::from).collect();
        assert_eq!(summarize(&mut samples).map(|s| s.p95), Some(29.0));
    }

    #[test]
    fn one_sample_is_its_own_minimum_median_and_p95() {
        let one = Summary {
            min: 7.0,
            median: 7.0,
            p95: 7.0,
        };
        assert_eq!(summarize(&mut [7.0]), Some(one));
    }

    #[test]
    fn nothing_and_not_a_number_have_no_summary() {
        assert_eq!(summarize(&mut []), None);
        assert_eq!(summarize(&mut [1.0, f64::NAN]), None);
    }
}
