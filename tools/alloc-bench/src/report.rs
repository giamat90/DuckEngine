//! The `RESULT` line format and the table that compares two runs.

use crate::stats::Summary;

/// One measured case.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub case: String,
    pub param: String,
    pub summary: Summary,
}

/// The text line of a row: `RESULT|case|param|median|min|max`.
#[must_use]
pub fn format_row(row: &Row) -> String {
    let Summary { median, min, max } = row.summary;
    format!("RESULT|{}|{}|{median}|{min}|{max}", row.case, row.param)
}

/// The row of a `RESULT` line; other lines give `None`.
#[must_use]
pub fn parse_row(line: &str) -> Option<Row> {
    let mut fields = line.strip_prefix("RESULT|")?.split('|');
    let case = fields.next()?.to_owned();
    let param = fields.next()?.to_owned();
    let mut number = || fields.next()?.parse::<f64>().ok();
    let summary = Summary {
        median: number()?,
        min: number()?,
        max: number()?,
    };
    Some(Row {
        case,
        param,
        summary,
    })
}

/// A markdown table of the cases of `default` with the matching cases of `candidate` and the ratio
/// default / candidate (above 1 means the candidate is faster). Cases missing from `candidate`
/// show `n/a`.
#[must_use]
pub fn compare(default: &[Row], candidate: &[Row]) -> String {
    let mut lines = vec![
        "| case | param | default ns | candidate ns | default / candidate |".to_owned(),
        "|---|---|---|---|---|".to_owned(),
    ];
    lines.extend(default.iter().map(|row| table_line(row, candidate)));
    lines.join("\n") + "\n"
}

fn table_line(row: &Row, candidate: &[Row]) -> String {
    let found = candidate
        .iter()
        .find(|c| c.case == row.case && c.param == row.param);
    let cells = match found {
        Some(c) => format!(
            "{:.2} | {:.2} | {:.2}",
            row.summary.median,
            c.summary.median,
            row.summary.median / c.summary.median
        ),
        None => format!("{:.2} | n/a | n/a", row.summary.median),
    };
    format!("| {} | {} | {cells} |", row.case, row.param)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(case: &str, param: &str, median: f64) -> Row {
        Row {
            case: case.to_owned(),
            param: param.to_owned(),
            summary: Summary {
                median,
                min: median - 1.0,
                max: median + 1.0,
            },
        }
    }

    #[test]
    fn a_row_survives_a_round_trip() {
        let r = row("churn", "64 B x 4096", 27.5);
        assert_eq!(parse_row(&format_row(&r)), Some(r));
    }

    #[test]
    fn lines_that_are_not_results_are_ignored() {
        assert_eq!(parse_row("ENV|os|windows"), None);
        assert_eq!(parse_row(""), None);
        assert_eq!(parse_row("RESULT|only|three"), None);
        assert_eq!(parse_row("RESULT|a|b|x|1|2"), None);
    }

    #[test]
    fn the_table_gives_the_ratio_default_over_candidate() {
        let table = compare(&[row("churn", "64 B", 30.0)], &[row("churn", "64 B", 6.0)]);
        assert!(
            table.contains("| churn | 64 B | 30.00 | 6.00 | 5.00 |"),
            "{table}"
        );
    }

    #[test]
    fn a_case_missing_from_the_candidate_shows_not_available() {
        let table = compare(
            &[row("churn", "64 B", 30.0), row("tree", "d16", 10.0)],
            &[row("churn", "64 B", 6.0)],
        );
        assert!(
            table.contains("| tree | d16 | 10.00 | n/a | n/a |"),
            "{table}"
        );
    }

    #[test]
    fn the_table_starts_with_a_header() {
        let table = compare(&[], &[]);
        assert!(
            table.starts_with("| case | param | default ns | candidate ns | default / candidate |"),
            "{table}"
        );
    }
}
