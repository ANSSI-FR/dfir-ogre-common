use std::sync::Arc;

use indexmap::IndexMap;
use log::error;
use pyo3::pyfunction;

use crate::{
    FieldName, Value,
    field::{ParserExtension, ParserExtensionTrait},
};

/// Windows specific field parser
/// Dispatch the content of the frn hex field on the 'sequence' and 'record' fields.
#[pyfunction]
pub fn win_frn_hex_parser(prefix: &str) -> ParserExtension {
    ParserExtension(Arc::new(FRNHexParser::new(prefix)))
}

/// Windows specific field parser
/// Dispatch the content of the frn hex field on the 'sequence' and 'record' fields.
#[pyfunction]
pub fn win_frn_int_parser(prefix: &str) -> ParserExtension {
    ParserExtension(Arc::new(FRNIntParser::new(prefix)))
}

pub struct FRNHexParser {
    sequence: FieldName,
    record: FieldName,
}
impl FRNHexParser {
    fn new(prefix: &str) -> Self {
        let sequence = FieldName::new(format!("{prefix}sequence_number"), false, None, None, None);
        let record = FieldName::new(format!("{prefix}record_number"), false, None, None, None);
        Self { sequence, record }
    }
}

impl ParserExtensionTrait for FRNHexParser {
    fn name(&self) -> String {
        "FRNHexParser".to_string()
    }
    fn parse(&self, input: &str, _: &str) -> Option<Vec<(String, Value)>> {
        if input.len() <= 7 {
            return None;
        }

        let seq = i64::from_str_radix(&input[2..6], 16).unwrap_or(0);
        let rec = i64::from_str_radix(&input[6..], 16).unwrap_or(0);

        let record = vec![
            (self.sequence.output_name().to_string(), Value::Int(seq)),
            (self.record.output_name().to_string(), Value::Int(rec)),
        ];
        Some(record)
    }

    fn output_fields_names(&self) -> Vec<FieldName> {
        vec![self.sequence.clone(), self.record.clone()]
    }
}

pub struct FRNIntParser {
    parser: FRNHexParser,
}
impl FRNIntParser {
    fn new(prefix: &str) -> Self {
        Self {
            parser: FRNHexParser::new(prefix),
        }
    }
}

impl ParserExtensionTrait for FRNIntParser {
    fn name(&self) -> String {
        "FRNIntParser".to_string()
    }

    fn parse(&self, input: &str, output_name: &str) -> Option<Vec<(String, Value)>> {
        if input.is_empty() {
            return None;
        }

        let value: i64 = match input.parse() {
            Ok(val) => val,
            Err(e) => {
                error!("Error while parsing integer FRN '{input}'. Error: {e}");
                return None;
            }
        };
        let width = 16;
        let hex_str = format!("0x{value:0width$X}");

        self.parser.parse(&hex_str, output_name)
    }

    fn output_fields_names(&self) -> Vec<FieldName> {
        self.parser.output_fields_names()
    }
}

/// Windows specific field parser
/// Transform the NTFSInfo Attributes into a list of boolean fields"
#[pyfunction]
pub fn win_ntfs_flag_parser() -> ParserExtension {
    ParserExtension(Arc::new(FileAttributesParser::new()))
}

/// ORC letters and the corresponding Windows FILE_ATTRIBUTE bits. Names are
/// shared by NTFSInfo and the raw MFT parser. EA and RECALL_ON_OPEN alias the
/// same on-disk bit; a raw MFT cannot distinguish their intended meaning.
pub(crate) const NTFS_FILE_ATTRIBUTES: [(char, u32, &str); 22] = [
    ('A', 0x00000020, "file_attributes_archive"),
    ('B', 0x00020000, "file_attributes_no_scrub_data"),
    ('C', 0x00000800, "file_attributes_compressed"),
    ('D', 0x00000010, "file_attributes_directory"),
    ('E', 0x00004000, "file_attributes_encrypted"),
    ('H', 0x00000002, "file_attributes_hidden"),
    ('I', 0x00002000, "file_attributes_not_content_indexed"),
    ('L', 0x00000400, "file_attributes_reparse_point"),
    ('N', 0x00000080, "file_attributes_normal"),
    ('O', 0x00001000, "file_attributes_offline"),
    ('P', 0x00000200, "file_attributes_sparse_file"),
    ('R', 0x00000001, "file_attributes_readonly"),
    ('S', 0x00000004, "file_attributes_system"),
    ('T', 0x00000100, "file_attributes_temporary"),
    ('V', 0x00010000, "file_attributes_virtual"),
    ('a', 0x00400000, "file_attributes_recall_on_data_access"),
    ('d', 0x00000040, "file_attributes_device"),
    ('e', 0x00040000, "file_attributes_ea"),
    ('o', 0x00040000, "file_attributes_recall_on_open"),
    ('p', 0x00080000, "file_attributes_pinned"),
    ('s', 0x00008000, "file_attributes_integrity_stream"),
    ('u', 0x00100000, "file_attributes_unpinned"),
];

pub(crate) fn ntfs_attribute_values(flags: u32) -> impl Iterator<Item = (String, Value)> {
    NTFS_FILE_ATTRIBUTES
        .iter()
        .map(move |(_, bit, name)| ((*name).to_string(), Value::Bool(flags & bit != 0)))
}

pub struct FileAttributesParser {
    default_field: FieldName,
    file_attributes: IndexMap<char, FieldName>,
    data: IndexMap<String, bool>,
    field_names: Vec<FieldName>,
}
impl FileAttributesParser {
    fn new() -> Self {
        let default_field =
            FieldName::new("file_attribute_raw".to_string(), false, None, None, None);
        let file_attributes: IndexMap<char, FieldName> = NTFS_FILE_ATTRIBUTES
            .iter()
            .map(|(letter, _, name)| {
                (
                    *letter,
                    FieldName::new((*name).to_string(), false, None, None, None),
                )
            })
            .collect();

        let mut data = IndexMap::with_capacity(file_attributes.len());
        let mut field_names = Vec::with_capacity(file_attributes.len() + 1);
        for field_name in file_attributes.values() {
            data.insert(field_name.output_name().to_string(), false);
            field_names.push(field_name.to_owned());
        }
        field_names.push(default_field.to_owned());

        Self {
            default_field,
            file_attributes,
            data,
            field_names,
        }
    }
}

impl ParserExtensionTrait for FileAttributesParser {
    fn name(&self) -> String {
        "FileAttributesParser".to_string()
    }

    fn parse(&self, input: &str, _: &str) -> Option<Vec<(String, Value)>> {
        if input.is_empty() {
            return None;
        }
        let mut record = Vec::with_capacity(self.file_attributes.len());
        if input.starts_with("0x") || input.starts_with("-0x") {
            record.push((
                self.default_field.output_name().to_string(),
                Value::String(input.to_owned()),
            ));
        } else {
            let mut data = self.data.clone();
            for flag in input.chars() {
                if flag != '.'
                    && let Some(field) = self.file_attributes.get(&flag)
                    && let Some(val) = data.get_mut(field.input_name())
                {
                    *val = true;
                }
            }
            for (key, value) in data {
                record.push((key, Value::Bool(value)));
            }
        }

        Some(record)
    }

    fn output_fields_names(&self) -> Vec<FieldName> {
        self.field_names.clone()
    }
}

/// Windows specific field parser
/// Cast the value of SignedHash field into the right hash field
#[pyfunction]
pub fn win_signed_hash_parser() -> ParserExtension {
    ParserExtension(Arc::new(SignedHashParser::new()))
}
pub struct SignedHashParser {
    md5: FieldName,
    sha1: FieldName,
    sha256: FieldName,
}
impl SignedHashParser {
    fn new() -> Self {
        let md5 = FieldName::new("file_pe_md5".to_string(), false, None, None, None);
        let sha1 = FieldName::new("file_pe_sha1".to_string(), false, None, None, None);
        let sha256 = FieldName::new("file_pe_sha256".to_string(), false, None, None, None);
        Self { md5, sha1, sha256 }
    }
}

impl ParserExtensionTrait for SignedHashParser {
    fn name(&self) -> String {
        "SignedHashParser".to_string()
    }

    fn parse(&self, input: &str, _: &str) -> Option<Vec<(String, Value)>> {
        match input.len() {
            32 => {
                let record = vec![(
                    self.md5.output_name().to_string(),
                    Value::String(input.to_string()),
                )];
                Some(record)
            }
            40 => {
                let record = vec![(
                    self.sha1.output_name().to_string(),
                    Value::String(input.to_string()),
                )];
                Some(record)
            }
            64 => {
                let record = vec![(
                    self.sha256.output_name().to_string(),
                    Value::String(input.to_string()),
                )];
                Some(record)
            }
            _ => None,
        }
    }

    fn output_fields_names(&self) -> Vec<FieldName> {
        vec![self.md5.clone(), self.sha1.clone(), self.sha256.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_attributes_parser_preserves_raw_hex_values() {
        let parser = FileAttributesParser::new();

        for input in ["0x00000020", "-0x00000020"] {
            let record = parser.parse(input, "mapped_attributes").unwrap();

            assert_eq!(
                record,
                vec![(
                    "file_attribute_raw".to_owned(),
                    Value::String(input.to_owned())
                )]
            );
        }
    }
    #[test]
    fn raw_ntfs_attributes_match_the_existing_orc_letter_fields() {
        let parser = FileAttributesParser::new();
        let raw: Vec<_> = ntfs_attribute_values(0x0000_0022).collect();
        assert_eq!(raw, parser.parse("AH", "Attributes").unwrap());
        let raw: Vec<_> = ntfs_attribute_values(0x0004_0000).collect();
        for name in ["file_attributes_ea", "file_attributes_recall_on_open"] {
            assert!(raw.contains(&(name.to_string(), Value::Bool(true))));
        }
    }
}
