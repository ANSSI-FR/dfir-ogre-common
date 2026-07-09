# Timeline Borrowed Serialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove avoidable timeline and record clones from JSON and CSV timeline serialization while preserving existing public APIs and output behavior.

**Architecture:** Add crate-private borrowed timeline serialization helpers in `src/timeline.rs` and have the public `TimeLine` methods delegate to them. JSON and CSV formatters will iterate over borrowed timeline entries and pass formatter context instead of mutating cloned `TimeLine` values. CSV quoted field rollback will be centralized in `CsvRowWriter`.

**Tech Stack:** Rust 2024, PyO3-exposed Rust types, existing `JsonObjectWriter` and `CsvRowWriter`, inline Rust unit tests, `cargo fmt`, `cargo test`, `cargo clippy`.

---

## File Structure

- Modify `src/serialise.rs`
  - Add rollback-safe `CsvRowWriter::try_quoted_field`.
  - Add tests for escaping and rollback behavior.
- Modify `src/timeline.rs`
  - Add crate-private `TimelineSerializationContext<'a>`.
  - Add crate-private borrowed JSON and CSV serialization methods.
  - Update public `TimeLine::json_serialise` and `TimeLine::csv_serialise` to delegate without changing signatures.
  - Add tests for formatter-context metadata ID precedence.
- Modify `src/format_json.rs`
  - Replace cloned timeline loops with borrowed iteration.
  - Add JSON formatter regression tests for multi-entry non-mutation and normalized metadata ID precedence.
- Modify `src/format_csv.rs`
  - Replace cloned timeline loops with borrowed iteration.
  - Add CSV formatter regression tests for multi-entry non-mutation and normalized metadata ID precedence.

---

### Task 1: Add Rollback-Safe Quoted CSV Fields

**Files:**
- Modify: `src/serialise.rs`

- [ ] **Step 1: Write failing tests for `CsvRowWriter::try_quoted_field`**

Add these tests in `src/serialise.rs` inside the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn csv_row_writer_try_quoted_field_quotes_and_escapes_generated_content() {
    let mut buffer = String::new();
    let mut row = CsvRowWriter::new(&mut buffer);

    row.quoted_field("first");
    row.try_quoted_field(|field| {
        field.push_str("hello;\"world\"\nnext");
        Ok(())
    })
    .unwrap();
    row.quoted_field("last");

    assert_eq!(buffer, "\"first\";\"hello;\"\"world\"\"\nnext\";\"last\"");
}

#[test]
fn csv_row_writer_try_quoted_field_rolls_back_failed_content() {
    let mut buffer = String::new();
    let result = {
        let mut row = CsvRowWriter::new(&mut buffer);
        row.quoted_field("first");
        let result = row.try_quoted_field(|field| {
            field.push_str("partial;\"payload\"");
            Err(Error::PluginError("boom".to_owned()))
        });
        row.quoted_field("second");
        result
    };

    assert!(matches!(result, Err(Error::PluginError(_))));
    assert_eq!(buffer, "\"first\";\"second\"");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test csv_row_writer_try_quoted_field --locked
```

Expected: compile failure because `CsvRowWriter::try_quoted_field` does not exist.

- [ ] **Step 3: Implement `try_quoted_field`**

Add this method to `impl<'a> CsvRowWriter<'a>` in `src/serialise.rs`, next to `try_raw_field` and `quoted_field`:

```rust
pub(crate) fn try_quoted_field<F>(&mut self, write_value: F) -> Result<(), Error>
where
    F: FnOnce(&mut String) -> Result<(), Error>,
{
    let buffer_len = self.buffer.len();
    let fields = self.fields;
    let mut field_buffer = String::new();
    if let Err(error) = write_value(&mut field_buffer) {
        self.buffer.truncate(buffer_len);
        self.fields = fields;
        return Err(error);
    }
    self.quoted_field(&field_buffer);
    Ok(())
}
```

The closure writes unescaped field content into `field_buffer`. `quoted_field` adds quotes and applies CSV escaping exactly once.

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test csv_row_writer_try_quoted_field --locked
```

Expected: both new tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/serialise.rs
git commit -m "add rollback-safe quoted csv fields"
```

---

### Task 2: Add Borrowed Timeline Serialization Helpers

**Files:**
- Modify: `src/timeline.rs`

- [ ] **Step 1: Write failing tests for formatter-context metadata ID precedence**

Add these tests in `src/timeline.rs` inside the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn timeline_json_context_metadata_id_overrides_owned_metadata_id() {
    let mut tl = timeline_for_serialization_tests();
    tl.metadata_id = Some("owned-md-id".to_owned());

    let mut json = String::new();
    tl.json_serialise_with_context(
        &mut json,
        &DateOutputCodec::IsoUtc(),
        false,
        TimelineSerializationContext {
            metadata_id: Some("formatter-md-id"),
            ..TimelineSerializationContext::default()
        },
    )
    .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["metadata_id"], "formatter-md-id");
}

#[test]
fn timeline_csv_context_metadata_id_overrides_owned_metadata_id() {
    let mut tl = timeline_for_serialization_tests();
    tl.metadata_id = Some("owned-md-id".to_owned());
    tl.data_id = Some("data-id".to_owned());

    let mut csv = String::new();
    tl.csv_serialise_with_context(
        &mut csv,
        &DateOutputCodec::IsoUtc(),
        false,
        true,
        TimelineSerializationContext {
            metadata_id: Some("formatter-md-id"),
            ..TimelineSerializationContext::default()
        },
    )
    .unwrap();

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(CSV_DELIMITER as u8)
        .from_reader(Box::new(csv.as_bytes()));
    let record = reader.records().next().unwrap().unwrap();
    assert_eq!(&record[8], "formatter-md-id");
    assert_eq!(&record[9], "data-id");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test context_metadata_id_overrides_owned_metadata_id --locked
```

Expected: compile failure because `TimelineSerializationContext`, `json_serialise_with_context`, and `csv_serialise_with_context` do not exist.

- [ ] **Step 3: Add the context type and JSON helper**

In `src/timeline.rs`, add this crate-private context type before `impl TimeLine`:

```rust
#[derive(Clone, Copy, Default)]
pub(crate) struct TimelineSerializationContext<'a> {
    pub(crate) metadata: Option<&'a MetadataSerialized>,
    pub(crate) data: Option<&'a Record>,
    pub(crate) metadata_id: Option<&'a str>,
}
```

Replace `TimeLine::json_serialise` with a delegating public method plus this crate-private helper:

```rust
pub fn json_serialise(
    &self,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
) -> Result<(), Error> {
    self.json_serialise_with_context(
        buffer,
        date_codec,
        include_empty,
        TimelineSerializationContext {
            metadata: self.metadata.as_ref(),
            data: self.data.as_ref(),
            metadata_id: self.metadata_id.as_deref(),
        },
    )
}

pub(crate) fn json_serialise_with_context(
    &self,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
    context: TimelineSerializationContext<'_>,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        object.optional_string_field("id", self.id.as_deref());
        object.optional_string_field("computer", self.computer.as_deref());
        object.string_field("timestamp", &serialize_date(&self.timestamp, date_codec));
        object.string_field("timestamp_meaning", &self.timestamp_meaning);
        object.string_field("data_type", &self.data_type);
        object.string_field("related_user", &self.related_user);
        object.string_field("description", &self.description);
        object.string_field("additional_description", &self.additional_description);
        object.optional_string_field("metadata_id", context.metadata_id);
        object.field_if("metadata", context.metadata.is_some(), |buffer| {
            buffer.push_str(context.metadata.unwrap().0.as_str());
        });
        object.optional_string_field("data_id", self.data_id.as_deref());
        if let Some(data) = context.data {
            object.try_field_raw("data", |buffer| {
                data.json_serialise(buffer, date_codec, include_empty)
            })?;
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

- [ ] **Step 4: Add the CSV helper and delegate the public method**

Replace the body of `TimeLine::csv_serialise` with a delegating public method plus this crate-private helper:

```rust
pub fn csv_serialise(
    &self,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
    normalized: bool,
) -> Result<(), Error> {
    self.csv_serialise_with_context(
        buffer,
        date_codec,
        include_empty,
        normalized,
        TimelineSerializationContext {
            metadata: self.metadata.as_ref(),
            data: self.data.as_ref(),
            metadata_id: self.metadata_id.as_deref(),
        },
    )
}

pub(crate) fn csv_serialise_with_context(
    &self,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
    normalized: bool,
    context: TimelineSerializationContext<'_>,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
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
            row.quoted_field(context.metadata_id.unwrap_or_default());
            row.quoted_field(self.data_id.as_deref().unwrap_or_default());
        } else {
            row.quoted_field(context.metadata.map(|m| m.0.as_str()).unwrap_or_default());
            if let Some(data) = context.data {
                row.try_quoted_field(|field| data.json_serialise(field, date_codec, include_empty))?;
            } else {
                row.empty_field();
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

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test context_metadata_id_overrides_owned_metadata_id --locked
cargo test timeline_json_serialise --locked
cargo test timeline_csv_serialise --locked
```

Expected: all focused timeline serialization tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/timeline.rs
git commit -m "add borrowed timeline serializers"
```

---

### Task 3: Update JSON Formatter To Serialize Borrowed Timelines

**Files:**
- Modify: `src/format_json.rs`

- [ ] **Step 1: Import `TimelineSerializationContext`**

Update the imports in `src/format_json.rs` to include:

```rust
use crate::timeline::TimelineSerializationContext;
```

- [ ] **Step 2: Write JSON formatter regression tests**

Add this helper inside `src/format_json.rs` test module:

```rust
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
```

Add these tests inside the same module:

```rust
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

    let timeline_text = fs::read_to_string(&timeline_path).unwrap();
    let first_line = timeline_text.lines().next().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(first_line).unwrap();
    assert_eq!(parsed["metadata_id"], line_builder.metadata_id);

    remove_dir_if_exists(Path::new(output_folder));
}
```

- [ ] **Step 3: Run formatter tests and static clone check before implementation**

Run:

```bash
cargo test timeline_json_write_does_not_mutate_source_timelines --locked
rg -n "data\\.timeline\\.clone\\(|data\\.data\\.clone\\(" src/format_json.rs
```

Expected: the formatter test may pass before implementation because it verifies behavior, not allocation. The `rg` command must print the existing clone sites in `src/format_json.rs`, proving the clone-removal work is still outstanding.

- [ ] **Step 4: Update JSON formatter loops**

In `JsonFormatter::write`, replace the timeline branch with:

```rust
if self.include_timeline {
    let metadata = self.serialized_metadata(&line_builder.metadata);
    for timeline in &data.timeline {
        self.line_buffer.clear();
        timeline.json_serialise_with_context(
            &mut self.line_buffer,
            &self.output_date_codec,
            self.include_empty,
            TimelineSerializationContext {
                metadata: Some(&metadata),
                data: Some(&data.data),
                metadata_id: timeline.metadata_id.as_deref(),
            },
        )?;
        self.line_buffer.push('\n');
        self.data_writer.write(&mut self.line_buffer)?;
        self.file_report.num_lines += 1;
    }
} else {
```

In `JsonFormatter::write_normalized`, replace the timeline loop with:

```rust
for timeline in &data.timeline {
    self.line_buffer.clear();
    timeline.json_serialise_with_context(
        &mut self.line_buffer,
        &self.output_date_codec,
        self.include_empty,
        TimelineSerializationContext {
            metadata: timeline.metadata.as_ref(),
            data: timeline.data.as_ref(),
            metadata_id: Some(&line_builder.metadata_id),
        },
    )?;
    self.line_buffer.push('\n');
    timeline_writer.write(&mut self.line_buffer)?;
    self.file_report.num_lines += 1;
}
```

- [ ] **Step 5: Run focused JSON formatter tests**

Run:

```bash
cargo test timeline_json_write_does_not_mutate_source_timelines --locked
cargo test normalized_json_timeline_uses_formatter_metadata_id --locked
cargo test normalized_file_writes_timeline_next_to_data_file --locked
```

Expected: all focused JSON tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/format_json.rs
git commit -m "serialize json timelines by reference"
```

---

### Task 4: Update CSV Formatter To Serialize Borrowed Timelines

**Files:**
- Modify: `src/format_csv.rs`

- [ ] **Step 1: Import `TimelineSerializationContext`**

Replace the existing timeline import in `src/format_csv.rs`:

```rust
timeline::TimeLine,
```

with:

```rust
timeline::{TimeLine, TimelineSerializationContext},
```

- [ ] **Step 2: Write CSV formatter regression tests**

Add this helper inside `src/format_csv.rs` test module:

```rust
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
```

Add these tests inside the same module:

```rust
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
        assert_eq!(&row[0], "host-csv-timeline");
        assert_eq!(&row[3], "csv_test");
        assert!(row[7].contains("\"computer\":\"host-csv-timeline\""));
        assert!(row[8].contains("\"message\":\"Hello\""));
    }

    remove_if_exists(&path);
}

#[test]
fn normalized_csv_timeline_uses_formatter_metadata_id() {
    std::fs::create_dir_all(TMP_FOLDER).unwrap();
    let base_file_name = "test_csv_timeline_md_id";
    let data_path = Path::new(TMP_FOLDER).join(format!("{base_file_name}.csv"));
    let metadata_path = Path::new(TMP_FOLDER).join("ogre_metadata.csv");
    let timeline_path = Path::new(TMP_FOLDER).join(format!("{base_file_name}.tl.csv"));
    remove_if_exists(&data_path);
    remove_if_exists(&metadata_path);
    remove_if_exists(&timeline_path);

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
        TMP_FOLDER,
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

    remove_if_exists(&data_path);
    remove_if_exists(&metadata_path);
    remove_if_exists(&timeline_path);
}
```

- [ ] **Step 3: Run focused CSV tests and static clone check before implementation**

Run:

```bash
cargo test timeline_csv_write_does_not_mutate_source_timelines --locked
cargo test normalized_csv_timeline_uses_formatter_metadata_id --locked
rg -n "data\\.timeline\\.clone\\(|data\\.data\\.clone\\(" src/format_csv.rs
```

Expected: tests may compile-fail until the formatter imports `TimelineSerializationContext`; after the import but before loop changes, the mutation test may pass and the metadata-ID test should fail if the stale timeline ID is preferred. The `rg` command must print the existing clone sites in `src/format_csv.rs`, proving the clone-removal work is still outstanding.

- [ ] **Step 4: Update CSV formatter loops**

In `CsvFormatter::write`, replace the timeline branch with:

```rust
if self.include_timeline {
    let metadata = self.serialized_metadata(&line_builder.metadata);
    for timeline in &data.timeline {
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
            },
        )?;

        self.line_buffer.push('\n');
        self.data_writer.write(&mut self.line_buffer)?;
        self.file_report.num_lines += 1;
    }
} else {
```

In `CsvFormatter::write_normalized`, replace the timeline loop with:

```rust
for timeline in &data.timeline {
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
        },
    )?;
    self.line_buffer.push('\n');
    timeline_writer.write(&mut self.line_buffer)?;
    self.file_report.num_lines += 1;
}
```

- [ ] **Step 5: Run focused CSV formatter tests**

Run:

```bash
cargo test timeline_csv_write_does_not_mutate_source_timelines --locked
cargo test normalized_csv_timeline_uses_formatter_metadata_id --locked
cargo test normalized_output --locked
```

Expected: all focused CSV tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/format_csv.rs
git commit -m "serialize csv timelines by reference"
```

---

### Task 5: Static Clone Check And Full Verification

**Files:**
- Verify: `src/format_json.rs`
- Verify: `src/format_csv.rs`
- Verify: all modified Rust files

- [ ] **Step 1: Verify targeted clone patterns are gone from formatter timeline paths**

Run:

```bash
rg -n "data\\.timeline\\.clone\\(|data\\.data\\.clone\\(" src/format_json.rs src/format_csv.rs
```

Expected: no output.

- [ ] **Step 2: Format**

Run:

```bash
cargo fmt --check
```

Expected: exit code 0. If formatting fails, run `cargo fmt`, then rerun `cargo fmt --check`.

- [ ] **Step 3: Run full tests**

Run:

```bash
cargo test --locked
```

Expected: all Rust tests pass.

- [ ] **Step 4: Run strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Expected: exit code 0.

- [ ] **Step 5: Commit final verification cleanup if needed**

If formatting or Clippy required code changes:

```bash
git add src/serialise.rs src/timeline.rs src/format_json.rs src/format_csv.rs
git commit -m "polish borrowed timeline serialization"
```

If no files changed, do not create an empty commit.
