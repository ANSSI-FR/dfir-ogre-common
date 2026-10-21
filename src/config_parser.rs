use std::collections::HashMap;

use pyo3::prelude::*;

use crate::{
    Error,
    date_util::DateOutputCodec,
    output::{
        COMPRESSION_LEVEL, DEFAULT_COMPRESSION_LEVEL, OutputFormat, OutputReport, OutputType,
    },
};
use log::error;
/// Holds the command a plugin runs and a short description of its purpose.
#[derive(Debug, Clone)]
#[pyclass(get_all, from_py_object)]
pub struct PluginDescription {
    command: String,
    description: String,
}

#[pymethods]
impl PluginDescription {
    #[new]
    /// Creates a new `PluginDescription`.
    ///
    /// * `command` – the CLI command the plugin implements.
    /// * `description` – a brief human‑readable description.
    pub fn new(command: String, description: String) -> Self {
        Self {
            command,
            description,
        }
    }

    /// Returns a copy of the stored command.
    fn get_command(&self) -> String {
        self.command.clone()
    }

    /// Returns a copy of the stored description.
    fn get_description(&self) -> String {
        self.description.clone()
    }
}

/// Configuration for how parser output should be written.
///
/// Controls the format, date handling, and which optional sections appear.
#[derive(Debug, Default, Clone)]
#[pyclass(from_py_object)]
pub struct OutputConfiguration {
    #[pyo3(get)]
    pub output_type: String,
    #[pyo3(get)]
    pub format: String,
    #[pyo3(get)]
    pub date_format: String,
    #[pyo3(get)]
    pub with_timeline: bool,
    #[pyo3(get)]
    pub timeline_include_undated: bool,
    #[pyo3(get)]
    pub include_empty: bool,
    #[pyo3(get, set)]
    pub output_folder: String,
    #[pyo3(get, set)]
    pub base_file_name: String,
    #[pyo3(get)]
    pub params: HashMap<String, String>,
}
impl OutputConfiguration {
    /// Creates a new `OutputConfiguration`.
    ///
    /// * `output_type` – identifier for the output kind.
    /// * `format` – primary format (e.g., “json”, “csv”).
    /// * `date_format` – pattern used for serialising timestamps.
    /// * `with_timeline` – include timeline information when true.
    /// * `include_empty` – emit empty fields when true.
    /// * `output_folder` – directory where files are written.
    /// * `base_file_name` – base name for created files.
    /// * `params` – free‑form key/value pairs for extra options.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_file_name: String,
        output_folder: String,
        output_type: String,
        format: String,
        date_format: String,
        with_timeline: bool,
        include_empty: bool,
        params: HashMap<String, String>,
    ) -> Self {
        Self {
            output_type,
            format,
            date_format,
            with_timeline,
            timeline_include_undated: false,
            include_empty,
            output_folder,
            base_file_name,
            params,
        }
    }

    pub(crate) fn compression_level(&self) -> Result<u32, Error> {
        let compression_level = match self.params.get(COMPRESSION_LEVEL) {
            Some(level) => level.parse()?,
            None => DEFAULT_COMPRESSION_LEVEL,
        };
        if compression_level > 9 {
            return Err(Error::ConfigurationError(format!(
                "Invalid compression level '{compression_level}', expected a value from 0 to 9"
            )));
        }
        Ok(compression_level)
    }
}

#[pymethods]
impl OutputConfiguration {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (base_file_name, output_folder, output_type="file".into(), format="jsonl".into(), date_format="iso_utc".into(), with_timeline=false, include_empty=true, params=HashMap::new(), timeline_include_undated=false))]
    /// Python constructor for an output configuration.
    ///
    /// `timeline_include_undated` emits a null-timestamp placeholder when timeline extraction finds
    /// no valid date. It is ignored unless `with_timeline` is enabled. `analytics_jsonl` rejects
    /// this option when it is true.
    fn py_new(
        base_file_name: String,
        output_folder: String,
        output_type: String,
        format: String,
        date_format: String,
        with_timeline: bool,
        include_empty: bool,
        params: HashMap<String, String>,
        timeline_include_undated: bool,
    ) -> Self {
        Self {
            output_type,
            format,
            date_format,
            with_timeline,
            timeline_include_undated,
            include_empty,
            output_folder,
            base_file_name,
            params,
        }
    }

    /// Implements Python's `deepcopy` protocol by cloning the struct.
    pub fn __deepcopy__(&self, _memo: Py<PyAny>) -> Self {
        self.clone()
    }

    /// Validates output options without opening or creating output files.
    pub fn validate(&self) -> Result<(), Error> {
        if self.format == "analytics_jsonl" && self.timeline_include_undated {
            return Err(Error::ConfigurationError(
                "'timeline_include_undated' is not supported with 'analytics_jsonl'".to_owned(),
            ));
        }
        DateOutputCodec::from_string(&self.date_format).validate()?;
        OutputFormat::from_str(&self.format)?;
        OutputType::from_str(&self.output_type)?;
        self.compression_level()?;
        Ok(())
    }
}

/// Top‑level configuration supplied to the parser runner.
///
/// Contains one or more `OutputConfiguration`s, a flag to force snake_case naming,
/// and an optional map of extra parameters.

#[derive(Debug, Default, Clone)]
#[pyclass(from_py_object)]
pub struct RunConfiguration {
    #[pyo3(get)]
    pub output: Vec<OutputConfiguration>,
    #[pyo3(get)]
    pub force_snake_case: bool,
    #[pyo3(get)]
    pub params: HashMap<String, Option<String>>,
}
#[pymethods]
impl RunConfiguration {
    #[new]
    #[pyo3(signature = (output, force_snake_case=true, params=None))]
    /// Constructs a new `RunConfiguration`.
    ///
    /// * `output` – list of desired output configurations.
    /// * `force_snake_case` – if true, field names are forced to snake_case.
    /// * `params` – optional free‑form key/value pairs.
    pub fn new(
        output: Vec<OutputConfiguration>,
        force_snake_case: bool,
        params: Option<HashMap<String, Option<String>>>,
    ) -> Self {
        Self {
            output,
            force_snake_case,
            params: params.unwrap_or_default(),
        }
    }
}

impl RunConfiguration {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.output.is_empty() {
            return Err(Error::ConfigurationError(
                "invalid run configuration, it requires at least one output configuration"
                    .to_string(),
            ));
        }

        for output in &self.output {
            output.validate()?;
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
#[pyclass(get_all)]
/// Summary of a parsing run.
/// Captures the last error (if any), the total error count, and the output reports generated.
pub struct RunReport {
    pub last_error: Option<String>,
    pub num_errors: usize,
    pub output_reports: Vec<OutputReport>,
}
#[pymethods]
impl RunReport {
    #[new]
    /// Creates an empty `RunReport`.
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    /// Records an error message and increments the error counter.
    pub fn add_error(&mut self, error: String) {
        error!("{error}");
        self.last_error = Some(error);
        self.num_errors += 1;
    }

    /// Adds an `OutputReport` to the collection, propagating any errors it contains.
    pub fn add_output_report(&mut self, output_result: OutputReport) {
        if output_result.last_error.is_some() {
            self.last_error = output_result.last_error.clone();
            self.num_errors += output_result.num_errors;
        }
        self.output_reports.push(output_result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn plugin_description_getters_return_constructor_values() {
        let description = PluginDescription::new(
            "collect_evtx".to_string(),
            "Collects Windows event logs".to_string(),
        );

        assert_eq!(description.get_command(), "collect_evtx");
        assert_eq!(description.get_description(), "Collects Windows event logs");
    }

    #[test]
    fn output_configuration_new_stores_all_fields() {
        let mut params = HashMap::new();
        params.insert("compression_level".to_string(), "9".to_string());

        let config = OutputConfiguration::new(
            "events".to_string(),
            "/tmp/output".to_string(),
            "gzip".to_string(),
            "normalized_jsonl".to_string(),
            "iso".to_string(),
            true,
            false,
            params.clone(),
        );

        assert_eq!(config.base_file_name, "events");
        assert_eq!(config.output_folder, "/tmp/output");
        assert_eq!(config.output_type, "gzip");
        assert_eq!(config.format, "normalized_jsonl");
        assert_eq!(config.date_format, "iso");
        assert!(config.with_timeline);
        assert!(!config.timeline_include_undated);
        assert!(!config.include_empty);
        assert_eq!(config.params, params);
    }

    #[test]
    fn output_configuration_python_constructor_accepts_timeline_include_undated() {
        let config = OutputConfiguration::py_new(
            "events".to_string(),
            "/tmp/output".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            HashMap::new(),
            true,
        );

        assert!(config.with_timeline);
        assert!(config.timeline_include_undated);
    }

    fn valid_output_configuration() -> OutputConfiguration {
        OutputConfiguration::new(
            "events".to_owned(),
            ".tmp/validate-output".to_owned(),
            "file".to_owned(),
            "jsonl".to_owned(),
            "iso_utc".to_owned(),
            false,
            true,
            HashMap::new(),
        )
    }

    #[test]
    fn output_configuration_validation_accepts_supported_options() {
        for output_type in ["file", "gzip", "GZIP"] {
            for format in [
                "jsonl",
                "csv",
                "normalized_jsonl",
                "normalized_csv",
                "analytics_jsonl",
            ] {
                for date_format in ["iso", "iso_utc", "utc_naive", "%Y-%m-%d %H:%M:%S%:z"] {
                    for compression_level in [None, Some("0"), Some("9")] {
                        let mut output = valid_output_configuration();
                        output.output_type = output_type.to_owned();
                        output.format = format.to_owned();
                        output.date_format = date_format.to_owned();
                        if let Some(level) = compression_level {
                            output
                                .params
                                .insert("compression_level".to_owned(), level.to_owned());
                        }

                        output.validate().unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn output_configuration_validation_rejects_unsupported_output_type() {
        let mut output = valid_output_configuration();
        output.output_type = "stdout".to_owned();

        assert!(matches!(
            output.validate(),
            Err(Error::InvalidOutputType(_))
        ));
    }

    #[test]
    fn output_configuration_validation_rejects_unsupported_format() {
        let mut output = valid_output_configuration();
        output.format = "json".to_owned();

        assert!(matches!(
            output.validate(),
            Err(Error::InvalidOutputFormat(_))
        ));
    }

    #[test]
    fn output_configuration_validation_rejects_invalid_date_pattern() {
        let mut output = valid_output_configuration();
        output.date_format = "%Y-%Q".to_owned();

        let error = output.validate().unwrap_err();

        assert!(matches!(error, Error::ConfigurationError(_)));
        assert!(error.to_string().contains("Invalid date output pattern"));
    }

    #[test]
    fn output_configuration_validation_rejects_non_integer_compression_levels() {
        for level in ["fast", "", "-1", "1.5", "4294967296"] {
            let mut output = valid_output_configuration();
            output
                .params
                .insert("compression_level".to_owned(), level.to_owned());

            assert!(
                matches!(output.validate(), Err(Error::ParseInt(_))),
                "level: {level}"
            );
        }
    }

    #[test]
    fn output_configuration_validation_rejects_out_of_range_compression_levels() {
        for level in ["10", "4294967295"] {
            let mut output = valid_output_configuration();
            output
                .params
                .insert("compression_level".to_owned(), level.to_owned());

            let error = output.validate().unwrap_err();

            assert!(matches!(error, Error::ConfigurationError(_)));
            assert!(error.to_string().contains("expected a value from 0 to 9"));
        }
    }

    #[test]
    fn output_configuration_validation_rejects_analytics_undated_option() {
        for with_timeline in [false, true] {
            let mut output = valid_output_configuration();
            output.format = "analytics_jsonl".to_owned();
            output.with_timeline = with_timeline;
            output.timeline_include_undated = true;

            assert!(matches!(
                output.validate(),
                Err(Error::ConfigurationError(_))
            ));
        }
    }

    #[test]
    fn output_configuration_validation_allows_undated_option_without_timeline() {
        let mut output = valid_output_configuration();
        output.timeline_include_undated = true;

        output.validate().unwrap();
    }

    #[test]
    fn output_configuration_validation_does_not_create_output_folder() {
        let output_folder = std::env::temp_dir().join(format!(
            "dfir-ogre-validate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut output = valid_output_configuration();
        output.output_folder = output_folder.display().to_string();
        output.output_type = "gzip".to_owned();
        output.format = "normalized_jsonl".to_owned();
        output.with_timeline = true;
        assert!(!output_folder.exists());

        output.validate().unwrap();

        assert!(!output_folder.exists());
    }

    #[test]
    fn run_configuration_validation_rejects_invalid_later_output() {
        let mut invalid = valid_output_configuration();
        invalid.output_type = "stdout".to_owned();
        let config = RunConfiguration::new(vec![valid_output_configuration(), invalid], true, None);

        assert!(matches!(
            config.validate(),
            Err(Error::InvalidOutputType(_))
        ));
    }

    #[test]
    fn run_configuration_new_defaults_missing_params_to_empty_map() {
        let output = OutputConfiguration::new(
            "records".to_string(),
            "/tmp/output".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso_utc".to_string(),
            false,
            true,
            HashMap::new(),
        );

        let config = RunConfiguration::new(vec![output], false, None);

        assert_eq!(config.output.len(), 1);
        assert!(!config.force_snake_case);
        assert!(config.params.is_empty());
    }

    #[test]
    fn run_configuration_new_uses_provided_params() {
        let mut params = HashMap::new();
        params.insert("case_id".to_string(), Some("IR-42".to_string()));
        params.insert("optional".to_string(), None);

        let config = RunConfiguration::new(vec![], true, Some(params.clone()));

        assert!(config.output.is_empty());
        assert!(config.force_snake_case);
        assert_eq!(config.params, params);
    }

    #[test]
    fn run_configuration_validation_rejects_empty_outputs() {
        let config = RunConfiguration::new(vec![], true, None);

        let error = config.validate().unwrap_err();

        assert!(matches!(error, Error::ConfigurationError(_)));
        assert!(error.to_string().contains("at least one output"));
    }

    #[test]
    fn analytics_jsonl_rejects_timeline_include_undated_with_or_without_timeline() {
        for with_timeline in [false, true] {
            let mut output = OutputConfiguration::new(
                "events".to_owned(),
                ".tmp/analytics-invalid".to_owned(),
                "file".to_owned(),
                "analytics_jsonl".to_owned(),
                "iso".to_owned(),
                with_timeline,
                true,
                HashMap::new(),
            );
            output.timeline_include_undated = true;

            let error = RunConfiguration::new(vec![output], true, None)
                .validate()
                .expect_err("analytics must reject undated timeline output");

            match error {
                Error::ConfigurationError(message) => assert_eq!(
                    message,
                    "'timeline_include_undated' is not supported with 'analytics_jsonl'"
                ),
                other => panic!("expected configuration error, got {other}"),
            }
        }
    }

    #[test]
    fn normalized_jsonl_still_accepts_timeline_include_undated() {
        let mut output = OutputConfiguration::new(
            "events".to_owned(),
            ".tmp/normalized-undated".to_owned(),
            "file".to_owned(),
            "normalized_jsonl".to_owned(),
            "iso".to_owned(),
            true,
            true,
            HashMap::new(),
        );
        output.timeline_include_undated = true;

        assert!(
            RunConfiguration::new(vec![output], true, None)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn run_report_add_error_tracks_last_error_and_count() {
        let mut report = RunReport::new();

        report.add_error("first".to_string());
        report.add_error("second".to_string());

        assert_eq!(report.last_error.as_deref(), Some("second"));
        assert_eq!(report.num_errors, 2);
        assert!(report.output_reports.is_empty());
    }

    #[test]
    fn run_report_add_output_report_accumulates_nested_errors() {
        let mut report = RunReport::new();
        let output_report = OutputReport {
            last_error: Some("write failed".to_string()),
            num_errors: 3,
            file_reports: vec![],
        };

        report.add_output_report(output_report);

        assert_eq!(report.last_error.as_deref(), Some("write failed"));
        assert_eq!(report.num_errors, 3);
        assert_eq!(report.output_reports.len(), 1);
    }

    #[test]
    fn run_report_add_output_report_without_errors_preserves_error_state() {
        let mut report = RunReport::new();
        report.add_error("parse failed".to_string());

        report.add_output_report(OutputReport::default());

        assert_eq!(report.last_error.as_deref(), Some("parse failed"));
        assert_eq!(report.num_errors, 1);
        assert_eq!(report.output_reports.len(), 1);
    }
}
