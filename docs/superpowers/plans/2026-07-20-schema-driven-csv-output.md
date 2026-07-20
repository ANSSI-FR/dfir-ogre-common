# Schema-Driven CSV Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make regular and normalized CSV data rows serialize only their declared top-level mapping fields so every row remains aligned with its header.

**Architecture:** `CsvFormatter` will derive one optional ordered `record_fields` schema from `FieldMapping` during construction. The same vector will build the standard CSV header and select record values during row serialization; `None` preserves existing headerless pass-through behavior, while JSON and timeline writers continue using the complete shared `LineBuilder` record.

**Tech Stack:** Rust 2024, existing `Record`/`Value` and `CsvRowWriter` types, inline Rust tests, `csv::ReaderBuilder`, and `serde_json` assertions.

## Global Constraints

- Filter only top-level fields from schema-driven regular and normalized CSV data rows.
- Preserve complete nested objects and arrays inside mapped CSV cells.
- Preserve JSONL, normalized JSONL, and timeline behavior.
- Preserve headerless pass-through when `CsvFormatter` receives no `FieldMapping`.
- Do not add dependencies or change filenames, delimiters, escaping, dates, metadata, or report accounting.
- Do not add or run Python-related unit tests.
- Work only on the current non-`main` branch and never push changes.

---

## File Structure

- Modify `src/format_csv.rs`: add regular CSV regression coverage, retain an ordered optional record schema in `CsvFormatter`, and serialize schema-backed rows by key.
- Modify `src/output.rs`: extend the normalized JSON/CSV integration test to prove format-local filtering.

### Task 1: Add Failing CSV Schema Regressions

**Files:**
- Modify: `src/format_csv.rs:411-601`
- Modify: `src/output.rs:528-628`
- Test: inline `#[cfg(test)]` modules in both files

**Interfaces:**
- Consumes: existing `CsvFormatter::new`, `LineBuilder::build`, `Output::write`, `Record`, and strict `csv::ReaderBuilder` parsing.
- Produces: regression tests named `standard_csv_filters_only_top_level_unmapped_fields` and `normalized_outputs_dispatch_data_and_write_metadata` that fail against all-value CSV serialization.

- [ ] **Step 1: Add the regular CSV regression test**

Add this test to `src/format_csv.rs` after the `file` test:

```rust
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
        record.add(
            "unmapped_top_level",
            Value::String("filtered".to_owned()),
        );

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
```

- [ ] **Step 2: Extend the normalized multi-output integration test**

In `src/output.rs`, update `normalized_outputs_dispatch_data_and_write_metadata` immediately after its mapped `event` insertion:

```rust
        record.add("event", Value::String("login".to_owned()));
        record.add(
            "unmapped_top_level",
            Value::String("json-only".to_owned()),
        );
```

Add the JSON preservation assertion after the existing `event` assertion:

```rust
        assert_eq!(json_data["event"], "login");
        assert_eq!(json_data["unmapped_top_level"], "json-only");
```

Add explicit normalized CSV alignment/filtering assertions after reading `csv_record`:

```rust
        let csv_record = csv_reader.records().next().unwrap().unwrap();
        assert_eq!(csv_record.len(), headers.len());
        assert_eq!(&csv_record[0], "login");
        assert!(csv_record[1].len() > 10);
        assert!(csv_record[2].len() > 10);
        assert!(!csv_record.iter().any(|value| value == "json-only"));
```

- [ ] **Step 3: Run both regressions and verify RED**

Run:

```bash
cargo test format_csv::tests::standard_csv_filters_only_top_level_unmapped_fields
cargo test output::tests::normalized_outputs_dispatch_data_and_write_metadata
```

Expected: both tests fail at strict CSV record parsing with `UnequalLengths`; the current rows contain one more cell than their headers because the unmapped top-level value is serialized.

### Task 2: Make CSV Row Serialization Schema-Driven

**Files:**
- Modify: `src/format_csv.rs:55-67`
- Modify: `src/format_csv.rs:78-181`
- Modify: `src/format_csv.rs:196-298`
- Modify: `src/format_csv.rs:328-346`
- Test: `src/format_csv.rs` and `src/output.rs` regressions from Task 1

**Interfaces:**
- Consumes: `FieldMapping::field_parser_tree.output_fields`, `Field::output_name`, `Record::get`, `CsvRowWriter::empty_field`, and generated `ogre_id` stored in `LineData::data`.
- Produces: `CsvFormatter::record_fields: Option<Vec<String>>`; `csv_serialise_record_with_extra_field(record, record_fields, extra_value, buffer, date_codec)`; and `build_standard_header(header, normalized, record_fields)`.

- [ ] **Step 1: Select fields by schema in the record serializer**

Change `csv_serialise_record_with_extra_field` in `src/format_csv.rs` to accept `record_fields` and write missing schema fields as empty cells:

```rust
fn csv_serialise_record_with_extra_field(
    record: &Record,
    record_fields: Option<&[String]>,
    extra_value: CsvExtraField<'_>,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
) -> Result<(), Error> {
    let mut row = CsvRowWriter::new(buffer);
    if let Some(record_fields) = record_fields {
        for field in record_fields {
            if let Some(value) = record.get(field) {
                row.try_raw_field(|buffer| {
                    Value::csv_serialise_value(value, buffer, date_codec)
                })?;
            } else {
                row.empty_field();
            }
        }
    } else {
        for (_, value) in record.iter() {
            row.try_raw_field(|buffer| {
                Value::csv_serialise_value(value, buffer, date_codec)
            })?;
        }
    }
    extra_value.write_to_row(&mut row);
    Ok(())
}
```

- [ ] **Step 2: Derive and retain the ordered formatter schema**

Add this field to `CsvFormatter`:

```rust
    record_fields: Option<Vec<String>>,
```

Replace the header construction in `CsvFormatter::new` with schema derivation shared by the header and future rows:

```rust
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
```

Initialize the new field in the returned formatter:

```rust
            record_fields,
```

- [ ] **Step 3: Use the schema for regular and normalized data rows**

Pass `self.record_fields.as_deref()` in `CsvFormatter::write`:

```rust
            csv_serialise_record_with_extra_field(
                &data.data,
                self.record_fields.as_deref(),
                CsvExtraField::Metadata(&metadata),
                &mut self.line_buffer,
                &self.output_date_codec,
            )?;
```

Pass the same schema in `CsvFormatter::write_normalized`:

```rust
        csv_serialise_record_with_extra_field(
            &data.data,
            self.record_fields.as_deref(),
            CsvExtraField::String(&line_builder.metadata_id),
            &mut self.line_buffer,
            &self.output_date_codec,
        )?;
```

Pass `None` from the existing `serialise_record_with_extra_field_does_not_mutate_record` unit test so it continues to exercise headerless pass-through:

```rust
        csv_serialise_record_with_extra_field(
            &record,
            None,
            CsvExtraField::Metadata(&metadata),
            &mut buffer,
            &DateOutputCodec::Iso(),
        )
```

- [ ] **Step 4: Build standard headers from the same record field list**

Replace `build_standard_header` with:

```rust
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
```

`record_fields` already ends with `ogre_id` for normalized CSV, so this produces `mapped fields;ogre_id;ogre_md_id` without a second identifier-specific branch.

- [ ] **Step 5: Run targeted tests and verify GREEN**

Run:

```bash
cargo test format_csv::tests::standard_csv_filters_only_top_level_unmapped_fields
cargo test output::tests::normalized_outputs_dispatch_data_and_write_metadata
cargo test format_csv::tests::file
cargo test format_csv::tests::normalized_output
```

Expected: all four tests pass. The first two prove filtering and cross-format isolation; `file` proves the `None` schema fallback; `normalized_output` protects identifier ordering and timeline behavior.

### Task 3: Format, Verify, and Commit

**Files:**
- Modify: `src/format_csv.rs`
- Modify: `src/output.rs`
- Include: `docs/superpowers/plans/2026-07-20-schema-driven-csv-output.md`

**Interfaces:**
- Consumes: completed Tasks 1 and 2.
- Produces: formatted, lint-clean, fully tested Rust changes and a local implementation commit.

- [ ] **Step 1: Format the Rust changes**

Run:

```bash
cargo fmt
```

Expected: command exits successfully and only formats the intended Rust files.

- [ ] **Step 2: Run the full Rust test suite**

Run:

```bash
cargo test
```

Expected: every Rust test passes.

- [ ] **Step 3: Verify formatting and lint all targets**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: both commands exit successfully with no warnings or formatting differences.

- [ ] **Step 4: Review the final diff**

Run:

```bash
git diff --check
git diff -- src/format_csv.rs src/output.rs
git status --short
```

Expected: no whitespace errors; the diff is limited to CSV schema selection, regression tests, and this ignored-by-default plan file.

- [ ] **Step 5: Commit locally**

Run:

```bash
git add src/format_csv.rs src/output.rs
git add -f docs/superpowers/plans/2026-07-20-schema-driven-csv-output.md
git commit -m "filter unmapped fields from csv output"
```

Expected: one local implementation commit is created on the current non-`main` branch. Do not push it.
