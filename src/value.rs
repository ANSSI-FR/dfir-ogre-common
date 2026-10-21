use blake3::Hasher;
use chrono::{DateTime, Utc};

use crate::{
    DateOutputCodec, Error, Record, hash_util,
    metadata::MetadataSerialized,
    serialise::{JsonArrayWriter, JsonObjectWriter},
    serialize_date,
};
use pyo3::prelude::*;
/// Possible data types for a record. Mirrors JSON types and adds Rust primitives like `i64`
/// and a UTC `DateTime`.
#[derive(Clone, Debug, PartialEq)]
#[pyclass(from_py_object)]
pub enum Value {
    Null(),
    String(String),
    Array(Vec<Value>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Date(DateTime<Utc>),
    Object(Record),

    Metadata(MetadataSerialized),
}

impl Value {
    fn should_emit_json_value(value: &Value, include_empty: bool) -> bool {
        include_empty || !matches!(value, Value::Null())
    }

    /// Returns `true` if the variant is `Value::Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null())
    }

    /// Serializes a `Record` to JSON, appending to `buffer`. `include_empty` decides if `null` fields appear.
    ///
    /// # Arguments
    /// * `record` – The `Record` to serialize.
    /// * `buffer` – Destination string for the JSON output.
    /// * `date_codec` – Codec for formatting dates.
    /// * `include_empty` – When `true`, `null` fields are emitted.
    pub fn json_serialise_record(
        record: &Record,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
        include_empty: bool,
    ) -> Result<(), Error> {
        let start_len = buffer.len();
        let result = (|| {
            let mut object = JsonObjectWriter::new(buffer);
            for (key, el) in record.iter() {
                if Self::should_emit_json_value(el, include_empty) {
                    object.try_field_raw(key, |buffer| {
                        Value::json_serialise_value(el, buffer, date_codec, include_empty)
                    })?;
                }
            }
            object.finish();
            Ok(())
        })();
        if result.is_err() {
            buffer.truncate(start_len);
        }
        result
    }

    /// Serializes a `Record` to CSV, appending to `buffer`.
    ///
    /// # Arguments
    /// * `record` – The `Record` to serialize.
    /// * `buffer` – Destination string for the JSON output.
    /// * `date_codec` – Codec for formatting dates.
    /// * delimiter – the csv delimiter.
    pub fn csv_serialise_record(
        record: &Record,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
        delimiter: char,
    ) -> Result<(), Error> {
        let start_len = buffer.len();
        let result = (|| {
            let mut first = true;
            for (_, val) in record.iter() {
                if !first {
                    buffer.push(delimiter);
                }
                Value::csv_serialise_value(val, buffer, date_codec)?;
                first = false;
            }
            Ok(())
        })();
        if result.is_err() {
            buffer.truncate(start_len);
        }
        result
    }

    /// Serializes a single `Value` to JSON, handling escaping, number formatting, and date conversion.
    ///
    /// # Arguments
    /// * `value` – The `Value` to serialize.
    /// * `buffer` – Destination string for the JSON fragment.
    /// * `date_codec` – Codec for date formatting.
    /// * `include_empty` – When `true`, `null` values are emitted.
    pub fn json_serialise_value(
        value: &Value,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
        include_empty: bool,
    ) -> Result<(), Error> {
        let start_len = buffer.len();
        let result = (|| {
            match value {
                Value::Null() => {
                    if include_empty {
                        buffer.push_str("null")
                    }
                }
                Value::String(v) => {
                    let escaped = escape_special_chars(v);
                    buffer.push('\"');
                    buffer.push_str(&escaped);
                    buffer.push('\"');
                }
                Value::Array(field_values) => {
                    let mut array = JsonArrayWriter::new(buffer);
                    for el in field_values {
                        if Self::should_emit_json_value(el, include_empty) {
                            array.try_element_raw(|buffer| {
                                Value::json_serialise_value(el, buffer, date_codec, include_empty)
                            })?;
                        }
                    }
                    array.finish();
                }
                Value::Int(v) => {
                    buffer.push_str(&v.to_string());
                }
                Value::Float(v) => {
                    if v.is_finite() {
                        buffer.push_str(&v.to_string());
                    } else {
                        buffer.push_str("null");
                    }
                }
                Value::Bool(v) => buffer.push_str(&v.to_string()),
                Value::Date(date_time) => {
                    let v = serialize_date(date_time, date_codec);
                    buffer.push('\"');
                    buffer.push_str(&escape_special_chars(&v));
                    buffer.push('\"');
                }

                Value::Object(hash_map) => {
                    Value::json_serialise_record(hash_map, buffer, date_codec, include_empty)?;
                }
                Value::Metadata(metadata) => {
                    buffer.push_str(&metadata.0);
                }
            }
            Ok(())
        })();
        if result.is_err() {
            buffer.truncate(start_len);
        }
        result
    }

    /// Serializes a single `Value` to CSV, handling escaping, number formatting, and date conversion.
    ///
    /// # Arguments
    /// * `value` – The `Value` to serialize.
    /// * `buffer` – Destination string for the JSON fragment.
    /// * `date_codec` – Codec for date formatting.
    pub fn csv_serialise_value(
        value: &Value,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
    ) -> Result<(), Error> {
        let start_len = buffer.len();
        let result = (|| {
            match value {
                Value::Null() => {}
                Value::String(v) => {
                    let escaped = escape_special_chars_csv(v);
                    buffer.push('\"');
                    buffer.push_str(&escaped);
                    buffer.push('\"');
                }
                Value::Array(field_values) => {
                    buffer.push('"');
                    buffer.push('[');
                    let mut object_buf = String::new();
                    for (i, el) in field_values.iter().enumerate() {
                        if i != 0 {
                            buffer.push(',');
                        }
                        Value::json_serialise_value(el, &mut object_buf, date_codec, true)?;
                        buffer.push_str(&escape_special_chars_csv(&object_buf));
                        object_buf.clear();
                    }
                    buffer.push(']');
                    buffer.push('"');
                }
                Value::Int(v) => {
                    buffer.push_str(&v.to_string());
                }
                Value::Float(v) => {
                    buffer.push_str(&v.to_string());
                }
                Value::Bool(v) => buffer.push_str(&v.to_string()),
                Value::Date(date_time) => {
                    let v = serialize_date(date_time, date_codec);
                    buffer.push('\"');
                    buffer.push_str(&v);
                    buffer.push('\"');
                }

                Value::Object(hash_map) => {
                    buffer.push('"');
                    let mut object_buf = String::new();

                    Value::json_serialise_record(hash_map, &mut object_buf, date_codec, true)?;
                    buffer.push_str(&escape_special_chars_csv(&object_buf));
                    buffer.push('"');
                }
                Value::Metadata(metadata) => {
                    buffer.push('\"');
                    buffer.push_str(&escape_special_chars_csv(metadata.0.as_str()));
                    buffer.push('\"');
                }
            }
            Ok(())
        })();
        if result.is_err() {
            buffer.truncate(start_len);
        }
        result
    }

    /// Returns a compact string representation of the value, used by `Display`.
    ///
    /// # Arguments
    /// * `date_codec` – Codec for formatting dates.
    pub fn to_string(&self, date_codec: &DateOutputCodec) -> String {
        match self {
            Value::Null() => "null".to_owned(),
            Value::String(v) => v.to_string(),
            Value::Array(values) => {
                let mut buffer = String::new();
                buffer.push('[');

                for (i, el) in values.iter().enumerate() {
                    if i != 0 {
                        buffer.push_str(", ");
                    }
                    buffer.push_str(&el.to_string(date_codec));
                }
                buffer.push(']');
                buffer
            }
            Value::Int(v) => v.to_string(),
            Value::Float(v) => v.to_string(),
            Value::Bool(v) => v.to_string(),
            Value::Date(date_time) => serialize_date(date_time, date_codec),
            Value::Object(record) => {
                let mut buffer = String::new();

                for (i, (key, value)) in (&record.0).into_iter().enumerate() {
                    if i != 0 {
                        buffer.push_str(", ");
                    }
                    buffer.push('\'');
                    buffer.push_str(key);
                    buffer.push('\'');
                    buffer.push(':');
                    buffer.push('\'');
                    buffer.push_str(&value.to_string(date_codec));
                    buffer.push('\'');
                }
                buffer
            }
            Value::Metadata(metadata) => metadata.0.as_str().to_string(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn hash(&self, hasher: &mut Hasher) {
        hash_util::start(hasher, "value");
        self.hash_canonical(hasher);
    }

    pub(crate) fn hash_canonical(&self, hasher: &mut Hasher) {
        match self {
            Value::Null() => {
                hash_util::write_tag(hasher, 0);
            }
            Value::String(s) => {
                hash_util::write_tag(hasher, 1);
                hash_util::write_str(hasher, s);
            }
            Value::Array(values) => {
                hash_util::write_tag(hasher, 2);
                hash_util::write_u64(hasher, values.len() as u64);
                for val in values {
                    val.hash_canonical(hasher);
                }
            }
            Value::Int(i) => {
                hash_util::write_tag(hasher, 3);
                hash_util::write_i64(hasher, *i);
            }
            Value::Float(f) => {
                hash_util::write_tag(hasher, 4);
                hash_util::write_f64(hasher, *f);
            }
            Value::Bool(b) => {
                hash_util::write_tag(hasher, 5);
                hash_util::write_bool(hasher, *b);
            }
            Value::Date(date_time) => {
                hash_util::write_tag(hasher, 6);
                hash_util::write_date(hasher, date_time);
            }
            Value::Object(record) => {
                hash_util::write_tag(hasher, 7);
                hash_util::write_u64(hasher, record.len() as u64);
                for (key, value) in record.iter() {
                    hash_util::write_field(hasher, key);
                    value.hash_canonical(hasher);
                }
            }
            Value::Metadata(metadata) => {
                hash_util::write_tag(hasher, 8);
                hash_util::write_str(hasher, metadata.0.as_str());
            }
        }
    }
}

/// Escape a string so that it can be safely embedded inside a string literal.
///
pub fn escape_special_chars(v: &str) -> String {
    // overestimate to vaoid realocating
    let mut result = String::with_capacity(v.len() * 2);
    for c in v.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\x08' => result.push_str("\\b"),     // backspace
            '\x0C' => result.push_str("\\f"),     // form feed
            '\x0B' => result.push_str("\\u000B"), // vertical tab
            '\u{0000}'..='\u{001F}' | '\u{007F}' => {
                // Escape all C0 and DEL control chars
                result.push_str("\\u");
                result.push_str(&format!("{:04X}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

/// Escape a string so that it can be safely embedded inside a string literal.
///
pub fn escape_special_chars_csv(v: &str) -> String {
    v.replace("\"", "\"\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_special_chars_behaviour() {
        let cases = vec![
            ("simple", "simple"),
            ("quote\"test", "quote\\\"test"),
            ("backslash\\test", "backslash\\\\test"),
            ("newline\n\t", "newline\\n\\t"),
            ("carriage\rreturn", "carriage\\rreturn"),
            ("\x0C", "\\f"),
            ("\x08", "\\b"),
            ("\x0B", "\\u000B"),
            ("\x01", "\\u0001"),
        ];
        for (input, expected) in cases {
            assert_eq!(escape_special_chars(input), expected);
        }
    }

    #[test]
    fn json_serialise_record_escapes_object_keys() {
        let key = "bad\"key\n\t\u{0001}\u{000B}";
        let mut record = Record::new();
        record.add(key, Value::String("value".to_string()));

        let mut buffer = String::new();
        Value::json_serialise_record(&record, &mut buffer, &DateOutputCodec::Iso(), true).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed[key], "value");
    }

    #[test]
    fn json_serialise_date_escapes_custom_pattern_literals() {
        let timestamp = DateTime::parse_from_rfc3339("2020-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        for (pattern, expected) in [
            ("%Y\"%m", "2020\"01"),
            ("%Y\\%m", "2020\\01"),
            ("%Y%n%m", "2020\n01"),
            ("%Y%t%m", "2020\t01"),
        ] {
            let mut buffer = String::new();
            Value::json_serialise_value(
                &Value::Date(timestamp),
                &mut buffer,
                &DateOutputCodec::Pattern(pattern.to_owned()),
                true,
            )
            .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&buffer).unwrap(),
                expected
            );
            assert_eq!(buffer.lines().count(), 1);
        }
    }

    #[test]
    fn json_serialise_nested_dates_preserves_custom_pattern_and_normal_bytes() {
        let timestamp = DateTime::parse_from_rfc3339("2020-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut nested = Record::new();
        nested.add("when", Value::Date(timestamp));
        let value = Value::Array(vec![Value::Date(timestamp), Value::Object(nested)]);
        for (codec, expected, expected_bytes) in [
            (
                DateOutputCodec::Pattern("%Y\"\\%n%t%m".to_owned()),
                "2020\"\\\n\t01",
                "[\"2020\\\"\\\\\\n\\t01\",{\"when\":\"2020\\\"\\\\\\n\\t01\"}]",
            ),
            (
                DateOutputCodec::Iso(),
                "2020-01-02T03:04:05.000000+00:00",
                "[\"2020-01-02T03:04:05.000000+00:00\",{\"when\":\"2020-01-02T03:04:05.000000+00:00\"}]",
            ),
        ] {
            let mut buffer = String::new();
            Value::json_serialise_value(&value, &mut buffer, &codec, true).unwrap();
            assert_eq!(buffer, expected_bytes);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&buffer).unwrap(),
                serde_json::json!([expected, {"when": expected}])
            );
        }
    }

    #[test]
    fn json_serialise_array_omits_only_null_when_include_empty_is_false() {
        let value = Value::Array(vec![Value::Null()]);

        let mut buffer = String::new();
        Value::json_serialise_value(&value, &mut buffer, &DateOutputCodec::Iso(), false).unwrap();

        assert_eq!(buffer, "[]");
    }

    #[test]
    fn json_serialise_array_omits_nulls_without_dangling_commas() {
        let value = Value::Array(vec![
            Value::Null(),
            Value::String("a".to_owned()),
            Value::Null(),
            Value::String("b".to_owned()),
            Value::Null(),
        ]);

        let mut buffer = String::new();
        Value::json_serialise_value(&value, &mut buffer, &DateOutputCodec::Iso(), false).unwrap();

        assert_eq!(buffer, r#"["a","b"]"#);
        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn json_serialise_nested_arrays_and_objects_with_skipped_nulls_parse() {
        let mut object = Record::new();
        object.add("kept", Value::String("value".to_owned()));
        object.add("skipped", Value::Null());

        let value = Value::Array(vec![
            Value::Null(),
            Value::Object(object),
            Value::Array(vec![Value::Null(), Value::Int(7)]),
        ]);

        let mut buffer = String::new();
        Value::json_serialise_value(&value, &mut buffer, &DateOutputCodec::Iso(), false).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed, serde_json::json!([{"kept": "value"}, [7]]));
    }

    #[test]
    fn json_serialise_array_keeps_null_when_include_empty_is_true() {
        let value = Value::Array(vec![Value::Null(), Value::String("a".to_owned())]);

        let mut buffer = String::new();
        Value::json_serialise_value(&value, &mut buffer, &DateOutputCodec::Iso(), true).unwrap();

        assert_eq!(buffer, r#"[null,"a"]"#);
    }

    #[test]
    fn json_serialise_non_finite_floats_as_null() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut buffer = String::new();
            Value::json_serialise_value(
                &Value::Float(value),
                &mut buffer,
                &DateOutputCodec::Iso(),
                true,
            )
            .unwrap();

            assert_eq!(buffer, "null");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&buffer).unwrap(),
                serde_json::Value::Null
            );
        }
    }

    #[test]
    fn metadata_csv_serialise_escapes_raw_json() {
        use std::sync::Arc;

        let raw_metadata = r#"{"computer":"host \"quoted\"","data_type":"evtx"}"#;
        let value = Value::Metadata(MetadataSerialized(Arc::new(raw_metadata.to_owned())));
        let mut buffer = String::new();

        Value::csv_serialise_value(&value, &mut buffer, &DateOutputCodec::Iso()).unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(crate::format_csv::CSV_DELIMITER as u8)
            .from_reader(buffer.as_bytes());
        let record = reader.records().next().unwrap().unwrap();

        assert_eq!(&record[0], raw_metadata);
        assert_eq!(1, record.len());
    }

    // ---------------------------------------------------------------------
    // Tests for `Value::hash`
    // ---------------------------------------------------------------------

    /// Helper that returns the Blake3 hash of a `Value`.
    fn hash_of(val: &Value) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        val.hash(&mut hasher);
        hasher.finalize()
    }

    #[test]
    fn value_hash_null_marker() {
        let actual = hash_of(&Value::Null());
        assert_eq!(
            actual.to_hex().as_str(),
            "434c51309bf384192fbf4f4089b57c46a055df7ea2e3bab0c44b24bf022c9dc9"
        );
    }

    #[test]
    fn value_hash_consistency_across_instances() {
        // Identical logical values must produce identical hashes
        let s1 = Value::String("test".into());
        let s2 = Value::String("test".into());
        assert_eq!(hash_of(&s1), hash_of(&s2));

        let i1 = Value::Int(12345);
        let i2 = Value::Int(12345);
        assert_eq!(hash_of(&i1), hash_of(&i2));

        let f1 = Value::Float(3.5);
        let f2 = Value::Float(3.5);
        assert_eq!(hash_of(&f1), hash_of(&f2));

        let b1 = Value::Bool(true);
        let b2 = Value::Bool(true);
        assert_eq!(hash_of(&b1), hash_of(&b2));

        let dt = chrono::Utc::now();
        let d1 = Value::Date(dt);
        let d2 = Value::Date(dt);
        assert_eq!(hash_of(&d1), hash_of(&d2));
    }

    #[test]
    fn value_hash_variant_distinction() {
        // Different variants should NOT collide
        let h_null = hash_of(&Value::Null());
        let h_str = hash_of(&Value::String("null".into()));
        assert_ne!(h_null, h_str);

        let h_true = hash_of(&Value::Bool(true));
        let h_false = hash_of(&Value::Bool(false));
        assert_ne!(h_true, h_false);
    }

    #[test]
    fn value_hash_array_order_matters() {
        let a1 = Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let a2 = Value::Array(vec![Value::Int(3), Value::Int(2), Value::Int(1)]);
        assert_ne!(hash_of(&a1), hash_of(&a2));
    }

    #[test]
    fn value_hash_frames_array_elements() {
        let first = Value::Array(vec![
            Value::String("ab".to_owned()),
            Value::String("c".to_owned()),
        ]);
        let second = Value::Array(vec![
            Value::String("a".to_owned()),
            Value::String("bc".to_owned()),
        ]);

        assert_ne!(hash_of(&first), hash_of(&second));
    }

    #[test]
    fn value_is_null_distinguishes_null_from_other_values() {
        assert!(Value::Null().is_null());
        assert!(!Value::String("".to_string()).is_null());
    }

    #[test]
    fn value_to_string_formats_arrays_objects_and_metadata() {
        use std::sync::Arc;

        let array = Value::Array(vec![Value::Int(1), Value::String("two".to_string())]);
        assert_eq!(array.to_string(&DateOutputCodec::Iso()), "[1, two]");

        let mut record = Record::new();
        record.add("a", Value::Int(1));
        record.add("b", Value::String("two".to_string()));
        let object = Value::Object(record);
        assert_eq!(
            object.to_string(&DateOutputCodec::Iso()),
            "'a':'1', 'b':'two'"
        );

        let metadata = Value::Metadata(MetadataSerialized(Arc::new("raw".to_string())));
        assert_eq!(metadata.to_string(&DateOutputCodec::Iso()), "raw");
    }
}
