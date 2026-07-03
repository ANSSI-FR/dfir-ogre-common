//! This module defines the [`Metadata`] struct, which stores optional forensic
//! information about a file or artifact (e.g. computer name, archive location,
//! timestamps).  It provides utilities to hash the content, convert the data to
//! a generic [`Record`] for output, and write a CSV header line.
//
//! The struct is exposed to Python via PyO3 with automatic getters/setters.

use std::sync::Arc;

use blake3::Hasher;
use chrono::{DateTime, Utc};
use pyo3::prelude::*;

use crate::{
    DateOutputCodec, escape_special_chars, format_csv::CSV_DELIMITER, serialize_date,
    serialise::{CsvRowWriter, JsonObjectWriter},
};

#[derive(Clone, Debug, Default, PartialEq)]
#[pyclass(from_py_object)]
pub struct MetadataSerialized(pub Arc<String>);

/// metadata associated with a file or artifact.
///
/// Fields are optional except for `computer`, which identifies the host system.
#[derive(Clone, Debug, Default, PartialEq)]
#[pyclass(get_all, set_all, from_py_object)]
pub struct Metadata {
    /// Host computer identifier.
    pub computer: String,
    pub data_type: String,
    pub id: Option<String>,
    pub orc_id: Option<String>,
    pub folder: Option<String>,
    pub archive: Option<String>,
    pub subarchive: Option<String>,
    pub archive_filename: Option<String>,
    pub original_filename: Option<String>,
    pub vss: Option<String>,
    pub orc_start_date: Option<DateTime<Utc>>,
    pub creation_date: Option<DateTime<Utc>>,
    pub modif_date: Option<DateTime<Utc>>,
}

#[pymethods]
impl Metadata {
    /// Constructs a new [`Metadata`] with the given `computer` name.
    ///
    /// All other fields are initialised to `None`.
    #[new]
    pub fn new(computer: String) -> Self {
        Self {
            computer,
            ..Default::default()
        }
    }
}

impl Metadata {
    /// Hashes all metadata fields (except id) into the supplied `hasher` to create a unique identifier.
    pub fn compute_id(&self, hasher: &mut Hasher) {
        hasher.update(self.computer.as_bytes());
        hasher.update(self.data_type.as_bytes());
        Metadata::update_hash_string(&self.orc_id, hasher);
        Metadata::update_hash_string(&self.folder, hasher);
        Metadata::update_hash_string(&self.archive, hasher);
        Metadata::update_hash_string(&self.subarchive, hasher);
        Metadata::update_hash_string(&self.archive_filename, hasher);
        Metadata::update_hash_string(&self.original_filename, hasher);
        Metadata::update_hash_string(&self.vss, hasher);
        Metadata::update_hash_date(&self.orc_start_date, hasher);
        Metadata::update_hash_date(&self.creation_date, hasher);
        Metadata::update_hash_date(&self.modif_date, hasher);
    }

    /// Helper for hashing optional string fields.
    fn update_hash_string(value: &Option<String>, hasher: &mut Hasher) {
        hasher.update(value.as_ref().unwrap_or(&"null".to_owned()).as_bytes());
    }

    /// Helper for hashing optional `DateTime<Utc>` fields.
    fn update_hash_date(value: &Option<DateTime<Utc>>, hasher: &mut Hasher) {
        if let Some(date_time) = value
            && let Some(nanos) = date_time.timestamp_nanos_opt()
        {
            hasher.update(&nanos.to_ne_bytes());
        } else {
            // Hash a zero value if the timestamp is missing or out‑of‑range.
            hasher.update(&0i64.to_ne_bytes());
        }
    }

    /// Writes the CSV header line for all metadata fields into `buffer`.
    ///
    /// Fields are joined using the library‑wide `CSV_DELIMITER`.  The order
    /// matches the order used by `to_record`.
    pub fn csv_serialise_header(buffer: &mut String) {
        buffer.push_str("computer");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("data_type");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("id");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("orc_id");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("folder");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("archive");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("subarchive");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("archive_filename");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("original_filename");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("vss");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("orc_start_date");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("creation_date");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("modif_date");
    }

    pub fn csv_serialise(&self, buffer: &mut String, date_codec: &DateOutputCodec) {
        let mut row = CsvRowWriter::new(buffer);
        row.quoted_field(&self.computer);
        row.quoted_field(&self.data_type);
        row.optional_quoted_field(self.id.as_deref());
        row.optional_quoted_field(self.orc_id.as_deref());
        row.optional_quoted_field(self.folder.as_deref());
        row.optional_quoted_field(self.archive.as_deref());
        row.optional_quoted_field(self.subarchive.as_deref());
        row.optional_quoted_field(self.archive_filename.as_deref());
        row.optional_quoted_field(self.original_filename.as_deref());
        row.optional_quoted_field(self.vss.as_deref());
        Self::serialise_csv_date(&mut row, &self.orc_start_date, date_codec);
        Self::serialise_csv_date(&mut row, &self.creation_date, date_codec);
        Self::serialise_csv_date(&mut row, &self.modif_date, date_codec);
    }

    fn serialise_csv_date(
        row: &mut CsvRowWriter<'_>,
        date: &Option<DateTime<Utc>>,
        date_codec: &DateOutputCodec,
    ) {
        if let Some(date) = date {
            row.quoted_field(&serialize_date(date, date_codec));
        } else {
            row.empty_field();
        }
    }

    pub fn json_serialise(&self, buffer: &mut String, date_codec: &DateOutputCodec) {
        let mut object = JsonObjectWriter::new(buffer);
        object.field_raw("computer", |buffer| {
            buffer.push('\"');
            buffer.push_str(&escape_special_chars(&self.computer));
            buffer.push('\"');
        });
        object.field_raw("data_type", |buffer| {
            buffer.push('\"');
            buffer.push_str(&escape_special_chars(&self.data_type));
            buffer.push('\"');
        });
        Self::serialise_json_string(&mut object, "id", &self.id);
        Self::serialise_json_string(&mut object, "orc_id", &self.orc_id);
        Self::serialise_json_string(&mut object, "folder", &self.folder);
        Self::serialise_json_string(&mut object, "archive", &self.archive);
        Self::serialise_json_string(&mut object, "subarchive", &self.subarchive);
        Self::serialise_json_string(&mut object, "archive_filename", &self.archive_filename);
        Self::serialise_json_string(&mut object, "original_filename", &self.original_filename);
        Self::serialise_json_string(&mut object, "vss", &self.vss);
        Self::serialise_json_date(&mut object, "orc_start_date", &self.orc_start_date, date_codec);
        Self::serialise_json_date(&mut object, "creation_date", &self.creation_date, date_codec);
        Self::serialise_json_date(&mut object, "modif_date", &self.modif_date, date_codec);
        object.finish();
    }

    fn serialise_json_string(object: &mut JsonObjectWriter<'_>, name: &str, value: &Option<String>) {
        object.field_if(name, value.is_some(), |buffer| {
            buffer.push('\"');
            buffer.push_str(&escape_special_chars(value.as_deref().unwrap_or_default()));
            buffer.push('\"');
        });
    }

    fn serialise_json_date(
        object: &mut JsonObjectWriter<'_>,
        name: &str,
        date: &Option<DateTime<Utc>>,
        date_codec: &DateOutputCodec,
    ) {
        object.field_if(name, date.is_some(), |buffer| {
            buffer.push('\"');
            buffer.push_str(&serialize_date(date.as_ref().unwrap(), date_codec));
            buffer.push('\"');
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DateOutputCodec,
        date_util::{DateInputCodec, parse_date},
    };
    use base64::{Engine as _, engine::general_purpose::URL_SAFE as BASE64};
    use chrono::Utc;
    #[test]
    fn compute_id_hash() {
        // Prepare two identical Metadata instances
        let mut meta1 = Metadata::new("host1".into());
        meta1.id = Some("id123".into());
        meta1.data_type = "evtx".into();
        meta1.orc_id = Some("orc456".into());
        meta1.folder = Some("C:/folder".into());
        meta1.archive = Some("archive.zip".into());
        meta1.subarchive = Some("sub.zip".into());
        meta1.archive_filename = Some("file.txt".into());
        meta1.original_filename = Some("orig.txt".into());
        meta1.vss = Some("vss001".into());
        meta1.orc_start_date = Some(Utc::now());
        meta1.creation_date = Some(Utc::now());
        meta1.modif_date = Some(Utc::now());

        let mut meta2 = Metadata::new("host1".into());
        meta2.id = meta1.id.clone();
        meta2.data_type = meta1.data_type.clone();
        meta2.orc_id = meta1.orc_id.clone();
        meta2.folder = meta1.folder.clone();
        meta2.archive = meta1.archive.clone();
        meta2.subarchive = meta1.subarchive.clone();
        meta2.archive_filename = meta1.archive_filename.clone();
        meta2.original_filename = meta1.original_filename.clone();
        meta2.vss = meta1.vss.clone();
        meta2.orc_start_date = meta1.orc_start_date;
        meta2.creation_date = meta1.creation_date;
        meta2.modif_date = meta1.modif_date;

        // Compute hashes
        let mut hasher1 = blake3::Hasher::new();
        meta1.compute_id(&mut hasher1);
        let hash1 = hasher1.finalize();

        let mut hasher2 = blake3::Hasher::new();
        meta2.compute_id(&mut hasher2);
        let hash2 = hasher2.finalize();

        assert_eq!(hash1, hash2, "Hashes of identical metadata should be equal");
        meta2.data_type = "changed".to_string();

        let mut hasher3 = blake3::Hasher::new();
        meta2.compute_id(&mut hasher3);
        let hash3 = hasher3.finalize();
        assert_ne!(hash2, hash3)
    }

    #[test]
    fn write_csv_header_outputs_correct_fields_in_order() {
        let mut buffer = String::new();
        Metadata::csv_serialise_header(&mut buffer);

        let expected = [
            "computer",
            "data_type",
            "id",
            "orc_id",
            "folder",
            "archive",
            "subarchive",
            "archive_filename",
            "original_filename",
            "vss",
            "orc_start_date",
            "creation_date",
            "modif_date",
        ]
        .join(&crate::format_csv::CSV_DELIMITER.to_string());

        assert_eq!(buffer, expected);
    }

    // Test the csv_serialise method for correct ordering and quoting of fields.
    #[test]
    fn csv_serialise_outputs_correct_fields_and_order() {
        // Use a deterministic UTC date for reproducible output.
        let creation_date = parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        let modif_date = parse_date("2021-02-02T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        let orc_start_date = parse_date("2023-03-03T00:00:00Z", &DateInputCodec::Iso()).unwrap();

        let mut meta = Metadata::new("host1".to_owned());
        meta.data_type = "evtx".into();
        meta.orc_id = None;
        meta.folder = Some("folder".to_owned());
        meta.archive = None;
        meta.subarchive = Some("subar\"chive".to_owned());
        meta.archive_filename = None;
        meta.original_filename = Some("orig.txt".to_owned());
        meta.vss = None;
        meta.orc_start_date = Some(orc_start_date);
        meta.creation_date = Some(creation_date);
        meta.modif_date = Some(modif_date);

        let mut hasher1 = blake3::Hasher::new();
        meta.compute_id(&mut hasher1);
        let hash1 = hasher1.finalize();
        meta.id = Some(BASE64.encode(hash1.as_slice()));

        let mut csv = String::new();

        Metadata::csv_serialise_header(&mut csv);
        csv.push('\n');
        meta.csv_serialise(&mut csv, &DateOutputCodec::IsoUtc());

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .flexible(false)
            .from_reader(Box::new(csv.as_bytes()));
        let mut line_number = 0;
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();
            assert_eq!(13, rec.len());

            assert_eq!(&rec[0], "host1");
            assert_eq!(&rec[1], "evtx");
            assert_eq!(&rec[2], "BSMNxNN828nzRZdQ8YnttEJAAQfy37suyDCbQvzWm8U=");
            assert_eq!(&rec[3], "");
            assert_eq!(&rec[4], "folder");
            assert_eq!(&rec[5], "");
            assert_eq!(&rec[6], "subar\"chive");
            assert_eq!(&rec[7], "");
            assert_eq!(&rec[8], "orig.txt");
            assert_eq!(&rec[9], "");
            assert_eq!(&rec[10], "2023-03-03T00:00:00.000000+00:00");
            assert_eq!(&rec[11], "2020-01-01T00:00:00.000000+00:00");
            assert_eq!(&rec[12], "2021-02-02T00:00:00.000000+00:00");
        }
        assert_eq!(1, line_number);
    }

    #[test]
    fn json_serialise_outputs_optional_strings_dates_and_escapes_values() {
        let creation_date = parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        let mut metadata = Metadata::new("host1".to_string());
        metadata.data_type = "evtx".to_string();
        metadata.folder = Some("folder \"quoted\"".to_string());
        metadata.creation_date = Some(creation_date);

        let mut buffer = String::new();
        metadata.json_serialise(&mut buffer, &DateOutputCodec::Iso());

        let expected = r#"{"computer":"host1","data_type":"evtx","folder":"folder \"quoted\"","creation_date":"2020-01-01T00:00:00.000000+00:00"}"#;
        assert_eq!(buffer, expected);
    }

    #[test]
    fn json_serialise_escapes_computer_and_data_type() {
        let mut metadata = Metadata::new("host\"one\\two\n\t\u{0001}".to_owned());
        metadata.data_type = "evtx\"type\\kind\n\t\u{000B}".to_owned();

        let mut buffer = String::new();
        metadata.json_serialise(&mut buffer, &DateOutputCodec::Iso());

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed["computer"], "host\"one\\two\n\t\u{0001}");
        assert_eq!(parsed["data_type"], "evtx\"type\\kind\n\t\u{000B}");
    }

    #[test]
    fn csv_serialise_escapes_computer_and_data_type() {
        let mut metadata = Metadata::new("host;\"one\"\nnext".to_owned());
        metadata.data_type = "evtx;\"type\"\nnext".to_owned();

        let mut csv = String::new();
        Metadata::csv_serialise_header(&mut csv);
        csv.push('\n');
        metadata.csv_serialise(&mut csv, &DateOutputCodec::Iso());

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(csv.as_bytes()));
        let record = reader.records().next().unwrap().unwrap();

        assert_eq!(&record[0], "host;\"one\"\nnext");
        assert_eq!(&record[1], "evtx;\"type\"\nnext");
        assert_eq!(13, record.len());
    }
}
