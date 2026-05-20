use std::{fs::OpenOptions, path::Path, sync::Arc};

use crate::{
    DateOutputCodec, Error, FileReport, Metadata, Value,
    line_builder::{LineBuilder, OUTPUT_METADATA_FIELD, OUTPUT_METADATA_ID_FIELD},
    metadata::MetadataSerialized,
    output::OutputType,
    output_writer::{FileWriter, GzipWriter, Writer},
};

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
    pub file_report: FileReport,
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
        normalized: bool,
    ) -> Result<Self, Error> {
        let file_path = match output_type {
            OutputType::File => Path::new(&output_folder).join(format!("{file_name}.jsonl")),
            OutputType::Gzip => Path::new(&output_folder).join(format!("{file_name}.jsonl.gz")),
        };
        let file_name = file_path.display().to_string();
        file_report.file_name = file_name.clone();

        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(file_path)?;

        let data_writer: Box<dyn Writer + Send + Sync> = match output_type {
            OutputType::File => Box::new(FileWriter::new(file)),
            OutputType::Gzip => Box::new(GzipWriter::new(file, compression_level)),
        };

        let (metadata_writer, timeline_writer) = match normalized {
            true => {
                let metadata_file_path = match output_type {
                    OutputType::File => Path::new(&output_folder).join("ogre_metadata.jsonl"),
                    OutputType::Gzip => Path::new(&output_folder).join("ogre_metadata.jsonl.gz"),
                };
                let metadata_file = OpenOptions::new()
                    .read(true)
                    .append(true)
                    .create(true)
                    .open(metadata_file_path)?;

                let metadata_writer: Box<dyn Writer + Send + Sync> = match output_type {
                    OutputType::File => Box::new(FileWriter::new(metadata_file)),
                    OutputType::Gzip => Box::new(GzipWriter::new(metadata_file, compression_level)),
                };
                let timeline_writer = if include_timeline {
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
                        .open(timeline_file_path)?;

                    let timeline_writer: Box<dyn Writer + Send + Sync> = match output_type {
                        OutputType::File => Box::new(FileWriter::new(timeline_file)),
                        OutputType::Gzip => {
                            Box::new(GzipWriter::new(timeline_file, compression_level))
                        }
                    };
                    Some(timeline_writer)
                } else {
                    None
                };

                (Some(metadata_writer), timeline_writer)
            }
            false => (None, None),
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
            file_report,
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
            for mut timeline in data.timeline.clone() {
                timeline.metadata = Some(self.serialized_metadata(&line_builder.metadata));
                timeline.data = Some(data.data.clone());
                self.line_buffer.clear();
                timeline.json_serialise(
                    &mut self.line_buffer,
                    &self.output_date_codec,
                    self.include_empty,
                )?;
                self.line_buffer.push('\n');
                self.data_writer.write(&mut self.line_buffer)?;
                self.file_report.num_lines += 1;
            }
        } else {
            let mut record = data.data.clone();
            record.add(
                OUTPUT_METADATA_FIELD,
                Value::Metadata(self.serialized_metadata(&line_builder.metadata)),
            );
            self.line_buffer.clear();
            Value::json_serialise_record(
                &record,
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
        }
        Ok(())
    }

    pub fn write_normalized(&mut self, line_builder: &LineBuilder) -> Result<(), Error> {
        let data = &line_builder.line_data;
        if self.include_timeline
            && let Some(timeline_writer) = &mut self.timeline_writer
        {
            for mut timeline in data.timeline.clone() {
                timeline.metadata_id = Some(line_builder.metadata_id.clone());
                self.line_buffer.clear();
                timeline.json_serialise(
                    &mut self.line_buffer,
                    &self.output_date_codec,
                    self.include_empty,
                )?;
                self.line_buffer.push('\n');
                timeline_writer.write(&mut self.line_buffer)?;
                self.file_report.num_lines += 1;
            }
        }
        let mut record = data.data.clone();
        record.add(
            OUTPUT_METADATA_ID_FIELD,
            Value::String(line_builder.metadata_id.clone()),
        );

        self.line_buffer.clear();
        Value::json_serialise_record(
            &record,
            &mut self.line_buffer,
            &self.output_date_codec,
            self.include_empty,
        )?;

        self.line_buffer.push('\n');
        self.data_writer.write(&mut self.line_buffer)?;
        if !self.include_timeline {
            self.file_report.num_lines += 1;
        }
        Ok(())
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
        self.data_writer.close()?;
        Ok(())
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

    use crate::{FieldMapping, Metadata, Record};

    use super::*;

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
            false,
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
            false,
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
