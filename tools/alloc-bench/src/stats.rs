//! Timing samples and their summary.

/// Median, minimum and maximum of the samples, in nanoseconds per unit of work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub median: f64,
    pub min: f64,
    pub max: f64,
}

/// Summary of `samples`, or `None` when there are none.
#[must_use]
pub fn summarize(samples: &[f64]) -> Option<Summary> {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let (min, max) = (*sorted.first()?, *sorted.last()?);
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    } else {
        sorted[middle]
    };
    Some(Summary { median, min, max })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_samples_have_no_summary() {
        assert_eq!(summarize(&[]), None);
    }

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        let s = summarize(&[9.0, 1.0, 5.0]);
        assert_eq!(
            s,
            Some(Summary {
                median: 5.0,
                min: 1.0,
                max: 9.0
            })
        );
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_two_middle_values() {
        let s = summarize(&[4.0, 1.0, 2.0, 10.0]);
        assert_eq!(
            s,
            Some(Summary {
                median: 3.0,
                min: 1.0,
                max: 10.0
            })
        );
    }

    #[test]
    fn one_sample_is_its_own_summary() {
        let s = summarize(&[7.5]);
        assert_eq!(
            s,
            Some(Summary {
                median: 7.5,
                min: 7.5,
                max: 7.5
            })
        );
    }
}
