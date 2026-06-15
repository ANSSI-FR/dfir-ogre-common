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
    value::escape_special_chars_csv,
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
        buffer.push_str(&self.computer);
        buffer.push(CSV_DELIMITER);
        buffer.push_str(&self.data_type);
        Self::serialise_csv_string(&self.id, buffer);
        Self::serialise_csv_string(&self.orc_id, buffer);
        Self::serialise_csv_string(&self.folder, buffer);
        Self::serialise_csv_string(&self.archive, buffer);
        Self::serialise_csv_string(&self.subarchive, buffer);
        Self::serialise_csv_string(&self.archive_filename, buffer);
        Self::serialise_csv_string(&self.original_filename, buffer);
        Self::serialise_csv_string(&self.vss, buffer);
        Self::serialise_csv_date(&self.orc_start_date, date_codec, buffer);
        Self::serialise_csv_date(&self.creation_date, date_codec, buffer);
        Self::serialise_csv_date(&self.modif_date, date_codec, buffer);
    }

    fn serialise_csv_string(value: &Option<String>, buffer: &mut String) {
        buffer.push(CSV_DELIMITER);
        if let Some(value) = value {
            buffer.push('\"');
            buffer.push_str(&escape_special_chars_csv(value));
            buffer.push('\"');
        }
    }

    fn serialise_csv_date(
        date: &Option<DateTime<Utc>>,
        date_codec: &DateOutputCodec,
        buffer: &mut String,
    ) {
        buffer.push(CSV_DELIMITER);
        if let Some(date) = date {
            buffer.push('\"');
            buffer.push_str(&serialize_date(date, date_codec));
            buffer.push('\"');
        }
    }

    pub fn json_serialise(&self, buffer: &mut String, date_codec: &DateOutputCodec) {
        buffer.push('{');
        buffer.push_str("\"computer\":\"");
        buffer.push_str(&self.computer);
        buffer.push('\"');
        buffer.push_str(",\"data_type\":\"");
        buffer.push_str(&self.data_type);
        buffer.push('\"');

        Self::serialise_json_string("id", &self.id, buffer);
        Self::serialise_json_string("orc_id", &self.orc_id, buffer);
        Self::serialise_json_string("folder", &self.folder, buffer);
        Self::serialise_json_string("archive", &self.archive, buffer);
        Self::serialise_json_string("subarchive", &self.subarchive, buffer);
        Self::serialise_json_string("archive_filename", &self.archive_filename, buffer);
        Self::serialise_json_string("original_filename", &self.original_filename, buffer);
        Self::serialise_json_string("vss", &self.vss, buffer);
        Self::serialise_json_date("orc_start_date", &self.orc_start_date, date_codec, buffer);
        Self::serialise_json_date("creation_date", &self.creation_date, date_codec, buffer);
        Self::serialise_json_date("modif_date", &self.modif_date, date_codec, buffer);
        buffer.push('}');
    }

    fn serialise_json_string(name: &str, value: &Option<String>, buffer: &mut String) {
        if let Some(value) = value {
            buffer.push_str(",\"");
            buffer.push_str(name);
            buffer.push_str("\":\"");
            buffer.push_str(&escape_special_chars(value));
            buffer.push('\"');
        }
    }

    fn serialise_json_date(
        name: &str,
        date: &Option<DateTime<Utc>>,
        date_codec: &DateOutputCodec,
        buffer: &mut String,
    ) {
        if let Some(date) = date {
            buffer.push_str(",\"");
            buffer.push_str(name);
            buffer.push_str("\":\"");
            buffer.push_str(&serialize_date(date, date_codec));
            buffer.push('\"');
        }
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
        meta2.orc_start_date = meta1.orc_start_date.clone();
        meta2.creation_date = meta1.creation_date.clone();
        meta2.modif_date = meta1.modif_date.clone();

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
        meta.id = Some(BASE64.encode(&hash1.as_slice()));

        let mut csv = String::new();

        Metadata::csv_serialise_header(&mut csv);
        csv.push_str("\n");
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
}
