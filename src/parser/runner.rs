use crate::{Output, RunReport, output::OutputReport};

/// Record an error and return true when parsing should stop.
///
/// `log_before_fail` is an error budget used by parsers: parsing stops when
/// `num_errors > log_before_fail`. A value of 0 stops after the first error, 1
/// allows one error and stops after the second, and so on.
pub(crate) fn record_error(
    report: &mut RunReport,
    error: impl Into<String>,
    log_before_fail: usize,
) -> bool {
    report.add_error(error.into());
    report.num_errors > log_before_fail
}

pub(crate) fn finalized_output_report(output: &mut Output) -> OutputReport {
    if let Err(error) = output.finalize() {
        let mut output_report = output.get_report();
        output_report.last_error = Some(error.to_string());
        output_report.num_errors += 1;
        return output_report;
    }
    output.get_report()
}

pub(crate) fn finalize_output(output: &mut Output, report: &mut RunReport) {
    report.add_output_report(finalized_output_report(output));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_error_stops_after_budget_is_exceeded() {
        let mut report = RunReport::new();

        assert!(!record_error(&mut report, "first", 1));
        assert_eq!(report.num_errors, 1);
        assert_eq!(report.last_error.as_deref(), Some("first"));

        assert!(record_error(&mut report, "second", 1));
        assert_eq!(report.num_errors, 2);
        assert_eq!(report.last_error.as_deref(), Some("second"));
    }

    #[test]
    fn zero_error_budget_stops_after_first_error() {
        let mut report = RunReport::new();

        assert!(record_error(&mut report, "first", 0));
        assert_eq!(report.num_errors, 1);
    }
}
