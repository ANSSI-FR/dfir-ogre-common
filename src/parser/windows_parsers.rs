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
        let sequence = FieldName::new(
            format!("{prefix}sequence_number"),
            false,
            None,
            None,
            None,
        );
        let record = FieldName::new(
            format!("{prefix}record_number"),
            false,
            None,
            None,
            None,
        );
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

pub struct FileAttributesParser {
    default_field: FieldName,
    file_attributes: IndexMap<char, FieldName>,
    data: IndexMap<String, bool>,
    field_names: Vec<FieldName>,
}
impl FileAttributesParser {
    fn new() -> Self {
        let default_field = FieldName::new(
            "file_attribute_raw".to_string(),
            false,
            None,
            None,
            None,
        );
        let mut file_attributes = IndexMap::new();
        file_attributes.insert(
            'A',
            FieldName::new(
                "file_attributes_archive".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'B',
            FieldName::new(
                "file_attributes_no_scrub_data".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'C',
            FieldName::new(
                "file_attributes_compressed".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'D',
            FieldName::new(
                "file_attributes_directory".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'E',
            FieldName::new(
                "file_attributes_encrypted".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'H',
            FieldName::new(
                "file_attributes_hidden".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'I',
            FieldName::new(
                "file_attributes_not_content_indexed".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'L',
            FieldName::new(
                "file_attributes_reparse_point".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'N',
            FieldName::new(
                "file_attributes_normal".to_string(),
                false,
                None,
                None,
                None,
            ),
        );

        file_attributes.insert(
            'O',
            FieldName::new(
                "file_attributes_offline".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'P',
            FieldName::new(
                "file_attributes_sparse_file".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'R',
            FieldName::new(
                "file_attributes_readonly".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'S',
            FieldName::new(
                "file_attributes_system".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'T',
            FieldName::new(
                "file_attributes_temporary".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'V',
            FieldName::new(
                "file_attributes_virtual".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'a',
            FieldName::new(
                "file_attributes_recall_on_data_access".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'd',
            FieldName::new(
                "file_attributes_device".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'e',
            FieldName::new(
                "file_attributes_ea".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'o',
            FieldName::new(
                "file_attributes_recall_on_open".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'p',
            FieldName::new(
                "file_attributes_pinned".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            's',
            FieldName::new(
                "file_attributes_integrity_stream".to_string(),
                false,
                None,
                None,
                None,
            ),
        );
        file_attributes.insert(
            'u',
            FieldName::new(
                "file_attributes_unpinned".to_string(),
                false,
                None,
                None,
                None,
            ),
        );

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

    fn parse(&self, input: &str, output_name: &str) -> Option<Vec<(String, Value)>> {
        if input.is_empty() {
            return None;
        }
        let mut record = Vec::with_capacity(self.file_attributes.len());
        if input.starts_with("0x") || input.starts_with("-0x") {
            record.push((
                self.default_field.output_name().to_string(),
                Value::String(output_name.to_string()),
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
        let md5 = FieldName::new(
            "file_pe_md5".to_string(),
            false,
            None,
            None,
            None,
        );
        let sha1 = FieldName::new(
            "file_pe_sha1".to_string(),
            false,
            None,
            None,
            None,
        );
        let sha256 = FieldName::new(
            "file_pe_sha256".to_string(),
            false,
            None,
            None,
            None,
        );
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
