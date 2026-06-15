use std::sync::{Arc, Mutex};

use crate::configuration::{DataTypeMapping, PluginConfiguration};
use crate::date_util::DateOutputCodec;
use crate::errors::Error;
use crate::line_builder::LineBuilder;

use crate::config_parser::RunConfiguration;
use crate::format_csv::CsvFormatter;
use crate::format_json::JsonFormatter;
use crate::{FieldMapping, Metadata, Record};

use pyo3::prelude::*;

pub const COMPRESSION_LEVEL: &str = "compression_level";
pub const DEFAULT_COMPRESSION_LEVEL: u32 = 5;

#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct OutputReport {
    pub last_error: Option<String>,
    pub num_errors: usize,
    pub file_reports: Vec<FileReport>,
}

#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct FileReport {
    pub output_type: String,
    pub format: String,
    pub date_format: String,
    pub with_timeline: bool,
    pub with_qualifiers: bool,
    pub include_empty: bool,
    pub file_name: String,
    pub num_lines: usize,
}

#[pyclass]
pub enum OutputType {
    File,
    Gzip,
}
impl OutputType {
    fn from_str(output_type: &str) -> Result<OutputType, Error> {
        match output_type.to_lowercase().as_str() {
            "file" => Ok(Self::File),
            "gzip" => Ok(Self::Gzip),
            _ => Err(Error::InvalidOutputType(output_type.to_owned())),
        }
    }
}

#[pyclass]
pub enum OutputFormat {
    Jsonl,
    NormalizedJsonl,
    Csv,
    NormalizedCsv,
}
impl OutputFormat {
    fn from_str(format: &str) -> Result<OutputFormat, Error> {
        match format {
            "jsonl" => Ok(OutputFormat::Jsonl),
            "normalized_jsonl" => Ok(OutputFormat::NormalizedJsonl),
            "csv" => Ok(OutputFormat::Csv),
            "normalized_csv" => Ok(OutputFormat::NormalizedCsv),
            _ => Err(Error::InvalidOutputFormat(format.to_owned())),
        }
    }
}

#[derive(Clone)]
enum OutputWriter {
    Jsonl(Arc<Mutex<JsonFormatter>>),
    NormalizedJsonl(Arc<Mutex<JsonFormatter>>),
    Csv(Arc<Mutex<CsvFormatter>>),
    NormalizedCsv(Arc<Mutex<CsvFormatter>>),
}
impl OutputWriter {
    /// Serialize a record and its accompanying metadata using the wrapped writer.
    ///
    /// # Arguments
    /// * `data` - Mutable reference to the `Record` containing the data to write
    /// * `metadata` - IndexMap containing metadata to associate with the write operation
    ///
    pub fn write(&mut self, data: &LineBuilder) -> Result<(), Error> {
        match self {
            Self::Jsonl(json_output) => {
                let mut out = json_output.lock().expect("mutex lock panicked");
                out.write(data)
            }

            Self::NormalizedJsonl(json_output) => {
                let mut out = json_output.lock().expect("mutex lock panicked");
                out.write_normalized(data)
            }

            Self::Csv(csv_output) => {
                let mut out = csv_output.lock().expect("mutex lock panicked");
                out.write(data)
            }

            Self::NormalizedCsv(csv_output) => {
                let mut out = csv_output.lock().expect("mutex lock panicked");
                out.write_normalized(data)
            }
        }
    }

    pub fn write_metadata(&mut self, data: &LineBuilder) -> Result<(), Error> {
        match self {
            Self::NormalizedJsonl(json_output) => {
                let mut out = json_output.lock().expect("mutex lock panicked");
                out.write_metadata(data)
            }
            Self::NormalizedCsv(csv_output) => {
                let mut out = csv_output.lock().expect("mutex lock panicked");
                out.write_metadata(data)
            }
            _ => Ok(()),
        }
    }

    /// Pull the FileReport that the underlying writer has been tracking.
    ///
    /// # Returns
    /// * `OuputResult` - Contains the file name and the number of lines written
    pub fn result(&self) -> FileReport {
        match self {
            Self::Jsonl(json_output) => {
                let out = json_output.lock().expect("mutex lock panicked");
                out.file_report.clone()
            }
            Self::NormalizedJsonl(json_output) => {
                let out = json_output.lock().expect("mutex lock panicked");
                out.file_report.clone()
            }
            Self::Csv(csv_output) => {
                let out = csv_output.lock().expect("mutex lock panicked");
                out.file_report.clone()
            }
            Self::NormalizedCsv(csv_output) => {
                let out = csv_output.lock().expect("mutex lock panicked");
                out.file_report.clone()
            }
        }
    }

    /// Flushes buffers and closes the underlying destination, handling any I/O errors.
    pub fn close(&mut self) -> Result<(), Error> {
        match self {
            Self::Jsonl(json_output) => {
                let mut out = json_output.lock().expect("mutex lock panicked");
                out.close()?;
            }
            Self::NormalizedJsonl(json_output) => {
                let mut out = json_output.lock().expect("mutex lock panicked");
                out.close()?;
            }
            Self::Csv(csv_output) => {
                let mut out = csv_output.lock().expect("mutex lock panicked");
                out.close()?;
            }
            Self::NormalizedCsv(csv_output) => {
                let mut out = csv_output.lock().expect("mutex lock panicked");
                out.close()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
#[pyclass(from_py_object)]
pub struct Output {
    outputs: Vec<(OutputWriter, bool)>,
    line_builder_without_qualifiers: Option<LineBuilder>,
    line_builder_with_qualifiers: Option<LineBuilder>,
}
impl Output {
    fn select_datatype(
        plugin_config: &PluginConfiguration,
        data_type: Option<String>,
    ) -> Result<&DataTypeMapping, Error> {
        if plugin_config.data_type_configs.is_empty() {
            return Err(Error::ConfigurationError(
                "invalid plugin configuration, it requires at least one data type configuration"
                    .to_string(),
            ));
        }

        let data_type_conf = match data_type {
            Some(data_type) => {
                let config = plugin_config
                    .data_type_configs
                    .iter()
                    .find(|config| config.data_type.eq(&data_type));
                match config {
                    Some(config) => config,
                    None => {
                        return Err(Error::ConfigurationError(format!(
                            "'{data_type}' not found in the plugin configuration. Plugin: {}",
                            plugin_config.plugin
                        )));
                    }
                }
            }
            //no specidic data type provided, we take the first one
            None => &plugin_config.data_type_configs[0],
        };
        Ok(data_type_conf)
    }
}

#[pymethods]
impl Output {
    #[new]
    #[pyo3(signature = (run_config, plugin_config, metadata, data_type=None))]
    pub fn new(
        run_config: RunConfiguration,
        plugin_config: PluginConfiguration,
        mut metadata: Metadata,
        data_type: Option<String>,
    ) -> Result<Self, Error> {
        let data_type_conf = Output::select_datatype(&plugin_config, data_type)?;
        metadata.data_type = data_type_conf.data_type.clone();
        //series of flag that defines what needs to be computed
        let mut compute_timeline = false;
        let mut compute_hash = false;
        let mut compute_with_qualifiers = false;
        let mut compute_without_qualifiers = false;
        let mut outputs = Vec::new();

        for output_conf in &run_config.output {
            let output_date_codec = DateOutputCodec::from_string(output_conf.date_format.as_str());
            let output_format = OutputFormat::from_str(&output_conf.format)?;
            let output_type = OutputType::from_str(&output_conf.output_type)?;

            let compression_level = output_conf.params.get(COMPRESSION_LEVEL);
            let compression_level: u32 = match compression_level {
                Some(c) => c.parse()?,
                None => DEFAULT_COMPRESSION_LEVEL,
            };

            if output_conf.with_qualifiers {
                compute_with_qualifiers = true
            } else {
                compute_without_qualifiers = true
            }
            if output_conf.with_timeline {
                compute_timeline = true
            }
            //create output folder
            let output_folder = &output_conf.output_folder;
            std::fs::create_dir_all(output_folder)?;

            //create filename
            let mut base_file_name = output_conf.base_file_name.to_owned();
            base_file_name.push('.');
            base_file_name.push_str(&data_type_conf.data_type);

            let file_report = FileReport {
                output_type: output_conf.output_type.clone(),
                format: output_conf.format.clone(),
                date_format: output_conf.date_format.clone(),
                with_timeline: output_conf.with_timeline,
                with_qualifiers: output_conf.with_qualifiers,
                include_empty: output_conf.include_empty,
                ..Default::default()
            };

            let output_writer = match output_format {
                OutputFormat::Jsonl => {
                    let json_output = JsonFormatter::new(
                        output_type,
                        output_folder,
                        &base_file_name,
                        output_date_codec,
                        output_conf.include_empty,
                        output_conf.with_timeline,
                        compression_level,
                        file_report,
                        false,
                    )?;
                    OutputWriter::Jsonl(Arc::new(Mutex::new(json_output)))
                }
                OutputFormat::NormalizedJsonl => {
                    let json_output = JsonFormatter::new(
                        output_type,
                        output_folder,
                        &base_file_name,
                        output_date_codec,
                        output_conf.include_empty,
                        output_conf.with_timeline,
                        compression_level,
                        file_report,
                        true,
                    )?;
                    OutputWriter::NormalizedJsonl(Arc::new(Mutex::new(json_output)))
                }
                OutputFormat::Csv => {
                    let csv = CsvFormatter::new(
                        data_type_conf.field_mapping.clone(),
                        output_type,
                        output_folder,
                        &base_file_name,
                        output_date_codec,
                        output_conf.include_empty,
                        output_conf.with_timeline,
                        compression_level,
                        file_report,
                        false,
                    )?;
                    OutputWriter::Csv(Arc::new(Mutex::new(csv)))
                }
                OutputFormat::NormalizedCsv => {
                    compute_hash = true;
                    let csv = CsvFormatter::new(
                        data_type_conf.field_mapping.clone(),
                        output_type,
                        output_folder,
                        &base_file_name,
                        output_date_codec,
                        output_conf.include_empty,
                        output_conf.with_timeline,
                        compression_level,
                        file_report,
                        true,
                    )?;
                    OutputWriter::NormalizedCsv(Arc::new(Mutex::new(csv)))
                }
            };
            outputs.push((output_writer, output_conf.with_qualifiers))
        }

        let field_mapping = match &data_type_conf.field_mapping {
            Some(field_mapping) => field_mapping.clone(),
            None => FieldMapping::new(vec![], None),
        };

        let timeline_builder = match compute_timeline {
            true => data_type_conf.timeline.clone(),
            false => None,
        };

        //create if at least one output configuration requires qualifiers
        let line_builder_with_qualifiers = match compute_with_qualifiers {
            true => Some(LineBuilder::new(
                metadata.clone(),
                timeline_builder.clone(),
                field_mapping.clone(),
                true,
                compute_hash,
                data_type_conf.has_primary_key,
                run_config.force_snake_case,
            )),
            false => None,
        };
        //create if at least one output configuration does not requires qualifiers
        let line_builder_without_qualifiers = match compute_without_qualifiers {
            true => Some(LineBuilder::new(
                metadata,
                timeline_builder,
                field_mapping,
                false,
                compute_hash,
                data_type_conf.has_primary_key,
                run_config.force_snake_case,
            )),
            false => None,
        };

        //write metadata for normalized output
        for (writer, _) in &mut outputs {
            if let Some(line_builder) = &line_builder_without_qualifiers {
                writer.write_metadata(line_builder)?;
            } else if let Some(line_builder) = &line_builder_with_qualifiers {
                writer.write_metadata(line_builder)?;
            }
        }

        Ok(Self {
            outputs,
            line_builder_without_qualifiers,
            line_builder_with_qualifiers,
        })
    }

    pub fn write(&mut self, data: &mut Record) -> Result<(), Error> {
        //build the record with qualifiers if necessary
        if let Some(builder) = &mut self.line_builder_with_qualifiers {
            if self.line_builder_without_qualifiers.is_some() {
                //the build process removes data from the record, so we clone it if two builder are required
                builder.build(&mut data.clone())?;
            } else {
                builder.build(data)?;
            }
        }
        //build the record without qualifiers if necessary
        if let Some(builder) = &mut self.line_builder_without_qualifiers {
            builder.build(data)?;
        }

        for (output, with_qualifiers) in &mut self.outputs {
            match with_qualifiers {
                true => {
                    let builder = &mut self
                        .line_builder_with_qualifiers
                        .as_mut()
                        .expect("A line builder with qualifier must exists at this point");

                    output.write(builder)?;
                }
                false => {
                    let builder = &mut self
                        .line_builder_without_qualifiers
                        .as_mut()
                        .expect("A line builder without qualifier must exists at this point");

                    output.write(builder)?;
                }
            }
        }

        data.0.clear();
        Ok(())
    }

    /// Enter the output as a Python context manager – returns a clone for the `with` block.
    pub fn __enter__(&self) -> Self {
        self.clone()
    }

    /// Exit the context, ensuring each writer is cleanly closed.
    pub fn __exit__(&mut self, _exc_type: Py<PyAny>, _exc_value: Py<PyAny>, _traceback: Py<PyAny>) {
        for (out, _) in &mut self.outputs {
            let _ = out.close();
        }
    }

    pub fn get_report(&self) -> OutputReport {
        let mut report = OutputReport {
            ..Default::default()
        };

        for (out, _) in &self.outputs {
            report.file_reports.push(out.result());
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DateInputCodec, configuration::DataTypeMapping};
    use std::collections::HashMap;

    fn data_type_mapping(data_type: &str) -> DataTypeMapping {
        DataTypeMapping {
            data_type: data_type.to_string(),
            description: None,
            default_date_pattern: DateInputCodec::Iso(),
            params: HashMap::new(),
            timeline: None,
            field_mapping: None,
            has_primary_key: false,
        }
    }

    fn plugin_config(data_types: Vec<&str>) -> PluginConfiguration {
        PluginConfiguration {
            plugin: "test-plugin".to_string(),
            file_encoding: "UTF_8".to_string(),
            data_type_configs: data_types.into_iter().map(data_type_mapping).collect(),
        }
    }

    #[test]
    fn output_type_from_str_accepts_known_values_case_insensitively() {
        assert!(matches!(
            OutputType::from_str("file").unwrap(),
            OutputType::File
        ));
        assert!(matches!(
            OutputType::from_str("GZIP").unwrap(),
            OutputType::Gzip
        ));
    }

    #[test]
    fn output_type_from_str_rejects_unknown_value() {
        match OutputType::from_str("stdout") {
            Err(Error::InvalidOutputType(value)) => assert_eq!(value, "stdout"),
            Ok(_) => panic!("unknown output type unexpectedly parsed"),
            Err(err) => panic!("unexpected error: {err}"),
        }
    }

    #[test]
    fn output_format_from_str_accepts_known_values() {
        assert!(matches!(
            OutputFormat::from_str("jsonl").unwrap(),
            OutputFormat::Jsonl
        ));
        assert!(matches!(
            OutputFormat::from_str("normalized_jsonl").unwrap(),
            OutputFormat::NormalizedJsonl
        ));
        assert!(matches!(
            OutputFormat::from_str("csv").unwrap(),
            OutputFormat::Csv
        ));
        assert!(matches!(
            OutputFormat::from_str("normalized_csv").unwrap(),
            OutputFormat::NormalizedCsv
        ));
    }

    #[test]
    fn output_format_from_str_rejects_unknown_value() {
        match OutputFormat::from_str("json") {
            Err(Error::InvalidOutputFormat(value)) => assert_eq!(value, "json"),
            Ok(_) => panic!("unknown output format unexpectedly parsed"),
            Err(err) => panic!("unexpected error: {err}"),
        }
    }

    #[test]
    fn select_datatype_uses_first_mapping_when_none_requested() {
        let config = plugin_config(vec!["events", "files"]);

        let selected = Output::select_datatype(&config, None).unwrap();

        assert_eq!(selected.data_type, "events");
    }

    #[test]
    fn select_datatype_uses_named_mapping_when_requested() {
        let config = plugin_config(vec!["events", "files"]);

        let selected = Output::select_datatype(&config, Some("files".to_string())).unwrap();

        assert_eq!(selected.data_type, "files");
    }

    #[test]
    fn select_datatype_rejects_empty_plugin_configuration() {
        let config = plugin_config(vec![]);

        match Output::select_datatype(&config, None) {
            Err(Error::ConfigurationError(message)) => {
                assert!(message.contains("at least one data type configuration"));
            }
            Ok(selected) => panic!("unexpectedly selected {}", selected.data_type),
            Err(err) => panic!("unexpected error: {err}"),
        }
    }

    #[test]
    fn select_datatype_rejects_unknown_requested_mapping() {
        let config = plugin_config(vec!["events"]);

        match Output::select_datatype(&config, Some("files".to_string())) {
            Err(Error::ConfigurationError(message)) => {
                assert!(message.contains("'files' not found"));
                assert!(message.contains("test-plugin"));
            }
            Ok(selected) => panic!("unexpectedly selected {}", selected.data_type),
            Err(err) => panic!("unexpected error: {err}"),
        }
    }
}
