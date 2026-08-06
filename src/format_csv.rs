use std::{fs::OpenOptions, path::Path, sync::Arc};

use crate::{
    Error, FieldMapping, FileReport, Metadata, Record, Value,
    date_util::DateOutputCodec,
    line_builder::{LineBuilder, OUPUT_DATA_ID, OUTPUT_METADATA_FIELD, OUTPUT_METADATA_ID_FIELD},
    metadata::MetadataSerialized,
    output::OutputType,
    output_writer::{FileWriter, GzipWriter, Writer, try_close_all},
    serialise::CsvRowWriter,
    timeline::{TimeLine, TimelineSerializationContext},
};
pub const CSV_DELIMITER: char = ';';

fn open_writer(
    path: &Path,
    output_type: &OutputType,
    compression_level: u32,
    header: Option<String>,
) -> Result<Box<dyn Writer + Send + Sync>, Error> {
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)?;
    let is_empty = file.metadata()?.len() == 0;

    let mut writer: Box<dyn Writer + Send + Sync> = match output_type {
        OutputType::File => Box::new(FileWriter::new(file)),
        OutputType::Gzip => Box::new(GzipWriter::new(file, compression_level)),
    };

    if is_empty && let Some(mut header) = header {
        header.push('\n');
        writer.write(&mut header)?;
    }

    Ok(writer)
}

enum CsvExtraField<'a> {
    Metadata(&'a MetadataSerialized),
    String(&'a str),
}

impl CsvExtraField<'_> {
    fn write_to_row(&self, row: &mut CsvRowWriter<'_>) {
        match self {
            Self::Metadata(metadata) => row.quoted_field(metadata.0.as_str()),
            Self::String(value) => row.quoted_field(value),
        }
    }
}

fn csv_serialise_record_with_extra_field(
    record: &Record,
    record_fields: Option<&[String]>,
    data_id_override: Option<&str>,
    extra_value: CsvExtraField<'_>,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
) -> Result<(), Error> {
    let mut row = CsvRowWriter::new(buffer);
    if let Some(record_fields) = record_fields {
        for field in record_fields {
            if field == OUPUT_DATA_ID
                && let Some(data_id) = data_id_override
            {
                row.quoted_field(data_id);
            } else if let Some(value) = record.get(field) {
                row.try_raw_field(|buffer| Value::csv_serialise_value(value, buffer, date_codec))?;
            } else {
                row.empty_field();
            }
        }
    } else {
        let mut override_written = false;
        for (key, value) in record.iter() {
            if key == OUPUT_DATA_ID
                && let Some(data_id) = data_id_override
            {
                row.quoted_field(data_id);
                override_written = true;
            } else {
                row.try_raw_field(|buffer| Value::csv_serialise_value(value, buffer, date_codec))?;
            }
        }
        if let Some(data_id) = data_id_override
            && !override_written
        {
            row.quoted_field(data_id);
        }
    }
    extra_value.write_to_row(&mut row);
    Ok(())
}

/// A writer for outputting structured data in CSV format.
/// This struct supports writing to either plain text files or compressed gzip files,
/// and is designed to efficiently handle large volumes of log or event data.
///
/// The `CSVOutput` struct builds each line using a `LineBuilder`, which constructs
/// a CSV string representation of a record. The lines are buffered and written in chunks
/// to minimize I/O overhead. It supports optional inclusion of empty fields and
/// configurable date formatting via `DateOutputCodec`.
///
pub struct CsvFormatter {
    data_writer: Box<dyn Writer + Send + Sync>,
    metadata_writer: Option<Box<dyn Writer + Send + Sync>>,
    timeline_writer: Option<Box<dyn Writer + Send + Sync>>,
    metadata_serialized: Option<MetadataSerialized>,
    line_buffer: String,
    record_fields: Option<Vec<String>>,
    output_date_codec: DateOutputCodec,
    include_empty: bool,
    pub file_report: FileReport,
    metadata_file_report: Option<FileReport>,
    timeline_file_report: Option<FileReport>,
    include_timeline: bool,
}
impl CsvFormatter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        field_mapping: Option<FieldMapping>,
        output_type: OutputType,
        output_folder: &str,
        file_name: &str,
        output_date_codec: DateOutputCodec,
        include_empty: bool,
        include_timeline: bool,
        compression_level: u32,
        mut file_report: FileReport,
        normalized: bool,
    ) -> Result<Self, Error> {
        let file_path = match output_type {
            OutputType::File => Path::new(&output_folder).join(format!("{file_name}.csv")),
            OutputType::Gzip => Path::new(&output_folder).join(format!("{file_name}.csv.gz")),
        };
        let report_template = file_report.clone();
        file_report.file_name = file_path.display().to_string();

        let (header, record_fields) = if let Some(mapping) = field_mapping {
            let mut record_fields = mapping
                .field_parser_tree
                .output_fields
                .iter()
                .map(|field| field.output_name().to_owned())
                .collect::<Vec<_>>();
            if normalized {
                record_fields.push(OUPUT_DATA_ID.to_owned());
            }

            let mut header = String::new();
            if include_timeline && !normalized {
                TimeLine::csv_serialise_header(&mut header, normalized)
            } else {
                build_standard_header(&mut header, normalized, &record_fields)?;
            }
            (Some(header), Some(record_fields))
        } else {
            (None, None)
        };
        let data_writer = open_writer(&file_path, &output_type, compression_level, header)?;

        //create a metadata file writer if the output is normalized
        let (metadata_writer, metadata_file_report) = match normalized {
            true => {
                let file_path = match output_type {
                    OutputType::File => Path::new(&output_folder).join("ogre_metadata.csv"),
                    OutputType::Gzip => Path::new(&output_folder).join("ogre_metadata.csv.gz"),
                };
                let mut header = String::new();
                Metadata::csv_serialise_header(&mut header);
                let metadata_writer =
                    open_writer(&file_path, &output_type, compression_level, Some(header))?;
                let mut metadata_file_report = report_template.clone();
                metadata_file_report.file_name = file_path.display().to_string();
                (Some(metadata_writer), Some(metadata_file_report))
            }
            false => (None, None),
        };

        //create a timeline file writer if the output is normalized
        let (timeline_writer, timeline_file_report) = match normalized {
            true => {
                if include_timeline {
                    let file_path = match output_type {
                        OutputType::File => {
                            Path::new(&output_folder).join(format!("{file_name}.tl.csv"))
                        }
                        OutputType::Gzip => {
                            Path::new(&output_folder).join(format!("{file_name}.tl.csv.gz"))
                        }
                    };
                    let mut header = String::new();
                    TimeLine::csv_serialise_header(&mut header, normalized);
                    let timeline_writer =
                        open_writer(&file_path, &output_type, compression_level, Some(header))?;
                    let mut timeline_file_report = report_template;
                    timeline_file_report.file_name = file_path.display().to_string();
                    (Some(timeline_writer), Some(timeline_file_report))
                } else {
                    (None, None)
                }
            }
            false => (None, None),
        };

        Ok(Self {
            data_writer,
            metadata_writer,
            timeline_writer,
            metadata_serialized: None,
            line_buffer: String::new(),
            record_fields,
            output_date_codec,
            include_empty,
            file_report,
            metadata_file_report,
            timeline_file_report,
            include_timeline,
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
                timeline.csv_serialise_with_context(
                    &mut self.line_buffer,
                    &self.output_date_codec,
                    self.include_empty,
                    false,
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

            csv_serialise_record_with_extra_field(
                &data.data,
                self.record_fields.as_deref(),
                data.data_id.as_deref(),
                CsvExtraField::Metadata(&metadata),
                &mut self.line_buffer,
                &self.output_date_codec,
            )?;

            self.line_buffer.push('\n');
            self.data_writer.write(&mut self.line_buffer)?;
            self.file_report.num_lines += 1;
        }
        Ok(())
    }

    pub fn write_metadata(&mut self, line_builder: &LineBuilder) -> Result<(), Error> {
        if let Some(writer) = &mut self.metadata_writer {
            self.line_buffer.clear();
            line_builder
                .metadata
                .csv_serialise(&mut self.line_buffer, &self.output_date_codec);

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
        let data = &line_builder.line_data;
        if self.include_timeline
            && let Some(timeline_writer) = &mut self.timeline_writer
        {
            for timeline in &data.timeline {
                if timeline.timestamp.is_none() && !self.file_report.timeline_include_undated {
                    continue;
                }
                self.line_buffer.clear();
                timeline.csv_serialise_with_context(
                    &mut self.line_buffer,
                    &self.output_date_codec,
                    self.include_empty,
                    true,
                    TimelineSerializationContext {
                        metadata: timeline.metadata.as_ref(),
                        data: timeline.data.as_ref(),
                        metadata_id: Some(&line_builder.metadata_id),
                        computer_override: None,
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
        csv_serialise_record_with_extra_field(
            &data.data,
            self.record_fields.as_deref(),
            data.data_id.as_deref(),
            CsvExtraField::String(&line_builder.metadata_id),
            &mut self.line_buffer,
            &self.output_date_codec,
        )?;

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

fn build_standard_header(
    header: &mut String,
    normalized: bool,
    record_fields: &[String],
) -> Result<(), Error> {
    for field in record_fields {
        header.push_str(field);
        header.push(CSV_DELIMITER);
    }
    if normalized {
        header.push_str(OUTPUT_METADATA_ID_FIELD);
    } else {
        header.push_str(OUTPUT_METADATA_FIELD);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    pub const TMP_FOLDER: &str = ".tmp";
    use std::{
        fs::{self, File},
        io::Read,
    };

    use flate2::read::MultiGzDecoder;

    use crate::{DateInputCodec, Field, FieldName, Metadata, Parser, Record, TimeLineBuilder};

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

    fn read_gzip(path: &Path) -> String {
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);

        let mut decoder = MultiGzDecoder::new(bytes.as_slice());
        let mut decoded = String::new();
        decoder.read_to_string(&mut decoded).unwrap();
        decoded
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
        let mut ouput = CsvFormatter::new(
            None,
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
            false,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        record.add("who", Value::String("World".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        record.add("greetings", Value::String("Hello".to_owned()));
        record.add("who", Value::Float(4.2));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        drop(ouput);

        let path = Path::new(TMP_FOLDER).join("test_file.csv");
        let message: String = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();
        let expected = "\"Hello\";\"World\";\"{\"\"computer\"\":\"\"test\"\",\"\"data_type\"\":\"\"\"\"}\"\n\"Hello\";4.2;\"{\"\"computer\"\":\"\"test\"\",\"\"data_type\"\":\"\"\"\"}\"\n";
        assert_eq!(expected, message)
    }

    #[test]
    fn standard_csv_filters_only_top_level_unmapped_fields() {
        let output_folder = Path::new(TMP_FOLDER).join("test_standard_csv_schema_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();

        let field_mapping = FieldMapping::new(
            vec![
                Field::Object {
                    name: FieldName::new("details".into(), false, None, None, None),
                    fields: vec![Field::Single {
                        name: FieldName::new("known".into(), false, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    }],
                    ignore: false,
                },
                Field::Single {
                    name: FieldName::new("missing".into(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("schema-host".into()),
            None,
            field_mapping.clone(),
            false,
            false,
            true,
        );
        let mut output = CsvFormatter::new(
            Some(field_mapping),
            OutputType::File,
            output_folder.to_str().unwrap(),
            "schema",
            DateOutputCodec::Iso(),
            false,
            false,
            7,
            FileReport::default(),
            false,
        )
        .unwrap();

        let mut details = Record::new();
        details.add("known", Value::String("kept".to_owned()));
        details.add("unmapped_nested", Value::String("also kept".to_owned()));
        let mut record = Record::new();
        record.add("details", Value::Object(details));
        record.add("unmapped_top_level", Value::String("filtered".to_owned()));

        line_builder.build(&mut record).unwrap();
        output.write(&line_builder).unwrap();
        output.close().unwrap();

        let data = fs::read_to_string(output_folder.join("schema.csv")).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(data.as_bytes());
        let headers = reader.headers().unwrap().clone();
        assert_eq!(
            headers.iter().collect::<Vec<_>>(),
            vec!["details", "missing", "ogre_md"]
        );

        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.len(), headers.len());
        let details: serde_json::Value = serde_json::from_str(&row[0]).unwrap();
        assert_eq!(details["known"], "kept");
        assert_eq!(details["unmapped_nested"], "also kept");
        assert_eq!(&row[1], "");
        let metadata: serde_json::Value = serde_json::from_str(&row[2]).unwrap();
        assert_eq!(metadata["computer"], "schema-host");
        assert!(!row.iter().any(|value| value == "filtered"));

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
        let mut ouput = CsvFormatter::new(
            None,
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
            false,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        record.add("who", Value::String("World".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();
        drop(ouput); //closes the file properly

        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            true,
        );
        let mut ouput = CsvFormatter::new(
            None,
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
            false,
        )
        .unwrap();
        let mut record = Record::new();
        record.add("greetings", Value::String("Hello".to_owned()));
        record.add("who", Value::Float(4.2));
        line_builder.build(&mut record).unwrap();
        ouput.write(&line_builder).unwrap();

        drop(ouput); //closes the file properly

        let path = Path::new(TMP_FOLDER).join("test_gz.csv.gz");

        let mut d = MultiGzDecoder::new(File::open(&path).unwrap());
        let mut buf = String::new();
        d.read_to_string(&mut buf).unwrap();
        drop(d);
        fs::remove_file(path).unwrap();

        let expected = "\"Hello\";\"World\";\"{\"\"computer\"\":\"\"test\"\",\"\"data_type\"\":\"\"\"\"}\"\n\"Hello\";4.2;\"{\"\"computer\"\":\"\"test\"\",\"\"data_type\"\":\"\"\"\"}\"\n";
        assert_eq!(expected, buf)
    }

    #[test]
    fn serialized_metadata_reuses_cached_value() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let path = Path::new(TMP_FOLDER).join("test_metadata_cache.csv");
        remove_if_exists(&path);

        let mut output = CsvFormatter::new(
            None,
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
            false,
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

        csv_serialise_record_with_extra_field(
            &record,
            None,
            None,
            CsvExtraField::Metadata(&metadata),
            &mut buffer,
            &DateOutputCodec::Iso(),
        )
        .unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(buffer.as_bytes()));
        let parsed = reader.records().next().unwrap().unwrap();
        assert_eq!(&parsed[0], "Hello");
        assert_eq!(&parsed[1], "{\"computer\":\"test\",\"data_type\":\"\"}");
        assert!(!record.contains_key(OUTPUT_METADATA_FIELD));
    }

    #[test]
    fn csv_record_serializer_projects_generated_data_id_column() {
        let mut record = Record::new();
        record.add("event", Value::String("login".to_owned()));
        let original = record.clone();
        let record_fields = vec!["event".to_owned(), OUPUT_DATA_ID.to_owned()];
        let mut buffer = String::new();

        csv_serialise_record_with_extra_field(
            &record,
            Some(&record_fields),
            Some("generated-id"),
            CsvExtraField::String("metadata-id"),
            &mut buffer,
            &DateOutputCodec::Iso(),
        )
        .unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(buffer.as_bytes());
        let parsed = reader.records().next().unwrap().unwrap();
        assert_eq!(
            parsed.iter().collect::<Vec<_>>(),
            vec!["login", "generated-id", "metadata-id"]
        );
        assert_eq!(record, original);
    }

    #[test]
    fn timeline_csv_write_does_not_mutate_source_timelines() {
        std::fs::create_dir_all(TMP_FOLDER).unwrap();
        let base_file_name = "test_csv_timeline_no_mutation";
        let path = Path::new(TMP_FOLDER).join(format!("{base_file_name}.csv"));
        remove_if_exists(&path);

        let timeline_builder = TimeLineBuilder::new(
            crate::TimeLineType::Standard,
            "csv_test".to_string(),
            100,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("host-csv-timeline".into()),
            Some(timeline_builder),
            timeline_field_mapping_for_two_dates(),
            false,
            false,
            true,
        );
        let mut output = CsvFormatter::new(
            None,
            OutputType::File,
            TMP_FOLDER,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                ..Default::default()
            },
            false,
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

        let output_text = fs::read_to_string(&path).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(output_text.as_bytes()));
        let rows: Vec<_> = reader.records().collect::<Result<_, _>>().unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(&row[0], "");
            assert_eq!(&row[3], "csv_test");
            assert!(row[7].contains("\"computer\":\"host-csv-timeline\""));
            assert!(row[8].contains("\"message\":\"Hello\""));
        }

        remove_if_exists(&path);
    }

    #[test]
    fn normalized_csv_timeline_uses_formatter_metadata_id() {
        let output_folder = Path::new(TMP_FOLDER).join("test_csv_timeline_md_id_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder_str = output_folder.to_str().unwrap();
        let base_file_name = "test_csv_timeline_md_id";
        let timeline_path = output_folder.join(format!("{base_file_name}.tl.csv"));

        let timeline_builder = TimeLineBuilder::new(
            crate::TimeLineType::Standard,
            "csv_test".to_string(),
            100,
            None,
            None,
        );
        let field_mapping = timeline_field_mapping_for_two_dates();
        let mut line_builder = LineBuilder::new(
            Metadata::new("host-csv-timeline".into()),
            Some(timeline_builder),
            field_mapping.clone(),
            true,
            false,
            true,
        );
        let mut output = CsvFormatter::new(
            Some(field_mapping),
            OutputType::File,
            output_folder_str,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                ..Default::default()
            },
            true,
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

        let timeline = fs::read_to_string(&timeline_path).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(timeline.as_bytes()));
        let first = reader.records().next().unwrap().unwrap();
        assert_eq!(&first[8], &line_builder.metadata_id);

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn normalized_gzip_compresses_data_metadata_and_timeline_headers() {
        let output_folder = Path::new(TMP_FOLDER).join("test_normalized_csv_gzip_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder_str = output_folder.to_str().unwrap();
        let base_file_name = "test_normalized_csv_gzip";
        let field_mapping = timeline_field_mapping_for_two_dates();
        let timeline_builder = TimeLineBuilder::new(
            crate::TimeLineType::Standard,
            "csv_test".to_string(),
            100,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("host-csv-gzip".into()),
            Some(timeline_builder),
            field_mapping.clone(),
            true,
            false,
            true,
        );
        let mut output = CsvFormatter::new(
            Some(field_mapping),
            OutputType::Gzip,
            output_folder_str,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport::default(),
            true,
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

        output.write_metadata(&line_builder).unwrap();
        output.write_normalized(&line_builder).unwrap();
        let reports = output.reports();
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].num_lines, 1);
        assert!(
            reports[0]
                .file_name
                .ends_with("test_normalized_csv_gzip.csv.gz")
        );
        assert_eq!(reports[1].num_lines, 1);
        assert!(reports[1].file_name.ends_with("ogre_metadata.csv.gz"));
        assert_eq!(reports[2].num_lines, 2);
        assert!(
            reports[2]
                .file_name
                .ends_with("test_normalized_csv_gzip.tl.csv.gz")
        );
        output.close().unwrap();

        let data = read_gzip(&output_folder.join(format!("{base_file_name}.csv.gz")));
        let metadata = read_gzip(&output_folder.join("ogre_metadata.csv.gz"));
        let timeline = read_gzip(&output_folder.join(format!("{base_file_name}.tl.csv.gz")));

        assert_eq!(data.lines().count(), 2);
        assert_eq!(metadata.lines().count(), 2);
        assert_eq!(timeline.lines().count(), 3);
        assert!(data.starts_with("when_a;when_b;message;ogre_id;ogre_md_id\n"));
        assert!(metadata.starts_with("computer;data_type;id;"));
        assert!(timeline.starts_with("id;computer;timestamp;"));

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn normalized_output() {
        let output_folder = Path::new(TMP_FOLDER).join("test_normalized_csv_dir");
        remove_dir_if_exists(&output_folder);
        std::fs::create_dir_all(&output_folder).unwrap();
        let output_folder_str = output_folder.to_str().unwrap();
        let timeline_builder = TimeLineBuilder::new(
            crate::TimeLineType::Standard,
            "test_data".to_string(),
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
                Field::Single {
                    name: FieldName::new("who".into(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
            None,
        );

        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            Some(timeline_builder),
            field_mapping.clone(),
            true,
            false,
            true,
        );

        let base_file_name = "test_normalised_csv";

        let mut ouput = CsvFormatter::new(
            Some(field_mapping),
            OutputType::File,
            output_folder_str,
            base_file_name,
            DateOutputCodec::Iso(),
            false,
            true,
            7,
            FileReport {
                ..Default::default()
            },
            true,
        )
        .unwrap();
        let mut record = Record::new();

        //data is inserted in the wrong order
        record.add("who", Value::String("World".to_owned()));
        let timestamp =
            crate::date_util::parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        record.add("when", Value::Date(timestamp));
        record.add("greetings", Value::String("Hello".to_owned()));
        line_builder.build(&mut record).unwrap();
        ouput.write_normalized(&line_builder).unwrap();
        assert_eq!(ouput.file_report.num_lines, 1);
        ouput.close().unwrap();

        let data_path = output_folder.join(format!("{base_file_name}.csv"));
        let data = fs::read_to_string(data_path).unwrap();

        let timeline_path = output_folder.join(format!("{base_file_name}.tl.csv"));
        let timeline = fs::read_to_string(timeline_path).unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(data.as_bytes()));
        let mut line_number = 0;
        let mut ogre_id = "".to_string();
        let mut ogre_md_id = "".to_string();
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();
            assert_eq!(5, rec.len());

            assert_eq!(&rec[0], "2020-01-01T00:00:00.000000+00:00");
            assert_eq!(&rec[1], "Hello");
            assert_eq!(&rec[2], "World");
            ogre_id = rec[3].to_string();
            assert_eq!(&ogre_id, "oGsnmVDx0hRcY8yUjKTLNbQIbNHeci9vLvp8UWfuJeU=");
            ogre_md_id = rec[4].to_string();
            assert_eq!(&ogre_md_id, "7E--LvjAPLv1UvYZXDmzbzruAvf220l54E--U90Tsbs=");
        }
        assert_eq!(1, line_number);

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(timeline.as_bytes()));
        let mut line_number = 0;
        let mut tl_ogre_id = "".to_string();
        let mut tl_ogre_md_id = "".to_string();
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();
            assert_eq!(10, rec.len());
            assert_eq!(&rec[0], "dQLXSFgmnMWoIHZFiNXCH_9eU8Yxvav8pVGhL0NIji4=");
            assert_eq!(&rec[1], "test");
            assert_eq!(&rec[2], "2020-01-01T00:00:00.000000+00:00");
            assert_eq!(&rec[3], "when");
            assert_eq!(&rec[4], "test_data");
            assert_eq!(&rec[5], "");
            assert_eq!(&rec[6], "");
            assert_eq!(&rec[7], "");
            tl_ogre_md_id = rec[8].to_string();
            assert_eq!(&ogre_md_id, "7E--LvjAPLv1UvYZXDmzbzruAvf220l54E--U90Tsbs=");
            tl_ogre_id = rec[9].to_string();
            assert_eq!(&ogre_id, "oGsnmVDx0hRcY8yUjKTLNbQIbNHeci9vLvp8UWfuJeU=");
        }
        assert_eq!(1, line_number);
        assert_eq!(tl_ogre_id, ogre_id);
        assert_eq!(tl_ogre_md_id, ogre_md_id);

        remove_dir_if_exists(&output_folder);
    }
}
