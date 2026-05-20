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
