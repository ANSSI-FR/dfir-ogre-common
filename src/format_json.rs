use std::{fs::OpenOptions, path::Path, sync::Arc};

use crate::{
    DateOutputCodec, Error, FileReport, Metadata, Record, escape_special_chars,
    line_builder::{LineBuilder, OUPUT_DATA_ID, OUTPUT_METADATA_FIELD, OUTPUT_METADATA_ID_FIELD},
    metadata::MetadataSerialized,
    output::OutputType,
    output_writer::{FileWriter, GzipWriter, Writer, try_close_all},
    serialise::JsonObjectWriter,
    timeline::TimelineSerializationContext,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsonMode {
    Standard,
    Normalized,
    Analytics,
}

impl JsonMode {
    fn uses_normalized_files(self) -> bool {
        matches!(self, Self::Normalized | Self::Analytics)
    }
}

enum JsonExtraField<'a> {
    Metadata(&'a MetadataSerialized),
    String(&'a str),
}

impl JsonExtraField<'_> {
    fn json_serialise(&self, buffer: &mut String) {
        match self {
            Self::Metadata(metadata) => buffer.push_str(metadata.0.as_str()),
            Self::String(value) => {
                buffer.push('\"');
                buffer.push_str(&escape_special_chars(value));
                buffer.push('\"');
            }
        }
    }
}

const ANALYTICS_COMPUTER_FIELD: &str = "computer";
const ANALYTICS_DATA_TYPE_FIELD: &str = "data_type";
const ANALYTICS_DATA_FIELD: &str = "data";

fn json_serialise_record_with_extra_field(
    record: &Record,
    data_id_override: Option<&str>,
    extra_key: &str,
    extra_value: JsonExtraField<'_>,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        record.json_serialise_fields_with_string_override(
            &mut object,
            date_codec,
            include_empty,
            data_id_override.map(|data_id| (OUPUT_DATA_ID, data_id)),
        )?;
        object.field_raw(extra_key, |buffer| extra_value.json_serialise(buffer));
        object.finish();
        Ok(())
    })();
    if result.is_err() {
        buffer.truncate(start_len);
    }
    result
}

fn json_serialise_analytics_record(
    record: &Record,
    data_id: &str,
    metadata: &Metadata,
    metadata_id: &str,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        object.string_field(OUPUT_DATA_ID, data_id);
        object.string_field(ANALYTICS_COMPUTER_FIELD, &metadata.computer);
        object.string_field(ANALYTICS_DATA_TYPE_FIELD, &metadata.data_type);
        object.string_field(OUTPUT_METADATA_ID_FIELD, metadata_id);
        object.try_field_raw(ANALYTICS_DATA_FIELD, |buffer| {
            record.json_serialise(buffer, date_codec, include_empty)
        })?;
        object.finish();
        Ok(())
    })();
    if result.is_err() {
        buffer.truncate(start_len);
    }
    result
}

/// A writer for outputting structured data in JSONL (JSON Lines) format.
/// This struct supports writing to either plain text files or compressed gzip files,
/// and is designed to efficiently handle large volumes of log or event data.
///
/// The `JsonOutput` struct builds each line using a `LineBuilder`, which constructs
/// a JSON string representation of a record. The lines are buffered and written in chunks
/// to minimize I/O overhead. It supports optional inclusion of empty fields and
/// configurable date formatting via `DateOutputCodec`.
///
pub struct JsonFormatter {
    data_writer: Box<dyn Writer + Send + Sync>,
    metadata_writer: Option<Box<dyn Writer + Send + Sync>>,
    timeline_writer: Option<Box<dyn Writer + Send + Sync>>,
    metadata_serialized: Option<MetadataSerialized>,
    line_buffer: String,
    output_date_codec: DateOutputCodec,
    include_empty: bool,
    include_timeline: bool,
    mode: JsonMode,
    pub file_report: FileReport,
    metadata_file_report: Option<FileReport>,
    timeline_file_report: Option<FileReport>,
}

impl JsonFormatter {
    /// Creates a new `JsonOutput` instance configured for writing to a file or a gzipped file.
    ///
    /// # Arguments
    ///
    /// * `output_type` - Specifies whether the output should be a plain `.jsonl` file or a compressed `.jsonl.gz` file.
    /// * `line_builder` - The `LineBuilder` responsible for generating the JSON representation of each record.
    /// * `output_folder` - The directory path where the output file will be created.
    /// * `file_name` - The base name of the output file (without extension).
    /// * `output_date_codec` - The codec used to format date fields in the output JSON.
    /// * `include_empty` - If `true`, fields with empty or null values will be included in the output; if `false`, they are omitted.
    /// * `compression_level` - The gzip compression level (1–9), where 1 is fastest and 9 provides the best compression.
    ///
    /// # Returns
    ///
    /// * `Ok(JsonOutput)` if the file was successfully opened and the writer initialized.
    /// * `Err(Error)` if the file could not be opened, created, or if there was an I/O error during setup.
    ///
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output_type: OutputType,
        output_folder: &str,
        file_name: &str,
        output_date_codec: DateOutputCodec,
        include_empty: bool,
        include_timeline: bool,
        compression_level: u32,
        mut file_report: FileReport,
        mode: JsonMode,
    ) -> Result<Self, Error> {
        let file_path = match output_type {
            OutputType::File => Path::new(&output_folder).join(format!("{file_name}.jsonl")),
            OutputType::Gzip => Path::new(&output_folder).join(format!("{file_name}.jsonl.gz")),
        };
        let report_template = file_report.clone();
        file_report.file_name = file_path.display().to_string();

        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(file_path)?;

        let data_writer: Box<dyn Writer + Send + Sync> = match output_type {
            OutputType::File => Box::new(FileWriter::new(file)),
            OutputType::Gzip => Box::new(GzipWriter::new(file, compression_level)),
        };

        let (metadata_writer, metadata_file_report, timeline_writer, timeline_file_report) =
            match mode.uses_normalized_files() {
                true => {
                    let metadata_file_path = match output_type {
                        OutputType::File => Path::new(&output_folder).join("ogre_metadata.jsonl"),
                        OutputType::Gzip => {
                            Path::new(&output_folder).join("ogre_metadata.jsonl.gz")
                        }
                    };
                    let metadata_file = OpenOptions::new()
                        .read(true)
                        .append(true)
                        .create(true)
                        .open(&metadata_file_path)?;

                    let metadata_writer: Box<dyn Writer + Send + Sync> = match output_type {
                        OutputType::File => Box::new(FileWriter::new(metadata_file)),
                        OutputType::Gzip => {
                            Box::new(GzipWriter::new(metadata_file, compression_level))
                        }
                    };
                    let mut metadata_file_report = report_template.clone();
                    metadata_file_report.file_name = metadata_file_path.display().to_string();
                    let (timeline_writer, timeline_file_report) = if include_timeline {
                        let timeline_file_path = match output_type {
                            OutputType::File => {
                                Path::new(&output_folder).join(format!("{file_name}.tl.jsonl"))
                            }
                            OutputType::Gzip => {
                                Path::new(&output_folder).join(format!("{file_name}.tl.jsonl.gz"))
                            }
                        };
                        let timeline_file = OpenOptions::new()
                            .read(true)
                            .append(true)
                            .create(true)
                            .open(&timeline_file_path)?;

                        let timeline_writer: Box<dyn Writer + Send + Sync> = match output_type {
                            OutputType::File => Box::new(FileWriter::new(timeline_file)),
                            OutputType::Gzip => {
                                Box::new(GzipWriter::new(timeline_file, compression_level))
                            }
                        };
                        let mut timeline_file_report = report_template.clone();
                        timeline_file_report.file_name = timeline_file_path.display().to_string();
                        (Some(timeline_writer), Some(timeline_file_report))
                    } else {
                        (None, None)
                    };

                    (
                        Some(metadata_writer),
                        Some(metadata_file_report),
                        timeline_writer,
                        timeline_file_report,
                    )
                }
                false => (None, None, None, None),
            };
        Ok(Self {
            data_writer,
            metadata_writer,
            timeline_writer,
            line_buffer: String::new(),
            output_date_codec,
            metadata_serialized: None,
            include_empty,
            include_timeline,
            mode,
            file_report,
            metadata_file_report,
            timeline_file_report,
        })
    }

    pub fn serialized_metadata(&mut self, metadata: &Metadata) -> MetadataSerialized {
        if let Some(serialized) = &self.metadata_serialized {
            serialized.clone()
        } else {
            let mut buffer = String::new();
            metadata.json_serialise(&mut buffer, &self.output_date_codec);
            let serialized = MetadataSerialized(Arc::new(buffer));
            self.metadata_serialized = Some(serialized.clone());
            serialized
        }
    }

    pub fn write(&mut self, line_builder: &LineBuilder) -> Result<(), Error> {
        let data = &line_builder.line_data;

        if self.include_timeline {
            let metadata = self.serialized_metadata(&line_builder.metadata);
            for timeline in &data.timeline {
                if timeline.timestamp.is_none() && !self.file_report.timeline_include_undated {
                    continue;
                }
                self.line_buffer.clear();
                timeline.json_serialise_with_context(
                    &mut self.line_buffer,
                    &self.output_date_codec,
                    self.include_empty,
                    TimelineSerializationContext {
                        metadata: Some(&metadata),
                        data: Some(&data.data),
                        metadata_id: timeline.metadata_id.as_deref(),
                        computer_override: None,
                        data_id_override: data.data_id.as_deref(),
                    },
                )?;
                self.line_buffer.push('\n');
                self.data_writer.write(&mut self.line_buffer)?;
                self.file_report.num_lines += 1;
            }
        } else {
            let metadata = self.serialized_metadata(&line_builder.metadata);
            self.line_buffer.clear();
            json_serialise_record_with_extra_field(
                &data.data,
                data.data_id.as_deref(),
                OUTPUT_METADATA_FIELD,
                JsonExtraField::Metadata(&metadata),
                &mut self.line_buffer,
                &self.output_date_codec,
                self.include_empty,
            )?;
            self.line_buffer.push('\n');
            self.data_writer.write(&mut self.line_buffer)?;
            self.file_report.num_lines += 1;
        }

        Ok(())
    }

    pub fn write_metadata(&mut self, line_builder: &LineBuilder) -> Result<(), Error> {
        let metadata = self.serialized_metadata(&line_builder.metadata);
        if let Some(writer) = &mut self.metadata_writer {
            self.line_buffer.clear();
            self.line_buffer.push_str(metadata.0.as_ref());
            self.line_buffer.push('\n');
            writer.write(&mut self.line_buffer)?;
            self.metadata_file_report
                .as_mut()
                .expect("metadata writer requires a file report")
                .num_lines += 1;
        }
        Ok(())
    }

    pub fn write_normalized(&mut self, line_builder: &LineBuilder) -> Result<(), Error> {
        debug_assert!(self.mode.uses_normalized_files());
        let data = &line_builder.line_data;
        if self.include_timeline
            && let Some(timeline_writer) = &mut self.timeline_writer
        {
            for timeline in &data.timeline {
                if timeline.timestamp.is_none() && !self.file_report.timeline_include_undated {
                    continue;
                }
                self.line_buffer.clear();
                timeline.json_serialise_with_context(
                    &mut self.line_buffer,
                    &self.output_date_codec,
                    self.include_empty,
                    TimelineSerializationContext {
                        metadata: timeline.metadata.as_ref(),
                        data: timeline.data.as_ref(),
                        metadata_id: Some(&line_builder.metadata_id),
                        computer_override: match self.mode {
                            JsonMode::Analytics => Some(line_builder.metadata.computer.as_str()),
                            JsonMode::Standard | JsonMode::Normalized => None,
                        },
                        data_id_override: None,
                    },
                )?;
                self.line_buffer.push('\n');
                timeline_writer.write(&mut self.line_buffer)?;
                self.timeline_file_report
                    .as_mut()
                    .expect("timeline writer requires a file report")
                    .num_lines += 1;
            }
        }

        self.line_buffer.clear();
        if self.mode == JsonMode::Analytics {
            let data_id = data.data_id.as_deref().ok_or_else(|| {
                Error::PluginError("analytics_jsonl requires a generated data id".to_owned())
            })?;
            json_serialise_analytics_record(
                &data.data,
                data_id,
                &line_builder.metadata,
                &line_builder.metadata_id,
                &mut self.line_buffer,
                &self.output_date_codec,
                self.include_empty,
            )?;
        } else {
            json_serialise_record_with_extra_field(
                &data.data,
                data.data_id.as_deref(),
                OUTPUT_METADATA_ID_FIELD,
                JsonExtraField::String(&line_builder.metadata_id),
                &mut self.line_buffer,
                &self.output_date_codec,
                self.include_empty,
            )?;
        }

        self.line_buffer.push('\n');
        self.data_writer.write(&mut self.line_buffer)?;
        self.file_report.num_lines += 1;
        Ok(())
    }

    pub fn reports(&self) -> Vec<FileReport> {
        let mut reports = vec![self.file_report.clone()];
        reports.extend(self.metadata_file_report.clone());
        reports.extend(self.timeline_file_report.clone());
        reports
    }

    /// Closes the output stream and ensures all buffered data is written and flushed.
    ///
    /// This method finalizes the output process by flushing the underlying writer and, in the case of
    /// gzip compression, finishing the compression stream. It is essential to call this method after
    /// all data has been written to ensure no data is lost due to buffering.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the stream was successfully closed and all data was flushed.
    /// * `Err(Error)` if an I/O error occurs during the closing process (e.g., disk error).
    ///
    pub fn close(&mut self) -> Result<(), Error> {
        let writers = [
            Some(self.data_writer.as_mut()),
            self.metadata_writer.as_deref_mut(),
            self.timeline_writer.as_deref_mut(),
        ];
        try_close_all(writers.into_iter().flatten(), |writer| writer.close())
    }
}

#[cfg(test)]
mod tests {
    pub const TMP_FOLDER: &str = ".tmp";
    use std::{
        fs::{self, File},
        io::Read,
    };

    use flate2::read::GzDecoder;

    use crate::{
        DateInputCodec, Field, FieldMapping, FieldName, Metadata, Parser, Record, TimeLineBuilder,
        TimeLineType, Value,
    };

    use super::*;

    fn remove_if_exists(path: &Path) {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }

    fn remove_dir_if_exists(path: &Path) {
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }

    fn timeline_field_mapping_for_two_dates() -> FieldMapping {
        FieldMapping::new(
            vec![
                Field::Single {
                    name: FieldName::new("when_a".into(), false, None, None, None),
                    parser: Parser::DateTime(DateInputCodec::Iso()),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("when_b".into(), false, None, None, None),
                    parser: Parser::DateTime(DateInputCodec::Iso()),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("message".into(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
            None,
        )
    }

    /// Tests writing to a plain text file.
    /// It checks that each line is properly formatted in JSON and terminated with a newline,
    /// and that the output matches the expected content.
    #[test]
    fn file() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            true,
        );
        let mut ouput = JsonFormatter::new(
            OutputType::File,
            TMP_FOLDER,
            "test_file",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Standard,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        record.add("greetings", Value::String("World".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        drop(ouput);

        let path = Path::new(TMP_FOLDER).join("test_file.jsonl");
        let message: String = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();
        let expected = "{\"greetings\":\"Hello\",\"ogre_md\":{\"computer\":\"test\",\"data_type\":\"\"}}\n{\"greetings\":\"World\",\"ogre_md\":{\"computer\":\"test\",\"data_type\":\"\"}}\n";

        assert_eq!(expected, message)
    }

    #[test]
    fn serialized_metadata_reuses_cached_value() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let path = Path::new(TMP_FOLDER).join("test_metadata_cache.jsonl");
        remove_if_exists(&path);

        let mut output = JsonFormatter::new(
            OutputType::File,
            TMP_FOLDER,
            "test_metadata_cache",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Standard,
        )
        .unwrap();
        let mut metadata = Metadata::new("host-a".into());
        metadata.data_type = "evtx".into();

        let first = output.serialized_metadata(&metadata);
        metadata.computer = "host-b".into();
        let second = output.serialized_metadata(&metadata);

        assert!(std::sync::Arc::ptr_eq(&first.0, &second.0));
        assert_eq!(
            first.0.as_ref(),
            "{\"computer\":\"host-a\",\"data_type\":\"evtx\"}"
        );

        drop(output);
        remove_if_exists(&path);
    }

    #[test]
    fn serialise_record_with_extra_field_does_not_mutate_record() {
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        let metadata = MetadataSerialized(Arc::new(
            "{\"computer\":\"test\",\"data_type\":\"\"}".to_owned(),
        ));
        let mut buffer = String::new();

        json_serialise_record_with_extra_field(
            &record,
            None,
            OUTPUT_METADATA_FIELD,
            JsonExtraField::Metadata(&metadata),
            &mut buffer,
            &DateOutputCodec::Iso(),
            false,
        )
        .unwrap();

        assert_eq!(
            "{\"greetings\":\"Hello\",\"ogre_md\":{\"computer\":\"test\",\"data_type\":\"\"}}",
            buffer
        );
        assert!(!record.contains_key(OUTPUT_METADATA_FIELD));
    }

    #[test]
    fn record_extra_field_serializer_projects_generated_data_id() {
        let mut record = Record::new();
        record.add("event", Value::String("login".to_owned()));
        let original = record.clone();
        let mut buffer = String::new();

        json_serialise_record_with_extra_field(
            &record,
            Some("generated-id"),
            OUTPUT_METADATA_ID_FIELD,
            JsonExtraField::String("metadata-id"),
            &mut buffer,
            &DateOutputCodec::Iso(),
            false,
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed["event"], "login");
        assert_eq!(parsed[OUPUT_DATA_ID], "generated-id");
        assert_eq!(parsed[OUTPUT_METADATA_ID_FIELD], "metadata-id");
        assert_eq!(record, original);
    }

    #[test]
    fn analytics_record_wraps_the_complete_unfiltered_record() {
        let mut record = Record::new();
        record.add("event", Value::String("login".to_owned()));
        record.add("ogre_id", Value::String("artifact-id".to_owned()));
        record.add("computer", Value::String("event-host".to_owned()));
        record.add("data_type", Value::String("artifact-type".to_owned()));
        record.add("ogre_md_id", Value::String("artifact-md-id".to_owned()));
        record.add("data", Value::String("artifact-data-field".to_owned()));
        let original = record.clone();

        let mut metadata = Metadata::new("metadata\"host\n".to_owned());
        metadata.data_type = "events\\kind".to_owned();
        let mut buffer = String::new();

        json_serialise_analytics_record(
            &record,
            "generated-id",
            &metadata,
            "metadata-id",
            &mut buffer,
            &DateOutputCodec::Iso(),
            false,
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), 5);
        assert_eq!(parsed["ogre_id"], "generated-id");
        assert_eq!(parsed["computer"], "metadata\"host\n");
        assert_eq!(parsed["data_type"], "events\\kind");
        assert_eq!(parsed["ogre_md_id"], "metadata-id");
        assert_eq!(parsed["data"]["event"], "login");
        assert_eq!(parsed["data"]["ogre_id"], "artifact-id");
        assert_eq!(parsed["data"]["computer"], "event-host");
        assert_eq!(parsed["data"]["data_type"], "artifact-type");
        assert_eq!(parsed["data"]["ogre_md_id"], "artifact-md-id");
        assert_eq!(parsed["data"]["data"], "artifact-data-field");
        assert_eq!(record, original);
    }

    #[test]
    fn analytics_record_emits_empty_metadata_dimensions_when_empty_fields_are_excluded() {
        let mut record = Record::new();
        record.add("empty", Value::Null());
        let metadata = Metadata::new(String::new());
        let mut buffer = String::new();

        json_serialise_analytics_record(
            &record,
            "generated-id",
            &metadata,
            "metadata-id",
            &mut buffer,
            &DateOutputCodec::Iso(),
            false,
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed["ogre_id"], "generated-id");
        assert_eq!(parsed["computer"], "");
        assert_eq!(parsed["data_type"], "");
        assert_eq!(parsed["ogre_md_id"], "metadata-id");
        assert_eq!(parsed["data"], serde_json::json!({}));
    }

    #[test]
    fn normalized_file_writes_metadata_and_data_records() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let data_path = Path::new(TMP_FOLDER).join("test_normalized_json.jsonl");
        let metadata_path = Path::new(TMP_FOLDER).join("ogre_metadata.jsonl");
        remove_if_exists(&data_path);
        remove_if_exists(&metadata_path);

        let mut line_builder = LineBuilder::new(
            Metadata::new("host-json".into()),
            None,
            FieldMapping::new(vec![], None),
            true,
            false,
            true,
        );
        let mut output = JsonFormatter::new(
            OutputType::File,
            TMP_FOLDER,
            "test_normalized_json",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Normalized,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();

        output.write_metadata(&line_builder).unwrap();
        output.write_normalized(&line_builder).unwrap();
        let reports = output.reports();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].num_lines, 1);
        assert!(reports[0].file_name.ends_with("test_normalized_json.jsonl"));
        assert_eq!(reports[1].num_lines, 1);
        assert!(reports[1].file_name.ends_with("ogre_metadata.jsonl"));

        drop(output);

        let metadata_json: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&metadata_path).unwrap().trim()).unwrap();
        assert_eq!(metadata_json["computer"], "host-json");
        assert!(metadata_json["id"].as_str().is_some());

        let data_json: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&data_path).unwrap().trim()).unwrap();
        assert_eq!(data_json["greetings"], "Hello");
        assert!(data_json["ogre_md_id"].as_str().unwrap().len() > 10);
        assert!(data_json["ogre_id"].as_str().unwrap().len() > 10);

        remove_if_exists(&data_path);
        remove_if_exists(&metadata_path);
    }

    #[test]
    fn normalized_file_writes_timeline_next_to_data_file() {
        let output_folder = Path::new(TMP_FOLDER).join("test_normalized_json_timeline_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder = output_folder.to_str().unwrap();
        let base_file_name = "test_normalized_json_timeline";
        let data_path = Path::new(output_folder).join(format!("{base_file_name}.jsonl"));
        let metadata_path = Path::new(output_folder).join("ogre_metadata.jsonl");
        let timeline_path = Path::new(output_folder).join(format!("{base_file_name}.tl.jsonl"));
        let repeated_folder_path = Path::new(output_folder).join(TMP_FOLDER);

        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "json_test".to_string(),
            100,
            None,
            None,
        );
        let field_mapping = FieldMapping::new(
            vec![
                Field::Single {
                    name: FieldName::new("when".into(), false, None, None, None),
                    parser: Parser::DateTime(DateInputCodec::Iso()),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("greetings".into(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("host-json-timeline".into()),
            Some(timeline_builder),
            field_mapping,
            true,
            false,
            true,
        );
        let mut output = JsonFormatter::new(
            OutputType::File,
            output_folder,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Normalized,
        )
        .unwrap();
        let mut record = Record::new();
        let timestamp =
            crate::date_util::parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        record.add("when", Value::Date(timestamp));
        record.add("greetings", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();

        output.write_metadata(&line_builder).unwrap();
        output.write_normalized(&line_builder).unwrap();
        assert_eq!(output.file_report.num_lines, 1);
        output.close().unwrap();

        assert!(data_path.exists());
        assert!(metadata_path.exists());
        assert!(timeline_path.exists());
        assert!(!repeated_folder_path.exists());

        let data_json: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&data_path).unwrap().trim()).unwrap();
        let timeline_json: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&timeline_path).unwrap().trim()).unwrap();
        assert_eq!(data_json["greetings"], "Hello");
        assert_eq!(timeline_json["computer"], "host-json-timeline");
        assert_eq!(timeline_json["timestamp_meaning"], "when");
        assert_eq!(timeline_json["data_type"], "json_test");
        assert_eq!(timeline_json["data_id"], data_json["ogre_id"]);
        assert!(timeline_json["metadata_id"].as_str().unwrap().len() > 10);

        remove_dir_if_exists(Path::new(output_folder));
    }

    #[test]
    fn timeline_json_write_does_not_mutate_source_timelines() {
        let output_folder = Path::new(TMP_FOLDER).join("test_json_timeline_no_mutation_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder = output_folder.to_str().unwrap();
        let base_file_name = "test_json_timeline_no_mutation";
        let data_path = Path::new(output_folder).join(format!("{base_file_name}.jsonl"));

        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "json_test".to_string(),
            100,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("host-json-timeline".into()),
            Some(timeline_builder),
            timeline_field_mapping_for_two_dates(),
            false,
            false,
            true,
        );
        let mut output = JsonFormatter::new(
            OutputType::File,
            output_folder,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Standard,
        )
        .unwrap();

        let mut record = Record::new();
        let timestamp_a =
            crate::date_util::parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        let timestamp_b =
            crate::date_util::parse_date("2020-01-02T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        record.add("when_a", Value::Date(timestamp_a));
        record.add("when_b", Value::Date(timestamp_b));
        record.add("message", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();

        assert_eq!(line_builder.line_data.timeline.len(), 2);
        output.write(&line_builder).unwrap();
        output.close().unwrap();

        for timeline in &line_builder.line_data.timeline {
            assert!(timeline.metadata.is_none());
            assert!(timeline.data.is_none());
        }

        let output_text = fs::read_to_string(&data_path).unwrap();
        let lines: Vec<&str> = output_text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(parsed["metadata"]["computer"], "host-json-timeline");
            assert_eq!(parsed["data"]["message"], "Hello");
        }

        remove_dir_if_exists(Path::new(output_folder));
    }

    #[test]
    fn normalized_json_timeline_uses_formatter_metadata_id() {
        let output_folder = Path::new(TMP_FOLDER).join("test_json_timeline_md_id_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder = output_folder.to_str().unwrap();
        let base_file_name = "test_json_timeline_md_id";
        let timeline_path = Path::new(output_folder).join(format!("{base_file_name}.tl.jsonl"));

        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "json_test".to_string(),
            100,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("host-json-timeline".into()),
            Some(timeline_builder),
            timeline_field_mapping_for_two_dates(),
            true,
            false,
            true,
        );
        let mut output = JsonFormatter::new(
            OutputType::File,
            output_folder,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Normalized,
        )
        .unwrap();

        let mut record = Record::new();
        let timestamp_a =
            crate::date_util::parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        let timestamp_b =
            crate::date_util::parse_date("2020-01-02T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        record.add("when_a", Value::Date(timestamp_a));
        record.add("when_b", Value::Date(timestamp_b));
        record.add("message", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();
        line_builder.line_data.timeline[0].metadata_id = Some("stale-md-id".to_owned());

        output.write_normalized(&line_builder).unwrap();
        output.close().unwrap();

        let timeline_text = fs::read_to_string(&timeline_path).unwrap();
        let first_line = timeline_text.lines().next().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(first_line).unwrap();
        assert_eq!(parsed["metadata_id"], line_builder.metadata_id);

        remove_dir_if_exists(Path::new(output_folder));
    }

    #[test]
    fn analytics_timeline_writer_overrides_stale_computer_from_metadata() {
        let output_folder = Path::new(TMP_FOLDER).join("test_analytics_timeline_override_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder_str = output_folder.to_str().unwrap();
        let timeline_path = output_folder.join("analytics.events.tl.jsonl");

        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "timeline-events".to_owned(),
            100,
            None,
            None,
        );
        let mut metadata = Metadata::new("metadata-host".into());
        metadata.data_type = "events".into();
        let mut line_builder = LineBuilder::new(
            metadata,
            Some(timeline_builder),
            timeline_field_mapping_for_two_dates(),
            true,
            false,
            true,
        );
        let mut record = Record::new();
        record.add(
            "when_a",
            Value::Date(crate::parse_date("2026-08-06T12:00:00Z", &DateInputCodec::Iso()).unwrap()),
        );
        record.add("message", Value::String("login".to_owned()));
        line_builder.build(&mut record).unwrap();
        line_builder.line_data.timeline[0].computer = Some("stale-host".to_owned());

        let mut output = JsonFormatter::new(
            OutputType::File,
            output_folder_str,
            "analytics.events",
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                format: "analytics_jsonl".to_owned(),
                with_timeline: true,
                ..Default::default()
            },
            JsonMode::Analytics,
        )
        .unwrap();

        output.write_normalized(&line_builder).unwrap();
        output.close().unwrap();

        let timeline: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&timeline_path).unwrap().trim()).unwrap();
        assert_eq!(timeline["computer"], "metadata-host");
        assert_eq!(
            line_builder.line_data.timeline[0].computer.as_deref(),
            Some("stale-host")
        );

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn normalized_gzip_close_finishes_metadata_stream() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let data_path = Path::new(TMP_FOLDER).join("test_normalized_json_gz.jsonl.gz");
        let metadata_path = Path::new(TMP_FOLDER).join("ogre_metadata.jsonl.gz");
        remove_if_exists(&data_path);
        remove_if_exists(&metadata_path);

        let mut line_builder = LineBuilder::new(
            Metadata::new("host-json-gz".into()),
            None,
            FieldMapping::new(vec![], None),
            true,
            false,
            true,
        );
        let mut output = JsonFormatter::new(
            OutputType::Gzip,
            TMP_FOLDER,
            "test_normalized_json_gz",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Normalized,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();

        output.write_metadata(&line_builder).unwrap();
        output.write_normalized(&line_builder).unwrap();
        output.close().unwrap();

        let mut metadata_decoder = GzDecoder::new(File::open(&metadata_path).unwrap());
        let mut metadata = String::new();
        metadata_decoder.read_to_string(&mut metadata).unwrap();
        let metadata_json: serde_json::Value = serde_json::from_str(metadata.trim()).unwrap();
        assert_eq!(metadata_json["computer"], "host-json-gz");

        let mut data_decoder = GzDecoder::new(File::open(&data_path).unwrap());
        let mut data = String::new();
        data_decoder.read_to_string(&mut data).unwrap();
        let data_json: serde_json::Value = serde_json::from_str(data.trim()).unwrap();
        assert_eq!(data_json["greetings"], "Hello");

        remove_if_exists(&data_path);
        remove_if_exists(&metadata_path);
    }

    #[test]
    fn analytics_gzip_finishes_enriched_data_and_metadata_streams() {
        let output_folder = Path::new(TMP_FOLDER).join("test_analytics_json_gzip_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder_str = output_folder.to_str().unwrap();
        let data_path = output_folder.join("analytics.events.jsonl.gz");
        let metadata_path = output_folder.join("ogre_metadata.jsonl.gz");

        let mut metadata = Metadata::new("host-json-gz".into());
        metadata.data_type = "events".into();
        let mut line_builder = LineBuilder::new(
            metadata,
            None,
            FieldMapping::new(vec![], None),
            true,
            false,
            true,
        );
        let mut output = JsonFormatter::new(
            OutputType::Gzip,
            output_folder_str,
            "analytics.events",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport {
                format: "analytics_jsonl".to_owned(),
                ..Default::default()
            },
            JsonMode::Analytics,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("event", Value::String("login".to_owned()));
        line_builder.build(&mut record).unwrap();

        output.write_metadata(&line_builder).unwrap();
        output.write_normalized(&line_builder).unwrap();
        output.close().unwrap();

        let mut data_decoder = GzDecoder::new(File::open(&data_path).unwrap());
        let mut data = String::new();
        data_decoder.read_to_string(&mut data).unwrap();
        let data: serde_json::Value = serde_json::from_str(data.trim()).unwrap();

        let mut metadata_decoder = GzDecoder::new(File::open(&metadata_path).unwrap());
        let mut metadata = String::new();
        metadata_decoder.read_to_string(&mut metadata).unwrap();
        let metadata: serde_json::Value = serde_json::from_str(metadata.trim()).unwrap();

        assert_eq!(data["computer"], "host-json-gz");
        assert_eq!(data["data_type"], "events");
        assert_eq!(data["data"]["event"], "login");
        assert!(data.get("event").is_none());
        assert_eq!(metadata["computer"], "host-json-gz");
        assert_eq!(metadata["data_type"], "events");

        remove_dir_if_exists(&output_folder);
    }

    /// Tests writing to a gzipped file.
    /// It verifies that the data is correctly compressed and can be decompressed back to the original JSONL format.
    #[test]
    fn gzip() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            true,
        );
        let mut ouput = JsonFormatter::new(
            OutputType::Gzip,
            TMP_FOLDER,
            "test_gz",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport {
                ..Default::default()
            },
            JsonMode::Standard,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        record.add("greetings", Value::String("World".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        drop(ouput); //closes the file properly
        let path = Path::new(TMP_FOLDER).join("test_gz.jsonl.gz");

        let mut d = GzDecoder::new(File::open(&path).unwrap());
        let mut buf = String::new();

        d.read_to_string(&mut buf).unwrap();
        drop(d);
        fs::remove_file(path).unwrap();
        let expected = "{\"greetings\":\"Hello\",\"ogre_md\":{\"computer\":\"test\",\"data_type\":\"\"}}\n{\"greetings\":\"World\",\"ogre_md\":{\"computer\":\"test\",\"data_type\":\"\"}}\n";

        assert_eq!(expected, buf)
    }
}
