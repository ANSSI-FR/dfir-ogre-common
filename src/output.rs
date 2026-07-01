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
    outputs: Vec<OutputWriter>,
    line_builder: LineBuilder,
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
            outputs.push(output_writer)
        }

        let field_mapping = match &data_type_conf.field_mapping {
            Some(field_mapping) => field_mapping.clone(),
            None => FieldMapping::new(vec![], None),
        };

        let timeline_builder = match compute_timeline {
            true => data_type_conf.timeline.clone(),
            false => None,
        };

        let line_builder = LineBuilder::new(
            metadata,
            timeline_builder,
            field_mapping,
            compute_hash,
            data_type_conf.has_primary_key,
            run_config.force_snake_case,
        );

        //write metadata for normalized output
        for writer in &mut outputs {
            writer.write_metadata(&line_builder)?;
        }

        Ok(Self {
            outputs,
            line_builder,
        })
    }

    pub fn write(&mut self, data: &mut Record) -> Result<(), Error> {
        self.line_builder.build(data)?;

        for output in &mut self.outputs {
            output.write(&self.line_builder)?;
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
        for out in &mut self.outputs {
            let _ = out.close();
        }
    }

    pub fn get_report(&self) -> OutputReport {
        let mut report = OutputReport {
            ..Default::default()
        };

        for out in &self.outputs {
            report.file_reports.push(out.result());
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DateInputCodec, Field, FieldName, OutputConfiguration, Parser, Value,
        configuration::DataTypeMapping, format_csv::CSV_DELIMITER,
    };
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

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

    fn remove_dir_if_exists(path: &Path) {
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }

    fn test_output_folder(name: &str) -> PathBuf {
        let path = Path::new(".tmp").join(name);
        remove_dir_if_exists(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn mapped_plugin_config(data_type: &str, field_mapping: FieldMapping) -> PluginConfiguration {
        PluginConfiguration {
            plugin: "test-plugin".to_string(),
            file_encoding: "UTF_8".to_string(),
            data_type_configs: vec![DataTypeMapping {
                data_type: data_type.to_string(),
                description: None,
                default_date_pattern: DateInputCodec::Iso(),
                params: HashMap::new(),
                timeline: None,
                field_mapping: Some(field_mapping),
                has_primary_key: false,
            }],
        }
    }

    fn event_field_mapping() -> FieldMapping {
        FieldMapping::new(
            vec![Field::Single {
                name: FieldName::new("event".to_owned(), false, None, None, None),
                parser: Parser::String(),
                default_value: None,
            }],
            None,
        )
    }

    fn normalized_output_config(
        base_file_name: &str,
        output_folder: &Path,
        format: &str,
    ) -> OutputConfiguration {
        OutputConfiguration::new(
            base_file_name.to_string(),
            output_folder.display().to_string(),
            "file".to_string(),
            format.to_string(),
            "iso".to_string(),
            false,
            true,
            HashMap::new(),
        )
    }

    fn json_output_config(
        base_file_name: &str,
        output_folder: &Path,
        params: HashMap<String, String>,
    ) -> OutputConfiguration {
        OutputConfiguration::new(
            base_file_name.to_string(),
            output_folder.display().to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            true,
            params,
        )
    }

    #[test]
    fn normalized_outputs_dispatch_data_and_write_metadata() {
        let output_folder = test_output_folder("output_normalized_dispatch");
        let base_file_name = "dispatch";
        let field_mapping = event_field_mapping();
        let run_config = RunConfiguration::new(
            vec![
                normalized_output_config(base_file_name, &output_folder, "normalized_jsonl"),
                normalized_output_config(base_file_name, &output_folder, "normalized_csv"),
            ],
            true,
            None,
        );
        let plugin_config = mapped_plugin_config("events", field_mapping);
        let mut output = Output::new(
            run_config,
            plugin_config,
            Metadata::new("host-output".into()),
            None,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("event", Value::String("login".to_owned()));

        output.write(&mut record).unwrap();
        for writer in &mut output.outputs {
            writer.close().unwrap();
        }
        let report = output.get_report();
        drop(output);

        assert!(record.is_empty());
        assert_eq!(report.file_reports.len(), 2);
        assert!(
            report
                .file_reports
                .iter()
                .all(|file_report| file_report.num_lines == 1)
        );

        let json_data_path = output_folder.join("dispatch.events.jsonl");
        let json_data: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&json_data_path).unwrap().trim()).unwrap();
        assert_eq!(json_data["event"], "login");
        assert!(json_data["ogre_md_id"].as_str().unwrap().len() > 10);

        let json_metadata_path = output_folder.join("ogre_metadata.jsonl");
        let json_metadata: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&json_metadata_path).unwrap().trim()).unwrap();
        assert_eq!(json_metadata["computer"], "host-output");
        assert_eq!(json_metadata["data_type"], "events");

        let csv_data_path = output_folder.join("dispatch.events.csv");
        let csv_data = fs::read_to_string(&csv_data_path).unwrap();
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(csv_data.as_bytes());
        let headers = csv_reader.headers().unwrap().clone();
        assert_eq!(&headers[0], "event");
        assert_eq!(&headers[1], "ogre_id");
        assert_eq!(&headers[2], "ogre_md_id");
        let csv_record = csv_reader.records().next().unwrap().unwrap();
        assert_eq!(&csv_record[0], "login");
        assert!(csv_record[1].len() > 10);
        assert!(csv_record[2].len() > 10);

        let csv_metadata_path = output_folder.join("ogre_metadata.csv");
        let metadata_csv = fs::read_to_string(&csv_metadata_path).unwrap();
        let mut metadata_reader = csv::ReaderBuilder::new()
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(metadata_csv.as_bytes());
        let metadata_record = metadata_reader.records().next().unwrap().unwrap();
        assert_eq!(&metadata_record[0], "host-output");
        assert_eq!(&metadata_record[1], "events");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn output_new_rejects_invalid_compression_level_param() {
        let output_folder = test_output_folder("output_bad_compression");
        let mut params = HashMap::new();
        params.insert(COMPRESSION_LEVEL.to_owned(), "fast".to_owned());
        let run_config = RunConfiguration::new(
            vec![json_output_config(
                "bad_compression",
                &output_folder,
                params,
            )],
            true,
            None,
        );
        let plugin_config = mapped_plugin_config("events", event_field_mapping());

        let result = Output::new(
            run_config,
            plugin_config,
            Metadata::new("host-output".into()),
            None,
        );

        assert!(result.is_err());
        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn multiple_json_outputs_receive_plain_records() {
        let output_folder = test_output_folder("output_multiple_plain");
        let field_mapping = FieldMapping::new(
            vec![Field::Single {
                name: FieldName::new("event".to_owned(), false, None, None, None),
                parser: Parser::String(),
                default_value: None,
            }],
            None,
        );
        let run_config = RunConfiguration::new(
            vec![
                json_output_config("first", &output_folder, HashMap::new()),
                json_output_config("second", &output_folder, HashMap::new()),
            ],
            true,
            None,
        );
        let plugin_config = mapped_plugin_config("events", field_mapping);
        let mut output = Output::new(
            run_config,
            plugin_config,
            Metadata::new("host-output".into()),
            None,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("event", Value::String("login".to_owned()));

        output.write(&mut record).unwrap();
        for writer in &mut output.outputs {
            writer.close().unwrap();
        }
        drop(output);

        let first: serde_json::Value = serde_json::from_str(
            fs::read_to_string(output_folder.join("first.events.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        let second: serde_json::Value = serde_json::from_str(
            fs::read_to_string(output_folder.join("second.events.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap();

        assert_eq!(first["event"], "login");
        assert_eq!(second["event"], "login");

        remove_dir_if_exists(&output_folder);
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
