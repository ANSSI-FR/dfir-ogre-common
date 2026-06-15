use std::collections::HashMap;

use pyo3::prelude::*;

use crate::output::OutputReport;
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
    pub with_qualifiers: bool,
    #[pyo3(get)]
    pub include_empty: bool,
    #[pyo3(get, set)]
    pub output_folder: String,
    #[pyo3(get, set)]
    pub base_file_name: String,
    #[pyo3(get)]
    pub params: HashMap<String, String>,
}
#[pymethods]
impl OutputConfiguration {
    /// Creates a new `OutputConfiguration`.
    ///
    /// * `output_type` – identifier for the output kind.
    /// * `format` – primary format (e.g., “json”, “csv”).
    /// * `date_format` – pattern used for serialising timestamps.
    /// * `with_timeline` – include timeline information when true.
    /// * `with_qualifiers` – attach qualifier data when true.
    /// * `include_empty` – emit empty fields when true.
    /// * `output_folder` – directory where files are written.
    /// * `base_file_name` – base name for created files.
    /// * `params` – free‑form key/value pairs for extra options.
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = ( base_file_name, output_folder, output_type="file".into(), format="jsonl".into(), date_format="iso_utc".into(), with_timeline=false,with_qualifiers=false,include_empty=true, params=HashMap::new()))]
    pub fn new(
        base_file_name: String,
        output_folder: String,
        output_type: String,
        format: String,
        date_format: String,
        with_timeline: bool,
        with_qualifiers: bool,
        include_empty: bool,
        params: HashMap<String, String>,
    ) -> Self {
        Self {
            output_type,
            format,
            date_format,
            with_timeline,
            with_qualifiers,
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
        assert!(config.with_qualifiers);
        assert!(!config.include_empty);
        assert_eq!(config.params, params);
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
