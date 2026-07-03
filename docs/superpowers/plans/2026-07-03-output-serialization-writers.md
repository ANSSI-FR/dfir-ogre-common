# Output Serialization Writers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix JSON/CSV output escaping and delimiter bugs by introducing small internal writers for JSON objects, JSON arrays, and CSV rows.

**Architecture:** Add `src/serialise.rs` as a crate-private fragment writer module. Migrate `Value`, `Metadata`, timeline serialization, and local formatter extra-field helpers to use these writers without changing formatter control flow or output file behavior.

**Tech Stack:** Rust 2024, existing crate modules, `serde_json` and `csv` for verification in unit tests.

---

## File Structure

- Create `src/serialise.rs`: crate-private `JsonObjectWriter`, `JsonArrayWriter`, and `CsvRowWriter`; no file I/O and no formatter state.
- Modify `src/lib.rs`: add `mod serialise;`.
- Modify `src/value.rs`: add JSON array omission tests first, then use `JsonObjectWriter` and `JsonArrayWriter`.
- Modify `src/metadata.rs`: add metadata escaping tests first, then use `JsonObjectWriter` and `CsvRowWriter`.
- Modify `src/timeline.rs`: add timeline JSON/CSV serialization tests first, then use `JsonObjectWriter` and `CsvRowWriter`.
- Modify `src/format_json.rs`: use `JsonObjectWriter` for record-plus-extra-field delimiter/key handling.
- Modify `src/format_csv.rs`: use `CsvRowWriter` for record-plus-extra-field delimiter handling.

## Task 1: Add Failing `Value` Tests For Skipped JSON Array Nulls

**Files:**
- Modify: `src/value.rs`

- [ ] **Step 1: Add tests in `src/value.rs` inside the existing `#[cfg(test)] mod tests`**

```rust
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
```

- [ ] **Step 2: Run the focused failing tests**

Run:

```bash
cargo test json_serialise_array_ -- --nocapture
```

Expected: at least `json_serialise_array_omits_nulls_without_dangling_commas` fails because current output contains invalid comma placement such as `[,\"a\",,\"b\",]`.

- [ ] **Step 3: Commit the failing tests**

```bash
git add src/value.rs
git commit -m "Add JSON array null omission tests"
```

## Task 2: Add Central Serialization Writers

**Files:**
- Create: `src/serialise.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/serialise.rs`**

```rust
use crate::{escape_special_chars, format_csv::CSV_DELIMITER, value::escape_special_chars_csv};

pub(crate) struct JsonObjectWriter<'a> {
    buffer: &'a mut String,
    fields: usize,
}

impl<'a> JsonObjectWriter<'a> {
    pub(crate) fn new(buffer: &'a mut String) -> Self {
        buffer.push('{');
        Self { buffer, fields: 0 }
    }

    pub(crate) fn field_raw<F>(&mut self, name: &str, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        self.write_field_prefix(name);
        write_value(self.buffer);
    }

    pub(crate) fn field_if<F>(&mut self, name: &str, should_emit: bool, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        if should_emit {
            self.field_raw(name, write_value);
        }
    }

    pub(crate) fn finish(self) {
        self.buffer.push('}');
    }

    fn write_field_prefix(&mut self, name: &str) {
        if self.fields != 0 {
            self.buffer.push(',');
        }
        self.buffer.push('"');
        self.buffer.push_str(&escape_special_chars(name));
        self.buffer.push_str("\":");
        self.fields += 1;
    }
}

pub(crate) struct JsonArrayWriter<'a> {
    buffer: &'a mut String,
    elements: usize,
}

impl<'a> JsonArrayWriter<'a> {
    pub(crate) fn new(buffer: &'a mut String) -> Self {
        buffer.push('[');
        Self {
            buffer,
            elements: 0,
        }
    }

    pub(crate) fn element_raw<F>(&mut self, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        if self.elements != 0 {
            self.buffer.push(',');
        }
        self.elements += 1;
        write_value(self.buffer);
    }

    pub(crate) fn element_if<F>(&mut self, should_emit: bool, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        if should_emit {
            self.element_raw(write_value);
        }
    }

    pub(crate) fn finish(self) {
        self.buffer.push(']');
    }
}

pub(crate) struct CsvRowWriter<'a> {
    buffer: &'a mut String,
    fields: usize,
}

impl<'a> CsvRowWriter<'a> {
    pub(crate) fn new(buffer: &'a mut String) -> Self {
        Self { buffer, fields: 0 }
    }

    pub(crate) fn raw_field(&mut self, value: &str) {
        self.write_delimiter();
        self.buffer.push_str(value);
    }

    pub(crate) fn quoted_field(&mut self, value: &str) {
        self.write_delimiter();
        self.buffer.push('"');
        self.buffer.push_str(&escape_special_chars_csv(value));
        self.buffer.push('"');
    }

    pub(crate) fn pre_escaped_quoted_field(&mut self, value: &str) {
        self.write_delimiter();
        self.buffer.push('"');
        self.buffer.push_str(value);
        self.buffer.push('"');
    }

    pub(crate) fn optional_quoted_field(&mut self, value: Option<&str>) {
        match value {
            Some(value) => self.quoted_field(value),
            None => self.empty_field(),
        }
    }

    pub(crate) fn empty_field(&mut self) {
        self.write_delimiter();
    }

    fn write_delimiter(&mut self) {
        if self.fields != 0 {
            self.buffer.push(CSV_DELIMITER);
        }
        self.fields += 1;
    }
}
```

- [ ] **Step 2: Register the module in `src/lib.rs`**

Add near the other private modules:

```rust
mod serialise;
```

- [ ] **Step 3: Check formatting for the new module**

Run:

```bash
cargo fmt --check
```

Expected: pass or show only formatting diffs needed in files touched by this task.

- [ ] **Step 4: Commit the writer module**

```bash
git add src/lib.rs src/serialise.rs
git commit -m "Add output serialization fragment writers"
```

## Task 3: Migrate `Value` JSON Serialization To Writers

**Files:**
- Modify: `src/value.rs`

- [ ] **Step 1: Import writers and add the emission predicate**

Near the existing `use crate::{...};` import, include `serialise::{JsonArrayWriter, JsonObjectWriter}`:

```rust
use crate::{
    DateOutputCodec, Error, Record, metadata::MetadataSerialized,
    serialise::{JsonArrayWriter, JsonObjectWriter},
    serialize_date,
};
```

Inside `impl Value`, add:

```rust
    fn should_emit_json_value(value: &Value, include_empty: bool) -> bool {
        include_empty || !matches!(value, Value::Null())
    }
```

- [ ] **Step 2: Replace `json_serialise_record` object delimiter logic**

Replace the current body with:

```rust
        let mut object = JsonObjectWriter::new(buffer);
        for (key, el) in record.iter() {
            let should_emit = Self::should_emit_json_value(el, include_empty);
            let mut value_buffer = String::new();
            if should_emit {
                Value::json_serialise_value(el, &mut value_buffer, date_codec, include_empty)?;
            }
            object.field_if(key, should_emit, |buffer| {
                buffer.push_str(&value_buffer);
            });
        }
        object.finish();
        Ok(())
```

- [ ] **Step 3: Replace the `Value::Array` branch**

Use:

```rust
            Value::Array(field_values) => {
                let mut array = JsonArrayWriter::new(buffer);
                for el in field_values {
                    let should_emit = Self::should_emit_json_value(el, include_empty);
                    let mut value_buffer = String::new();
                    if should_emit {
                        Value::json_serialise_value(el, &mut value_buffer, date_codec, include_empty)?;
                    }
                    array.element_if(should_emit, |buffer| {
                        buffer.push_str(&value_buffer);
                    });
                }
                array.finish();
            }
```

- [ ] **Step 4: Run the focused tests**

Run:

```bash
cargo test json_serialise_array_ json_serialise_record_escapes_object_keys -- --nocapture
```

Expected: all matching tests pass. If Cargo rejects multiple filters, run them separately:

```bash
cargo test json_serialise_array_ -- --nocapture
cargo test json_serialise_record_escapes_object_keys -- --nocapture
```

- [ ] **Step 5: Commit the `Value` migration**

```bash
git add src/value.rs src/serialise.rs
git commit -m "Use JSON writers for value serialization"
```

## Task 4: Add Failing Metadata Escaping Tests

**Files:**
- Modify: `src/metadata.rs`

- [ ] **Step 1: Add tests inside `src/metadata.rs` tests module**

```rust
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
```

- [ ] **Step 2: Run the focused failing tests**

Run:

```bash
cargo test metadata::tests::json_serialise_escapes_computer_and_data_type metadata::tests::csv_serialise_escapes_computer_and_data_type -- --nocapture
```

Expected: Cargo may reject multiple filters; if so run each separately. The JSON test should fail before the implementation because raw quotes/newlines make invalid JSON.

- [ ] **Step 3: Commit the failing metadata tests**

```bash
git add src/metadata.rs
git commit -m "Add metadata escaping tests"
```

## Task 5: Migrate `Metadata` Serialization To Writers

**Files:**
- Modify: `src/metadata.rs`
- Modify: `src/serialise.rs` if Task 3 changed writer methods to return `Result`

- [ ] **Step 1: Replace imports in `src/metadata.rs`**

Use:

```rust
use crate::{
    DateOutputCodec, escape_special_chars, format_csv::CSV_DELIMITER,
    serialise::{CsvRowWriter, JsonObjectWriter},
    serialize_date,
};
```

Remove direct `value::escape_special_chars_csv` import if it becomes unused.

- [ ] **Step 2: Rewrite `csv_serialise` to use `CsvRowWriter`**

Replace `csv_serialise` with:

```rust
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
```

Replace `serialise_csv_date` with:

```rust
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
```

Remove `serialise_csv_string` if unused.

- [ ] **Step 3: Rewrite `json_serialise` to use `JsonObjectWriter`**

Use:

```rust
    pub fn json_serialise(&self, buffer: &mut String, date_codec: &DateOutputCodec) {
        let mut object = JsonObjectWriter::new(buffer);
        object.field_raw("computer", |buffer| {
            buffer.push('"');
            buffer.push_str(&escape_special_chars(&self.computer));
            buffer.push('"');
        });
        object.field_raw("data_type", |buffer| {
            buffer.push('"');
            buffer.push_str(&escape_special_chars(&self.data_type));
            buffer.push('"');
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
```

Replace helper signatures:

```rust
    fn serialise_json_string(object: &mut JsonObjectWriter<'_>, name: &str, value: &Option<String>) {
        object.field_if(name, value.is_some(), |buffer| {
            buffer.push('"');
            buffer.push_str(&escape_special_chars(value.as_deref().unwrap_or_default()));
            buffer.push('"');
        });
    }

    fn serialise_json_date(
        object: &mut JsonObjectWriter<'_>,
        name: &str,
        date: &Option<DateTime<Utc>>,
        date_codec: &DateOutputCodec,
    ) {
        object.field_if(name, date.is_some(), |buffer| {
            buffer.push('"');
            buffer.push_str(&serialize_date(date.as_ref().unwrap(), date_codec));
            buffer.push('"');
        });
    }
```

- [ ] **Step 4: Run metadata tests**

Run:

```bash
cargo test metadata::tests -- --nocapture
```

Expected: all metadata tests pass. The exact serialized CSV text may now quote the first two fields; parsed CSV assertions should remain valid.

- [ ] **Step 5: Commit metadata migration**

```bash
git add src/metadata.rs src/serialise.rs
git commit -m "Use writers for metadata serialization"
```

## Task 6: Add Timeline Serialization Tests

**Files:**
- Modify: `src/timeline.rs`

- [ ] **Step 1: Add tests inside `src/timeline.rs` tests module**

```rust
    fn timeline_for_serialization_tests() -> TimeLine {
        TimeLine {
            computer: Some("COMP;\"1\"\nnext".to_owned()),
            timestamp: crate::date_util::parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso())
                .unwrap(),
            timestamp_meaning: "Created".to_owned(),
            data_type: "evtx".to_owned(),
            related_user: "user".to_owned(),
            description: "desc".to_owned(),
            additional_description: "add".to_owned(),
            metadata: Some(MetadataSerialized(Arc::new(
                r#"{"computer":"COMP;\"1\"\nnext","data_type":"evtx"}"#.to_owned(),
            ))),
            data: Some({
                let mut record = Record::new();
                record.add("message", Value::String("hello;\"world\"\nnext".to_owned()));
                record
            }),
            ..TimeLine::default()
        }
    }

    #[test]
    fn timeline_json_serialise_with_metadata_and_data_parses() {
        let tl = timeline_for_serialization_tests();
        let mut json = String::new();

        tl.json_serialise(&mut json, &DateOutputCodec::IsoUtc(), false)
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["computer"], "COMP;\"1\"\nnext");
        assert_eq!(parsed["metadata"]["data_type"], "evtx");
        assert_eq!(parsed["data"]["message"], "hello;\"world\"\nnext");
    }

    #[test]
    fn timeline_json_serialise_missing_optional_metadata_and_data_parses() {
        let mut tl = timeline_for_serialization_tests();
        tl.metadata = None;
        tl.data = None;

        let mut json = String::new();
        tl.json_serialise(&mut json, &DateOutputCodec::IsoUtc(), false)
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("metadata").is_none());
        assert!(parsed.get("data").is_none());
        assert!(!json.contains(",,"));
        assert!(!json.contains(",}"));
    }

    #[test]
    fn timeline_csv_serialise_quotes_metadata_and_data_fields() {
        let tl = timeline_for_serialization_tests();

        let mut csv = String::new();
        TimeLine::csv_serialise_header(&mut csv, false);
        csv.push('\n');
        tl.csv_serialise(&mut csv, &DateOutputCodec::IsoUtc(), false, false)
            .unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(csv.as_bytes()));
        let record = reader.records().next().unwrap().unwrap();

        assert_eq!(&record[0], "COMP;\"1\"\nnext");
        assert_eq!(record[7].contains(r#""data_type":"evtx""#), true);
        assert_eq!(record[8].contains("hello;\\\"world\\\"\\nnext"), true);
        assert_eq!(9, record.len());
    }
```

If `Arc`, `MetadataSerialized`, `Record`, or `Value` are not already imported in the tests module, add them to the test imports:

```rust
    use std::sync::Arc;
    use crate::{Record, Value, metadata::MetadataSerialized};
```

- [ ] **Step 2: Run focused timeline tests**

Run:

```bash
cargo test timeline_json_serialise timeline_csv_serialise -- --nocapture
```

Expected: Cargo may require running filters separately. The tests should expose any current delimiter or CSV quoting weaknesses before migration.

- [ ] **Step 3: Commit timeline tests**

```bash
git add src/timeline.rs
git commit -m "Add timeline serialization writer tests"
```

## Task 7: Migrate `TimeLine` Serialization To Writers

**Files:**
- Modify: `src/timeline.rs`

- [ ] **Step 1: Update imports**

Add:

```rust
    serialise::{CsvRowWriter, JsonObjectWriter},
```

to the existing `use crate::{...};` list.

- [ ] **Step 2: Replace `json_serialise` delimiter handling**

Use `JsonObjectWriter` and preserve field order:

```rust
        let mut object = JsonObjectWriter::new(buffer);
        object.field_if("id", self.id.is_some(), |buffer| {
            Self::write_json_string_value(self.id.as_deref().unwrap_or_default(), buffer);
        });
        object.field_if("computer", self.computer.is_some(), |buffer| {
            Self::write_json_string_value(self.computer.as_deref().unwrap_or_default(), buffer);
        });
        object.field_raw("timestamp", |buffer| {
            Self::write_json_string_value(&serialize_date(&self.timestamp, date_codec), buffer);
        });
        object.field_raw("timestamp_meaning", |buffer| {
            Self::write_json_string_value(&self.timestamp_meaning, buffer);
        });
        object.field_raw("data_type", |buffer| {
            Self::write_json_string_value(&self.data_type, buffer);
        });
        object.field_raw("related_user", |buffer| {
            Self::write_json_string_value(&self.related_user, buffer);
        });
        object.field_raw("description", |buffer| {
            Self::write_json_string_value(&self.description, buffer);
        });
        object.field_raw("additional_description", |buffer| {
            Self::write_json_string_value(&self.additional_description, buffer);
        });
        object.field_if("metadata_id", self.metadata_id.is_some(), |buffer| {
            Self::write_json_string_value(self.metadata_id.as_deref().unwrap_or_default(), buffer);
        });
        object.field_if("metadata", self.metadata.is_some(), |buffer| {
            buffer.push_str(self.metadata.as_ref().unwrap().0.as_str());
        });
        object.field_if("data_id", self.data_id.is_some(), |buffer| {
            Self::write_json_string_value(self.data_id.as_deref().unwrap_or_default(), buffer);
        });
        object.field_if("data", self.data.is_some(), |buffer| {
            buffer.push_str(&data_buffer);
        });
        object.finish();
        Ok(())
```

Before creating the `JsonObjectWriter`, prepare the optional data fragment so errors still propagate before any delimiter is emitted:

```rust
        let mut data_buffer = String::new();
        if let Some(data) = &self.data {
            data.json_serialise(&mut data_buffer, date_codec, include_empty)?;
        }
```

Add helper:

```rust
    fn write_json_string_value(value: &str, buffer: &mut String) {
        buffer.push('"');
        buffer.push_str(&escape_special_chars(value));
        buffer.push('"');
    }
```

Remove `serialise_json_string` and `serialise_json_date` if unused.

- [ ] **Step 3: Replace `csv_serialise` delimiter handling**

Use:

```rust
        let mut row = CsvRowWriter::new(buffer);
        if normalized {
            row.quoted_field(self.id.as_deref().unwrap_or_default());
        }
        row.quoted_field(self.computer.as_deref().unwrap_or_default());
        row.raw_field(&serialize_date(&self.timestamp, date_codec));
        row.quoted_field(&self.timestamp_meaning);
        row.quoted_field(&self.data_type);
        row.quoted_field(&self.related_user);
        row.quoted_field(&self.description);
        row.quoted_field(&self.additional_description);

        if normalized {
            row.quoted_field(self.metadata_id.as_deref().unwrap_or_default());
            row.quoted_field(self.data_id.as_deref().unwrap_or_default());
        } else {
            row.quoted_field(self.metadata.as_ref().map(|m| m.0.as_str()).unwrap_or_default());
            if let Some(data) = &self.data {
                let mut object_buffer = String::new();
                data.json_serialise(&mut object_buffer, date_codec, include_empty)?;
                row.quoted_field(&object_buffer);
            } else {
                row.empty_field();
            }
        }
        Ok(())
```

Remove `serialise_csv_string` and `serialise_csv_date` if unused.

- [ ] **Step 4: Run timeline tests**

Run:

```bash
cargo test timeline::tests::timeline_struct_serialisation timeline_json_serialise timeline_csv_serialise -- --nocapture
```

Expected: all timeline tests pass. If Cargo requires one filter at a time, run each failing filter separately.

- [ ] **Step 5: Commit timeline migration**

```bash
git add src/timeline.rs src/serialise.rs
git commit -m "Use writers for timeline serialization"
```

## Task 8: Migrate Formatter Extra-Field Helpers

**Files:**
- Modify: `src/format_json.rs`
- Modify: `src/format_csv.rs`

- [ ] **Step 1: Update JSON extra-field helper**

In `src/format_json.rs`, import:

```rust
    serialise::JsonObjectWriter,
```

Change `JsonExtraField::json_serialise` to remain value-only, and replace `json_serialise_record_with_extra_field` with writer-owned delimiters:

```rust
    let mut object = JsonObjectWriter::new(buffer);
    for (key, value) in record.iter() {
        let should_emit = include_empty || !matches!(value, Value::Null());
        let mut value_buffer = String::new();
        if should_emit {
            Value::json_serialise_value(value, &mut value_buffer, date_codec, include_empty)?;
        }
        object.field_if(key, should_emit, |buffer| {
            buffer.push_str(&value_buffer);
        });
    }
    object.field_raw(extra_key, |buffer| extra_value.json_serialise(buffer));
    object.finish();
    Ok(())
```

- [ ] **Step 2: Update CSV extra-field helper**

In `src/format_csv.rs`, import:

```rust
    serialise::CsvRowWriter,
```

Replace `CsvExtraField::csv_serialise` with:

```rust
    fn write_to_row(&self, row: &mut CsvRowWriter<'_>) {
        match self {
            Self::Metadata(metadata) => row.pre_escaped_quoted_field(metadata.0.as_str()),
            Self::String(value) => row.quoted_field(value),
        }
    }
```

Replace `csv_serialise_record_with_extra_field` with:

```rust
    let mut row = CsvRowWriter::new(buffer);
    for (_, value) in record.iter() {
        let mut field = String::new();
        Value::csv_serialise_value(value, &mut field, date_codec)?;
        row.raw_field(&field);
    }
    extra_value.write_to_row(&mut row);
    Ok(())
```

This keeps `MetadataSerialized` behavior unchanged in `CsvFormatter`: that value is already CSV-escaped by `CsvFormatter::serialized_metadata`, so it must be wrapped in quotes without escaping a second time.

- [ ] **Step 3: Run existing formatter-focused tests**

Run:

```bash
cargo test format_json::tests format_csv::tests -- --nocapture
```

Expected: all formatter tests pass. Existing assertions that parse CSV/JSON should continue to pass.

- [ ] **Step 4: Commit formatter helper migration**

```bash
git add src/format_json.rs src/format_csv.rs src/serialise.rs
git commit -m "Use serialization writers for formatter extra fields"
```

## Task 9: Full Validation And Cleanup

**Files:**
- Check all changed files.

- [ ] **Step 1: Run formatting**

```bash
cargo fmt --check
```

Expected: pass. If it fails, run `cargo fmt`, inspect the diff, then rerun `cargo fmt --check`.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: pass with no warnings. Fix any warnings in the smallest relevant module.

- [ ] **Step 3: Run the full test suite**

```bash
LD_LIBRARY_PATH=/home/asalais/.local/share/uv/python/cpython-3.13.5-linux-x86_64-gnu/lib cargo test
```

Expected: pass. Do not add or run Python unit tests.

- [ ] **Step 4: Inspect final diff**

```bash
git status --short
git diff --stat
git diff -- src/serialise.rs src/value.rs src/metadata.rs src/timeline.rs src/format_json.rs src/format_csv.rs src/lib.rs
```

Expected: only intended implementation files are modified; no build outputs or local artifacts are present.

- [ ] **Step 5: Commit final cleanup if needed**

If validation fixes changed files after Task 8, commit them:

```bash
git add src/serialise.rs src/value.rs src/metadata.rs src/timeline.rs src/format_json.rs src/format_csv.rs src/lib.rs
git commit -m "Polish output serialization writers"
```

If there are no changes after validation, do not create an empty commit.

## Self-Review Checklist

- Spec coverage: Tasks 1-3 cover JSON array skipped null behavior and writer contracts. Tasks 4-5 cover metadata JSON/CSV escaping. Tasks 6-7 cover timeline serialization and tests. Task 8 covers formatter extra-field helper usage. Task 9 covers required validation.
- Scope: The plan does not replace the output stack with `serde_json` or `csv`, does not refactor formatter control flow, and does not change output filenames, metadata caching, normalized behavior, line counting, headers, or Python tests.
- Writer naming: The plan uses `JsonObjectWriter`, `JsonArrayWriter`, and `CsvRowWriter`; delimiter ownership is inside these types.
- Skipped JSON values: The plan defines `Value::should_emit_json_value` and uses `field_if`/`element_if` before writing delimiters.
