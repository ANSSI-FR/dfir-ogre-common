# Output Serialization Writer Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix metadata CSV escaping and simplify writer-based JSON serialization without changing output shape.

**Architecture:** Extend `src/serialise.rs` so JSON writers support fallible direct writes with rollback and string-field helpers. Then migrate the affected value, formatter, metadata, and timeline serializers to those helpers while preserving raw-JSON `MetadataSerialized` and CSV escaping at CSV output boundaries.

**Tech Stack:** Rust 2024, crate-local `Error`, `csv::ReaderBuilder` for CSV regression tests, existing inline `#[cfg(test)]` modules.

---

## File Structure

- Modify `src/serialise.rs`: add fallible JSON writer methods, string helpers, rollback behavior, and direct writer tests.
- Modify `src/value.rs`: remove temporary JSON buffers in record/array serialization, add rollback around JSON serializers, and fix `Value::Metadata` CSV escaping.
- Modify `src/format_json.rs`: serialize formatter record values directly into `JsonObjectWriter`.
- Modify `src/metadata.rs`: use JSON string helpers for metadata string fields.
- Modify `src/timeline.rs`: use JSON string helpers for timeline string fields.

## Task 1: Extend Writer Helpers With Rollback

**Files:**
- Modify: `src/serialise.rs`

- [ ] **Step 1: Add failing writer helper tests**

Add this test module at the end of `src/serialise.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn json_object_writer_writes_strings_and_skips_missing_optionals() {
        let mut buffer = String::new();
        let mut object = JsonObjectWriter::new(&mut buffer);

        object.string_field("bad\"key", "value\"with\\chars\n");
        object.optional_string_field("missing", None);
        object.optional_string_field("present", Some("kept"));
        object.finish();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed["bad\"key"], "value\"with\\chars\n");
        assert_eq!(parsed["present"], "kept");
        assert!(parsed.get("missing").is_none());
    }

    #[test]
    fn json_object_writer_rolls_back_failed_field() {
        let mut buffer = String::from("prefix");
        let result = {
            let mut object = JsonObjectWriter::new(&mut buffer);
            let result = object.try_field_raw("bad", |buffer| {
                buffer.push_str("\"partial\"");
                Err(Error::PluginError("boom".to_owned()))
            });
            object.string_field("ok", "value");
            object.finish();
            result
        };

        assert!(matches!(result, Err(Error::PluginError(_))));
        assert_eq!(buffer, r#"prefix{"ok":"value"}"#);
    }

    #[test]
    fn json_array_writer_rolls_back_failed_element() {
        let mut buffer = String::from("prefix");
        let result = {
            let mut array = JsonArrayWriter::new(&mut buffer);
            array.element_raw(|buffer| buffer.push_str("1"));
            let result = array.try_element_raw(|buffer| {
                buffer.push_str("partial");
                Err(Error::PluginError("boom".to_owned()))
            });
            array.element_raw(|buffer| buffer.push_str("2"));
            array.finish();
            result
        };

        assert!(matches!(result, Err(Error::PluginError(_))));
        assert_eq!(buffer, "prefix[1,2]");
    }

    #[test]
    fn csv_row_writer_quotes_and_escapes_fields() {
        let mut buffer = String::new();
        let mut row = CsvRowWriter::new(&mut buffer);

        row.quoted_field("a\"b");
        row.optional_quoted_field(None);
        row.optional_quoted_field(Some("c"));

        assert_eq!(buffer, r#""a""b";;"c""#);
    }
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
cargo test serialise::tests
```

Expected: compile failure for missing `string_field`, `optional_string_field`, `try_field_raw`, and `try_element_raw`.

- [ ] **Step 3: Add helper implementations**

Update the import at the top of `src/serialise.rs`:

```rust
use crate::{Error, escape_special_chars, format_csv::CSV_DELIMITER, value::escape_special_chars_csv};
```

Add these methods to `impl<'a> JsonObjectWriter<'a>` after `field_raw`:

```rust
    pub(crate) fn try_field_raw<F>(&mut self, name: &str, write_value: F) -> Result<(), Error>
    where
        F: FnOnce(&mut String) -> Result<(), Error>,
    {
        let buffer_len = self.buffer.len();
        let fields = self.fields;
        self.write_field_prefix(name);
        if let Err(error) = write_value(self.buffer) {
            self.buffer.truncate(buffer_len);
            self.fields = fields;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn string_field(&mut self, name: &str, value: &str) {
        self.field_raw(name, |buffer| {
            buffer.push('"');
            buffer.push_str(&escape_special_chars(value));
            buffer.push('"');
        });
    }

    pub(crate) fn optional_string_field(&mut self, name: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.string_field(name, value);
        }
    }
```

Add this method to `impl<'a> JsonArrayWriter<'a>` after `element_raw`:

```rust
    pub(crate) fn try_element_raw<F>(&mut self, write_value: F) -> Result<(), Error>
    where
        F: FnOnce(&mut String) -> Result<(), Error>,
    {
        let buffer_len = self.buffer.len();
        let elements = self.elements;
        if self.elements != 0 {
            self.buffer.push(',');
        }
        self.elements += 1;
        if let Err(error) = write_value(self.buffer) {
            self.buffer.truncate(buffer_len);
            self.elements = elements;
            return Err(error);
        }
        Ok(())
    }
```

- [ ] **Step 4: Run writer helper tests and verify they pass**

Run:

```bash
cargo test serialise::tests
```

Expected: all `serialise::tests` pass.

- [ ] **Step 5: Commit writer helpers**

Run:

```bash
git add src/serialise.rs
git commit -m "Add rollback-safe serialization writer helpers"
```

## Task 2: Update Value Serialization And Fix Metadata CSV Escaping

**Files:**
- Modify: `src/value.rs`

- [ ] **Step 1: Add the metadata CSV regression test**

In `src/value.rs`, add this test inside `mod tests`:

```rust
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
```

- [ ] **Step 2: Run the metadata CSV test and verify it fails**

Run:

```bash
cargo test value::tests::metadata_csv_serialise_escapes_raw_json
```

Expected: failure while parsing CSV or assertion failure because embedded quotes are not CSV-escaped.

- [ ] **Step 3: Replace `json_serialise_record` with direct fallible writes and rollback**

Replace `Value::json_serialise_record` with:

```rust
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
```

- [ ] **Step 4: Wrap `json_serialise_value` in rollback and remove array temp buffers**

Replace the body of `Value::json_serialise_value` with this structure:

```rust
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
                    buffer.push('"');
                    buffer.push_str(&escaped);
                    buffer.push('"');
                }
                Value::Array(field_values) => {
                    let mut array = JsonArrayWriter::new(buffer);
                    for el in field_values {
                        if Self::should_emit_json_value(el, include_empty) {
                            array.try_element_raw(|buffer| {
                                Value::json_serialise_value(
                                    el,
                                    buffer,
                                    date_codec,
                                    include_empty,
                                )
                            })?;
                        }
                    }
                    array.finish();
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
                    buffer.push('"');
                    buffer.push_str(&v);
                    buffer.push('"');
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
```

- [ ] **Step 5: Fix `Value::Metadata` CSV escaping**

Replace the `Value::Metadata` arm in `Value::csv_serialise_value` with:

```rust
            Value::Metadata(metadata) => {
                buffer.push('"');
                buffer.push_str(&escape_special_chars_csv(metadata.0.as_str()));
                buffer.push('"');
            }
```

- [ ] **Step 6: Run value tests**

Run:

```bash
cargo test value::tests
```

Expected: all `value::tests` pass.

- [ ] **Step 7: Commit value serialization changes**

Run:

```bash
git add src/value.rs
git commit -m "Fix metadata CSV escaping"
```

## Task 3: Update JSON Formatter Extra Field Serialization

**Files:**
- Modify: `src/format_json.rs`

- [ ] **Step 1: Replace temporary value buffers in `json_serialise_record_with_extra_field`**

Replace `json_serialise_record_with_extra_field` with:

```rust
fn json_serialise_record_with_extra_field(
    record: &Record,
    extra_key: &str,
    extra_value: JsonExtraField<'_>,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        for (key, value) in record.iter() {
            let should_emit = include_empty || !matches!(value, Value::Null());
            if should_emit {
                object.try_field_raw(key, |buffer| {
                    Value::json_serialise_value(value, buffer, date_codec, include_empty)
                })?;
            }
        }
        object.field_raw(extra_key, |buffer| extra_value.json_serialise(buffer));
        object.finish();
        Ok(())
    })();
    if result.is_err() {
        buffer.truncate(start_len);
    }
    result
}
```

- [ ] **Step 2: Run formatter JSON tests**

Run:

```bash
cargo test format_json::tests
```

Expected: all `format_json::tests` pass.

- [ ] **Step 3: Commit JSON formatter change**

Run:

```bash
git add src/format_json.rs
git commit -m "Use fallible JSON writer in formatter"
```

## Task 4: Simplify Metadata And Timeline JSON String Fields

**Files:**
- Modify: `src/metadata.rs`
- Modify: `src/timeline.rs`

- [ ] **Step 1: Update metadata JSON serialization helpers**

In `src/metadata.rs`, change the crate import from:

```rust
use crate::{
    DateOutputCodec, escape_special_chars,
    format_csv::CSV_DELIMITER,
    serialise::{CsvRowWriter, JsonObjectWriter},
    serialize_date,
};
```

to:

```rust
use crate::{
    DateOutputCodec,
    format_csv::CSV_DELIMITER,
    serialise::{CsvRowWriter, JsonObjectWriter},
    serialize_date,
};
```

Replace the initial `computer` and `data_type` fields in `Metadata::json_serialise` with:

```rust
        object.string_field("computer", &self.computer);
        object.string_field("data_type", &self.data_type);
```

Replace `serialise_json_string` with:

```rust
    fn serialise_json_string(
        object: &mut JsonObjectWriter<'_>,
        name: &str,
        value: &Option<String>,
    ) {
        object.optional_string_field(name, value.as_deref());
    }
```

Replace `serialise_json_date` with:

```rust
    fn serialise_json_date(
        object: &mut JsonObjectWriter<'_>,
        name: &str,
        date: &Option<DateTime<Utc>>,
        date_codec: &DateOutputCodec,
    ) {
        if let Some(date) = date {
            object.string_field(name, &serialize_date(date, date_codec));
        }
    }
```

- [ ] **Step 2: Update timeline JSON serialization helpers**

In `src/timeline.rs`, remove `escape_special_chars` from the crate import:

```rust
use crate::{
    FieldName, Record, Value,
    date_util::DateOutputCodec,
    errors::Error,
    format_csv::CSV_DELIMITER,
    metadata::MetadataSerialized,
    serialise::{CsvRowWriter, JsonObjectWriter},
    serialize_date,
};
```

In `TimeLine::json_serialise`, replace the string field writes with:

```rust
        object.optional_string_field("id", self.id.as_deref());
        object.optional_string_field("computer", self.computer.as_deref());
        object.string_field("timestamp", &serialize_date(&self.timestamp, date_codec));
        object.string_field("timestamp_meaning", &self.timestamp_meaning);
        object.string_field("data_type", &self.data_type);
        object.string_field("related_user", &self.related_user);
        object.string_field("description", &self.description);
        object.string_field("additional_description", &self.additional_description);
        object.optional_string_field("metadata_id", self.metadata_id.as_deref());
        object.field_if("metadata", self.metadata.is_some(), |buffer| {
            buffer.push_str(self.metadata.as_ref().unwrap().0.as_str());
        });
        object.optional_string_field("data_id", self.data_id.as_deref());
        object.field_if("data", self.data.is_some(), |buffer| {
            buffer.push_str(&data_buffer);
        });
```

Remove the now-unused `write_json_string_value` method from `impl TimeLine`.

- [ ] **Step 3: Run metadata and timeline tests**

Run:

```bash
cargo test metadata::tests
cargo test timeline::tests
```

Expected: all `metadata::tests` and `timeline::tests` pass.

- [ ] **Step 4: Commit metadata and timeline cleanup**

Run:

```bash
git add src/metadata.rs src/timeline.rs
git commit -m "Use JSON string writer helpers"
```

## Task 5: Full Verification

**Files:**
- No planned source edits.

- [ ] **Step 1: Run formatting check**

Run:

```bash
cargo fmt --check
```

Expected: exit code 0.

- [ ] **Step 2: Run strict clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: exit code 0 with no warnings.

- [ ] **Step 3: Run full test suite**

Run:

```bash
cargo test
```

Expected: all Rust unit tests and doc tests pass.

- [ ] **Step 4: Review final diff**

Run:

```bash
git diff --stat HEAD~4..HEAD
git status --short --branch
```

Expected: the diff contains only the planned serialization files, and the working tree is clean.
