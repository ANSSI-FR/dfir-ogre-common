# Analytics JSONL Data Envelope Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change `analytics_jsonl` data rows to a fixed technical envelope with the complete parsed record under `data`, while making analytics timeline rows directly joinable through metadata `computer`, metadata `data_type`, and the generated data ID.

**Architecture:** Keep generated IDs in `LineData.data_id` and project them only at output-writer boundaries, so the mapped `Record` remains artifact data. Serialize analytics rows directly into a five-field JSON object without cloning or filtering the nested record; extend the borrowed timeline context for output-only overrides. Reject undated analytics configuration during run validation and document direct ClickHouse `JSONEachRow` tables backed by `ReplacingMergeTree`.

**Tech Stack:** Rust 2024, PyO3, `serde_json`, `indexmap`, ClickHouse SQL documentation, inline Rust tests, Cargo

## Global Constraints

- Work only on the existing non-`main` branch `feature/analytics-jsonl-data-envelope`; never implement on `main`.
- Never push this branch or any commit to a remote repository.
- Keep all public Rust and Python type/function signatures unchanged.
- Do not change parser behavior, field mapping, or any data-, metadata-, or timeline-ID hash calculation.
- The analytics envelope has exactly `ogre_id`, `computer`, `data_type`, `ogre_md_id`, and `data`; JSON field order is not contractual.
- Serialize `LineData.data` wholesale under analytics `data`; do not filter or overwrite artifact fields named `ogre_id`, `computer`, `data_type`, `ogre_md_id`, or `data`.
- Source analytics envelope and timeline `computer`/`data_type` from the same `Metadata` instance.
- Keep analytics timeline `data_id` equal to its source envelope `ogre_id`.
- Reject `timeline_include_undated: true` for `analytics_jsonl`, even when `with_timeline` is false, before creating output paths.
- Preserve existing `jsonl`, `normalized_jsonl`, `csv`, and `normalized_csv` serialized behavior, including mixed-format runs that request ID computation.
- Keep the metadata sidecar shape, filenames, append mode, gzip behavior, date codecs, `include_empty`, reports, and close behavior unchanged.
- Add no dependencies or fixtures. Do not add or run Python tests.
- Use path-specific staging; `docs/` is ignored globally, while previously tracked documentation remains trackable.

---

## File Structure

- Modify `src/config_parser.rs`: reject the incompatible analytics option in preflight validation and test strict/non-analytics cases.
- Modify `dfir_ogre_common.pyi`: document the analytics restriction without changing the constructor signature.
- Modify `src/record.rs`: provide crate-private JSON field serialization with one string override, used to preserve generated-ID projection without mutating records.
- Modify `src/line_builder.rs`: stop inserting the generated `ogre_id` into `LineData.data`; retain `LineData.data_id` and timeline linkage.
- Modify `src/format_json.rs`: preserve existing ID projection, write the analytics envelope, and pass analytics timeline overrides.
- Modify `src/format_csv.rs`: preserve normalized and mixed standard CSV ID behavior from `LineData.data_id`.
- Modify `src/timeline.rs`: carry a nested-data ID override plus analytics `data_type` override in borrowed serialization context.
- Modify `src/output.rs`: add end-to-end validation, compatibility, envelope, join, and mixed-undated tests.
- Modify `src/parser/evtx.rs`: update the fixture-backed analytics test to inspect `row["data"]` while preserving `.tmp/analytics` output.
- Modify `docs/analytics-jsonl-clickhouse-data-model.md`: replace the flat model with the approved envelope, table engines, ordering keys, joins, and views.
- Do not create new runtime modules, dependencies, or test fixtures.

### Task 1: Reject Undated Analytics Configuration During Preflight

**Files:**
- Modify: `src/config_parser.rs:101-188`
- Modify: `dfir_ogre_common.pyi:588-606`
- Test: `src/config_parser.rs:235-310`
- Test: `src/output.rs:985-1045`

**Interfaces:**
- Consumes: `RunConfiguration::validate(&self) -> Result<(), Error>` and the existing string-valued `OutputConfiguration.format`.
- Produces: `Error::ConfigurationError("'timeline_include_undated' is not supported with 'analytics_jsonl'")` for every analytics output whose flag is true.
- Preserves: the `OutputConfiguration` Rust/Python constructors and all non-analytics validation behavior.

- [ ] **Step 1: Add failing strict-validation tests**

Add these tests to the existing `src/config_parser.rs` test module:

```rust
#[test]
fn analytics_jsonl_rejects_timeline_include_undated_with_or_without_timeline() {
    for with_timeline in [false, true] {
        let mut output = OutputConfiguration::new(
            "events".to_owned(),
            ".tmp/analytics-invalid".to_owned(),
            "file".to_owned(),
            "analytics_jsonl".to_owned(),
            "iso".to_owned(),
            with_timeline,
            true,
            HashMap::new(),
        );
        output.timeline_include_undated = true;

        let error = RunConfiguration::new(vec![output], true, None)
            .validate()
            .expect_err("analytics must reject undated timeline output");

        assert_eq!(
            error.to_string(),
            "'timeline_include_undated' is not supported with 'analytics_jsonl'"
        );
    }
}

#[test]
fn normalized_jsonl_still_accepts_timeline_include_undated() {
    let mut output = OutputConfiguration::new(
        "events".to_owned(),
        ".tmp/normalized-undated".to_owned(),
        "file".to_owned(),
        "normalized_jsonl".to_owned(),
        "iso".to_owned(),
        true,
        true,
        HashMap::new(),
    );
    output.timeline_include_undated = true;

    assert!(RunConfiguration::new(vec![output], true, None).validate().is_ok());
}
```

Add an `src/output.rs` integration test that proves validation precedes filesystem work:

```rust
#[test]
fn invalid_analytics_undated_option_creates_no_output_folder() {
    let output_folder = Path::new(".tmp").join("output_invalid_analytics_undated");
    remove_dir_if_exists(&output_folder);
    let mut output_conf = normalized_output_config(
        "invalid_analytics_undated",
        &output_folder,
        "analytics_jsonl",
    );
    output_conf.timeline_include_undated = true;

    let result = Output::new(
        RunConfiguration::new(vec![output_conf], true, None),
        mapped_plugin_config("events", event_field_mapping()),
        Metadata::new("host-output".into()),
        None,
    );

    let error = match result {
        Ok(_) => panic!("invalid analytics configuration was accepted"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::ConfigurationError(_)));
    assert!(!output_folder.exists());
}
```

- [ ] **Step 2: Run the validation tests and verify they fail**

Run:

```bash
cargo test --locked analytics_jsonl_rejects_timeline_include_undated
cargo test --locked invalid_analytics_undated_option_creates_no_output_folder
```

Expected: both commands FAIL because `RunConfiguration::validate` currently checks only that at least one output exists; the integration test also observes that `Output::new` creates the requested folder.

- [ ] **Step 3: Implement preflight validation and update API documentation**

Add this check after the existing empty-output check in `RunConfiguration::validate`:

```rust
if self.output.iter().any(|output| {
    output.format == "analytics_jsonl" && output.timeline_include_undated
}) {
    return Err(Error::ConfigurationError(
        "'timeline_include_undated' is not supported with 'analytics_jsonl'".to_owned(),
    ));
}
```

Update the Rust doc comment above `OutputConfiguration::py_new` and the matching `dfir_ogre_common.pyi` constructor documentation to say:

```text
timeline_include_undated emits a null-timestamp placeholder when timeline extraction
finds no valid date. It is ignored unless with_timeline is enabled. analytics_jsonl
rejects this option when it is true.
```

Do not change the constructor arguments, defaults, annotations, or generated Python surface.

- [ ] **Step 4: Run validation regressions**

Run:

```bash
cargo test --locked analytics_jsonl_rejects_timeline_include_undated
cargo test --locked normalized_jsonl_still_accepts_timeline_include_undated
cargo test --locked invalid_analytics_undated_option_creates_no_output_folder
cargo test --locked timeline_include_undated_is_applied_per_output
```

Expected: all commands PASS, and the existing non-analytics per-output test remains unchanged.

- [ ] **Step 5: Format and commit the validation change**

Run:

```bash
cargo fmt
cargo fmt --check
git add src/config_parser.rs src/output.rs dfir_ogre_common.pyi
git commit -m "Reject undated analytics timelines"
```

Expected: formatting passes and the commit contains only validation, its tests, and the documentation-only type-stub update.

### Task 2: Project Generated IDs at Writer Boundaries

**Files:**
- Modify: `src/record.rs:1-88`
- Modify: `src/timeline.rs:56-205`
- Modify: `src/format_json.rs:3-74,271-382`
- Modify: `src/format_csv.rs:1-72,215-321`
- Test: `src/record.rs:120-450`
- Test: `src/timeline.rs:1460-1580`
- Test: `src/format_json.rs:545-705`
- Test: `src/format_csv.rs:430-1040`
- Test: `src/output.rs:540-650`

**Interfaces:**
- Produces: `Record::json_serialise_fields_with_string_override(&self, object: &mut JsonObjectWriter<'_>, date_codec: &DateOutputCodec, include_empty: bool, string_override: Option<(&str, &str)>) -> Result<(), Error>`.
- Produces: `Record::json_serialise_with_string_override(&self, buffer: &mut String, date_codec: &DateOutputCodec, include_empty: bool, string_override: Option<(&str, &str)>) -> Result<(), Error>`.
- Extends: `TimelineSerializationContext<'a>` with `data_id_override: Option<&'a str>` for the generated ID inside non-normalized timeline `data` objects.
- Changes private helpers: `json_serialise_record_with_extra_field` and `csv_serialise_record_with_extra_field` accept `data_id_override: Option<&str>`.
- Preserves: source-key position when replacing an existing root `ogre_id`, appends the generated field when absent, and never mutates the `Record`.

- [ ] **Step 1: Add failing JSON override tests**

Add this test in `src/record.rs`:

```rust
#[test]
fn json_string_override_replaces_only_the_root_field_without_mutation() {
    let mut nested = Record::new();
    nested.add("ogre_id", Value::String("nested-source".to_owned()));
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    record.add("ogre_id", Value::String("root-source".to_owned()));
    record.add("nested", Value::Object(nested));
    let original = record.clone();
    let mut buffer = String::new();

    record
        .json_serialise_with_string_override(
            &mut buffer,
            &DateOutputCodec::Iso(),
            false,
            Some(("ogre_id", "generated-id")),
        )
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
    assert_eq!(parsed["ogre_id"], "generated-id");
    assert_eq!(parsed["nested"]["ogre_id"], "nested-source");
    assert_eq!(record, original);
}
```

Add a timeline-context test in `src/timeline.rs`:

```rust
#[test]
fn timeline_json_context_projects_generated_id_into_nested_data() {
    let mut tl = timeline_for_serialization_tests();
    tl.data.as_mut().unwrap().add(
        "ogre_id",
        Value::String("artifact-id".to_owned()),
    );
    let original = tl.data.clone();
    let mut json = String::new();

    tl.json_serialise_with_context(
        &mut json,
        &DateOutputCodec::IsoUtc(),
        false,
        TimelineSerializationContext {
            data: tl.data.as_ref(),
            data_id_override: Some("generated-id"),
            ..TimelineSerializationContext::default()
        },
    )
    .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["data"]["ogre_id"], "generated-id");
    assert_eq!(tl.data, original);
}
```

- [ ] **Step 2: Add failing writer-boundary tests**

Add a JSON helper test in `src/format_json.rs` that passes a record with no generated ID:

```rust
#[test]
fn record_extra_field_serializer_projects_generated_data_id() {
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    let mut buffer = String::new();

    json_serialise_record_with_extra_field(
        &record,
        Some("generated-id"),
        OUTPUT_METADATA_ID_FIELD,
        JsonExtraField::String("metadata-id"),
        &mut buffer,
        &DateOutputCodec::Iso(),
        false,
    )
    .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
    assert_eq!(parsed["event"], "login");
    assert_eq!(parsed["ogre_id"], "generated-id");
    assert_eq!(parsed["ogre_md_id"], "metadata-id");
}
```

Add a CSV helper test in `src/format_csv.rs`:

```rust
#[test]
fn csv_record_serializer_projects_generated_data_id_column() {
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    let fields = vec!["event".to_owned(), OUPUT_DATA_ID.to_owned()];
    let mut buffer = String::new();

    csv_serialise_record_with_extra_field(
        &record,
        Some(&fields),
        Some("generated-id"),
        CsvExtraField::String("metadata-id"),
        &mut buffer,
        &DateOutputCodec::Iso(),
    )
    .unwrap();

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(CSV_DELIMITER as u8)
        .from_reader(buffer.as_bytes());
    let row = reader.records().next().unwrap().unwrap();
    assert_eq!(&row[0], "login");
    assert_eq!(&row[1], "generated-id");
    assert_eq!(&row[2], "metadata-id");
}
```

- [ ] **Step 3: Run the new override tests and verify they fail to compile**

Run:

```bash
cargo test --locked json_string_override_replaces_only_the_root_field
cargo test --locked record_extra_field_serializer_projects_generated_data_id
cargo test --locked csv_record_serializer_projects_generated_data_id_column
```

Expected: compilation FAILS because the new `Record` methods, context field, and private helper parameters do not exist yet.

- [ ] **Step 4: Implement reusable non-mutating JSON field projection**

Import `JsonObjectWriter` in `src/record.rs` and add these crate-private methods to the non-Python `impl Record` block:

```rust
pub(crate) fn json_serialise_fields_with_string_override(
    &self,
    object: &mut JsonObjectWriter<'_>,
    date_codec: &DateOutputCodec,
    include_empty: bool,
    string_override: Option<(&str, &str)>,
) -> Result<(), Error> {
    let mut override_written = false;
    for (key, value) in self.iter() {
        if let Some((override_key, override_value)) = string_override
            && key.as_str() == override_key
        {
            object.string_field(key, override_value);
            override_written = true;
            continue;
        }
        if include_empty || !matches!(value, Value::Null()) {
            object.try_field_raw(key, |buffer| {
                Value::json_serialise_value(value, buffer, date_codec, include_empty)
            })?;
        }
    }
    if let Some((override_key, override_value)) = string_override
        && !override_written
    {
        object.string_field(override_key, override_value);
    }
    Ok(())
}

pub(crate) fn json_serialise_with_string_override(
    &self,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
    string_override: Option<(&str, &str)>,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        self.json_serialise_fields_with_string_override(
            &mut object,
            date_codec,
            include_empty,
            string_override,
        )?;
        object.finish();
        Ok(())
    })();
    if result.is_err() {
        buffer.truncate(start_len);
    }
    result
}
```

Keep the existing public `Record::json_serialise` method unchanged.

- [ ] **Step 5: Thread the generated ID through JSON and timeline contexts**

Import `OUPUT_DATA_ID` in both `src/format_json.rs` and `src/timeline.rs`. Add `data_id_override: Option<&'a str>` to `TimelineSerializationContext` and initialize it to `None` in `TimeLine::json_serialise`, `TimeLine::csv_serialise`, normalized timeline writers, and existing explicit test contexts.

In both JSON and CSV standard timeline writers, pass:

```rust
data_id_override: data.data_id.as_deref(),
```

When serializing a non-normalized timeline's nested `data` in both `TimeLine::json_serialise_with_context` and `TimeLine::csv_serialise_with_context`, call:

```rust
data.json_serialise_with_string_override(
    buffer,
    date_codec,
    include_empty,
    context
        .data_id_override
        .map(|data_id| (OUPUT_DATA_ID, data_id)),
)
```

For the CSV quoted-field closure, use its local `field` buffer in place of `buffer`.

Change `json_serialise_record_with_extra_field` to accept `data_id_override: Option<&str>` immediately after `record`. Replace its record loop with:

```rust
record.json_serialise_fields_with_string_override(
    &mut object,
    date_codec,
    include_empty,
    data_id_override.map(|data_id| (OUPUT_DATA_ID, data_id)),
)?;
```

Pass `data.data_id.as_deref()` from both `JsonFormatter::write` and the non-analytics branch of `JsonFormatter::write_normalized`.

Update every existing direct unit-test call to `json_serialise_record_with_extra_field` with `None` when it is testing behavior unrelated to generated IDs.

- [ ] **Step 6: Thread the generated ID through CSV record output**

Add `data_id_override: Option<&str>` after `record_fields` in `csv_serialise_record_with_extra_field`. For declared fields, emit the generated value when the field name is `OUPUT_DATA_ID`; otherwise retain the current mapped-field logic:

```rust
for field in record_fields {
    if field.as_str() == OUPUT_DATA_ID
        && let Some(data_id) = data_id_override
    {
        row.quoted_field(data_id);
    } else if let Some(value) = record.get(field) {
        row.try_raw_field(|buffer| Value::csv_serialise_value(value, buffer, date_codec))?;
    } else {
        row.empty_field();
    }
}
```

For undeclared fields, replace an existing `ogre_id` in place and append the generated ID only when no source key occupied that position. This preserves the current unheaded CSV column order:

```rust
let mut data_id_written = false;
for (key, value) in record.iter() {
    if key.as_str() == OUPUT_DATA_ID
        && let Some(data_id) = data_id_override
    {
        row.quoted_field(data_id);
        data_id_written = true;
    } else {
        row.try_raw_field(|buffer| Value::csv_serialise_value(value, buffer, date_codec))?;
    }
}
if let Some(data_id) = data_id_override
    && !data_id_written
{
    row.quoted_field(data_id);
}
```

Pass `data.data_id.as_deref()` from both `CsvFormatter::write` and `CsvFormatter::write_normalized`.

Update every existing direct unit-test call to `csv_serialise_record_with_extra_field` with `None` when it is testing behavior unrelated to generated IDs.

- [ ] **Step 7: Add and run a mixed-format compatibility test**

Add `mixed_formats_preserve_existing_id_projection` in `src/output.rs`. Configure four distinct base names in one `Output`: `jsonl`, `csv`, `normalized_jsonl`, and `normalized_csv`. Write one mapped `event` record, then assert:

```rust
assert_eq!(standard_json["event"], "login");
assert_eq!(standard_json["ogre_id"], normalized_json["ogre_id"]);
assert!(standard_json.get("ogre_md").is_some());
assert!(normalized_json["ogre_md_id"].as_str().unwrap().len() > 10);

assert_eq!(standard_csv_headers.iter().collect::<Vec<_>>(), vec!["event", "ogre_md"]);
assert_eq!(
    normalized_csv_headers.iter().collect::<Vec<_>>(),
    vec!["event", "ogre_id", "ogre_md_id"]
);
assert_eq!(
    &normalized_csv_row[1],
    normalized_json["ogre_id"].as_str().unwrap()
);
```

Use the existing `OutputConfiguration::new`, `mapped_plugin_config`, `event_field_mapping`, CSV reader, and cleanup helpers. This test passes before the builder mutation is removed but protects behavior in Task 3.

Run:

```bash
cargo test --locked generated_data_id
cargo test --locked mixed_formats_preserve_existing_id_projection
cargo test --locked format_json::tests
cargo test --locked format_csv::tests
```

Expected: all commands PASS, with no duplicate `ogre_id` fields or columns.

- [ ] **Step 8: Format and commit writer-boundary projection**

Run:

```bash
cargo fmt
cargo fmt --check
git add src/record.rs src/timeline.rs src/format_json.rs src/format_csv.rs src/output.rs
git commit -m "Preserve generated IDs at writer boundaries"
```

Expected: the commit introduces the projection infrastructure while serialized output is still compatible with the pre-change builder.

### Task 3: Move Analytics Artifact Data into the Envelope

**Files:**
- Modify: `src/line_builder.rs:13-16,289-319`
- Modify: `src/format_json.rs:44-109,330-382`
- Test: `src/line_builder.rs:1480-1555`
- Test: `src/format_json.rs:575-660,930-1110`
- Test: `src/output.rs:651-705,891-982`
- Test: `src/parser/evtx.rs:333-416`

**Interfaces:**
- Consumes: writer-boundary ID projection from Task 2.
- Produces: `json_serialise_analytics_record(record: &Record, data_id: &str, metadata: &Metadata, metadata_id: &str, buffer: &mut String, date_codec: &DateOutputCodec, include_empty: bool) -> Result<(), Error>`.
- Produces: analytics rows with exactly five root fields and an unfiltered artifact object under `data`.
- Preserves: `LineData.data_id`, `TimeLine.data_id`, ID hash inputs/results, and existing non-analytics schemas.

- [ ] **Step 1: Replace flat analytics unit expectations with failing envelope expectations**

Replace `analytics_record_replaces_top_level_dimensions_and_preserves_nested_values` in `src/format_json.rs` with a test that puts every envelope name in the source record:

```rust
#[test]
fn analytics_record_wraps_the_complete_unfiltered_record() {
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    record.add("ogre_id", Value::String("artifact-id".to_owned()));
    record.add("computer", Value::String("event-host".to_owned()));
    record.add("data_type", Value::String("artifact-type".to_owned()));
    record.add("ogre_md_id", Value::String("artifact-md-id".to_owned()));
    record.add("data", Value::String("artifact-data-field".to_owned()));
    let original = record.clone();
    let mut metadata = Metadata::new("metadata\"host\n".to_owned());
    metadata.data_type = "events\\kind".to_owned();
    let mut buffer = String::new();

    json_serialise_analytics_record(
        &record,
        "generated-id",
        &metadata,
        "metadata-id",
        &mut buffer,
        &DateOutputCodec::Iso(),
        false,
    )
    .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
    assert_eq!(parsed.as_object().unwrap().len(), 5);
    assert_eq!(parsed["ogre_id"], "generated-id");
    assert_eq!(parsed["computer"], "metadata\"host\n");
    assert_eq!(parsed["data_type"], "events\\kind");
    assert_eq!(parsed["ogre_md_id"], "metadata-id");
    assert_eq!(parsed["data"]["event"], "login");
    assert_eq!(parsed["data"]["ogre_id"], "artifact-id");
    assert_eq!(parsed["data"]["computer"], "event-host");
    assert_eq!(parsed["data"]["data_type"], "artifact-type");
    assert_eq!(parsed["data"]["ogre_md_id"], "artifact-md-id");
    assert_eq!(parsed["data"]["data"], "artifact-data-field");
    assert_eq!(record, original);
}
```

Change the empty-field test to parse the result and assert an always-present empty `data` object:

```rust
assert_eq!(parsed["ogre_id"], "generated-id");
assert_eq!(parsed["computer"], "");
assert_eq!(parsed["data_type"], "");
assert_eq!(parsed["ogre_md_id"], "metadata-id");
assert_eq!(parsed["data"], serde_json::json!({}));
```

- [ ] **Step 2: Add a failing builder ownership/linkage test**

Add this behavior test to `src/line_builder.rs`, using a timeline mapping with one parsed date:

```rust
#[test]
fn generated_data_id_stays_outside_record_and_links_timelines() {
    let timeline_builder = TimeLineBuilder::new(
        TimeLineType::Standard,
        "timeline-type".to_owned(),
        100,
        None,
        None,
    );
    let field_mapping = FieldMapping::new(
        vec![Field::Single {
            name: FieldName::new("when".into(), false, None, None, None),
            parser: Parser::DateTime(DateInputCodec::Iso()),
            default_value: None,
        }],
        None,
    );
    let mut builder = LineBuilder::new(
        Metadata::new("host".into()),
        Some(timeline_builder),
        field_mapping,
        true,
        false,
        true,
    );
    let mut record = Record::new();
    record.add("ogre_id", Value::String("artifact-id".to_owned()));
    record.add(
        "when",
        Value::Date(
            crate::parse_date("2026-08-06T12:00:00Z", &DateInputCodec::Iso()).unwrap(),
        ),
    );

    builder.build(&mut record).unwrap();

    let generated = builder.line_data.data_id.as_deref().unwrap();
    assert_ne!(generated, "artifact-id");
    assert_eq!(
        builder.line_data.data.get("ogre_id"),
        Some(&Value::String("artifact-id".to_owned()))
    );
    assert_eq!(builder.line_data.timeline.len(), 1);
    assert_eq!(
        builder.line_data.timeline[0].data_id.as_deref(),
        Some(generated)
    );
}
```

In the same test, clear and rebuild from a record without an artifact `ogre_id`, then assert `!builder.line_data.data.contains_key(OUPUT_DATA_ID)` while `builder.line_data.data_id.is_some()`.

- [ ] **Step 3: Run the envelope and ownership tests and verify they fail**

Run:

```bash
cargo test --locked analytics_record_wraps_the_complete_unfiltered_record
cargo test --locked generated_data_id_stays_outside_record_and_links_timelines
```

Expected: FAIL because analytics is still flat and `LineBuilder` still overwrites/inserts `ogre_id` in the parsed record.

- [ ] **Step 4: Stop mutating the parsed record**

In `LineBuilder::build`, retain:

```rust
self.line_data.data_id = Some(data_id.clone());

let mut hasher = Hasher::new();
for timeline in &mut self.line_data.timeline {
    timeline.data_id = Some(data_id.clone());
    timeline.metadata_id = Some(self.metadata_id.clone());
    timeline.computer = Some(self.metadata.computer.clone());
    timeline.compute_id(&mut hasher);
    hasher.reset();
}
```

Delete only this mutation:

```rust
self.line_data
    .data
    .add(OUPUT_DATA_ID, Value::String(data_id.clone()));
```

Keep `OUPUT_DATA_ID` for the formatter modules; do not change hash construction or timeline ID computation.

- [ ] **Step 5: Implement the streaming analytics envelope**

Add `ANALYTICS_DATA_FIELD` and import `OUPUT_DATA_ID` in `src/format_json.rs`:

```rust
const ANALYTICS_COMPUTER_FIELD: &str = "computer";
const ANALYTICS_DATA_TYPE_FIELD: &str = "data_type";
const ANALYTICS_DATA_FIELD: &str = "data";
```

Replace the flat/filtering serializer body with:

```rust
fn json_serialise_analytics_record(
    record: &Record,
    data_id: &str,
    metadata: &Metadata,
    metadata_id: &str,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        object.string_field(OUPUT_DATA_ID, data_id);
        object.string_field(ANALYTICS_COMPUTER_FIELD, &metadata.computer);
        object.string_field(ANALYTICS_DATA_TYPE_FIELD, &metadata.data_type);
        object.string_field(OUTPUT_METADATA_ID_FIELD, metadata_id);
        object.try_field_raw(ANALYTICS_DATA_FIELD, |buffer| {
            record.json_serialise(buffer, date_codec, include_empty)
        })?;
        object.finish();
        Ok(())
    })();
    if result.is_err() {
        buffer.truncate(start_len);
    }
    result
}
```

In the analytics branch of `JsonFormatter::write_normalized`, obtain the generated ID without mutating the record:

```rust
let data_id = data.data_id.as_deref().ok_or_else(|| {
    Error::PluginError("analytics_jsonl requires a generated data id".to_owned())
})?;
json_serialise_analytics_record(
    &data.data,
    data_id,
    &line_builder.metadata,
    &line_builder.metadata_id,
    &mut self.line_buffer,
    &self.output_date_codec,
    self.include_empty,
)?;
```

Do not call `json_serialise_with_string_override` for the nested analytics `data` object.

- [ ] **Step 6: Update formatter, output, gzip, and EVTX assertions**

Update analytics tests to read parsed fields beneath `data`:

```rust
assert_eq!(data["computer"], "metadata-host");
assert_eq!(data["data_type"], "events");
assert_eq!(data["data"]["event"], "login");
assert_eq!(
    data["data"]["when"],
    "2026-08-06T12:00:00.000000+00:00"
);
assert!(data.get("event").is_none());
```

In the gzip test, assert `data["data"]["event"] == "login"` in addition to the four envelope strings. In `analytics_jsonl_uses_normalized_lifecycle`, assert the root object has five fields and the event is nested.

Update `evtx_analytics_jsonl_writes_joinable_rows` without changing its output directory or fixture:

```rust
assert_eq!(data_row["computer"].as_str(), Some("analytics-test-host"));
assert_eq!(data_row["data_type"].as_str(), Some("evtx"));
assert_eq!(
    data_row["data"]["system"]["computer"].as_str(),
    Some("MINWINPC")
);
assert!(data_row.get("system").is_none());
```

Continue reading `ogre_id` and `ogre_md_id` from the root and leave generated files in `.tmp/analytics` as the test currently does.

- [ ] **Step 7: Run core and compatibility tests**

Run:

```bash
cargo test --locked generated_data_id_stays_outside_record_and_links_timelines
cargo test --locked analytics_record
cargo test --locked analytics_jsonl
cargo test --locked normalized_output
cargo test --locked normalized_file_writes_metadata_and_data_records
cargo test --locked mixed_formats_preserve_existing_id_projection
cargo test --locked evtx_analytics_jsonl_writes_joinable_rows
```

Expected: all commands PASS. The EVTX test writes 123 data rows and 123 timeline rows, and normalized JSONL/CSV still expose generated IDs outside their parsed records.

- [ ] **Step 8: Format and commit the envelope change**

Run:

```bash
cargo fmt
cargo fmt --check
git add src/line_builder.rs src/format_json.rs src/output.rs src/parser/evtx.rs
git commit -m "Wrap analytics records in data envelope"
```

Expected: the commit changes the analytics data shape and internal ID ownership without changing timeline dimensions yet.

Keep the existing `"timeline-events"` assertion in the end-to-end analytics test through this task; Task 4 changes that output dimension after adding its dedicated override tests.

### Task 4: Align Analytics Timeline Dimensions and Enforce Join Invariants

**Files:**
- Modify: `src/timeline.rs:56-120`
- Modify: `src/format_json.rs:330-360`
- Test: `src/timeline.rs:1500-1545`
- Test: `src/format_json.rs:930-1010`
- Test: `src/output.rs:891-1045`
- Test: `src/parser/evtx.rs:386-416`

**Interfaces:**
- Consumes: `TimelineSerializationContext<'a>.data_id_override` from Task 2.
- Produces: `TimelineSerializationContext<'a>.data_type_override: Option<&'a str>`.
- Produces: analytics JSON timeline `computer` and `data_type` from metadata; all other JSON modes and CSV continue using stored timeline values.
- Preserves: stored `TimeLine`, timeline-ID hashing, `metadata_id`, and `data_id`.

- [ ] **Step 1: Add a failing data-type override/non-mutation unit test**

Add this test in `src/timeline.rs`:

```rust
#[test]
fn timeline_json_context_data_type_override_wins_without_mutation() {
    let tl = timeline_for_serialization_tests();
    let stored_data_type = tl.data_type.clone();
    let mut json = String::new();

    tl.json_serialise_with_context(
        &mut json,
        &DateOutputCodec::IsoUtc(),
        false,
        TimelineSerializationContext {
            data_type_override: Some("metadata-events"),
            ..TimelineSerializationContext::default()
        },
    )
    .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["data_type"], "metadata-events");
    assert_eq!(tl.data_type, stored_data_type);
}
```

Extend `analytics_timeline_writer_overrides_stale_computer_from_metadata` so the stored timeline has `data_type == "timeline-events"`, the emitted JSON must have `data_type == "events"`, and the stored value remains unchanged.

- [ ] **Step 2: Change the end-to-end analytics join assertions so they fail**

Rename `analytics_jsonl_enriches_main_and_timeline_rows` to `analytics_jsonl_writes_enveloped_joinable_rows` and replace the timeline classification expectation with the approved tuple:

```rust
assert_eq!(data["data"]["event"], "login");
assert_eq!(timeline["computer"], data["computer"]);
assert_eq!(timeline["data_type"], data["data_type"]);
assert_eq!(timeline["data_id"], data["ogre_id"]);
assert_eq!(timeline["metadata_id"], data["ogre_md_id"]);
assert_eq!(timeline["data_type"], "events");
```

The timeline builder in this test must remain configured with `"timeline-events"`, so the test proves that the metadata override is effective.

Run:

```bash
cargo test --locked timeline_json_context_data_type_override_wins_without_mutation
cargo test --locked analytics_jsonl_writes_enveloped_joinable_rows
```

Expected: compilation/test FAIL because the context has no data-type override and the analytics timeline still serializes `"timeline-events"`.

- [ ] **Step 3: Implement the analytics-only timeline data-type override**

Add this field to the context:

```rust
pub(crate) data_type_override: Option<&'a str>,
```

In timeline JSON serialization, replace the current data-type write with:

```rust
object.string_field(
    "data_type",
    context
        .data_type_override
        .unwrap_or(self.data_type.as_str()),
);
```

Set `data_type_override: None` in direct timeline serialization and non-analytics writer contexts. Do not consult this field in CSV serialization.

In the normalized JSON timeline writer, pass both analytics overrides:

```rust
computer_override: match self.mode {
    JsonMode::Analytics => Some(line_builder.metadata.computer.as_str()),
    JsonMode::Standard | JsonMode::Normalized => None,
},
data_type_override: match self.mode {
    JsonMode::Analytics => Some(line_builder.metadata.data_type.as_str()),
    JsonMode::Standard | JsonMode::Normalized => None,
},
```

Do not mutate `timeline.data_type` and do not recompute `timeline.id`.

- [ ] **Step 4: Test mixed undated construction and analytics filtering**

Add `mixed_undated_construction_does_not_emit_analytics_timeline` in `src/output.rs` with the same no-date timeline mapping used by `timeline_include_undated_is_applied_per_output`. Configure:

```rust
let mut keep = json_output_config("standard_undated", &output_folder, HashMap::new());
keep.with_timeline = true;
keep.timeline_include_undated = true;

let mut analytics = normalized_output_config(
    "analytics_dated_only",
    &output_folder,
    "analytics_jsonl",
);
analytics.with_timeline = true;
```

Write an event without a timestamp, then assert:

```rust
let standard = fs::read_to_string(
    output_folder.join("standard_undated.events.jsonl")
)
.unwrap();
let analytics_timeline = fs::read_to_string(
    output_folder.join("analytics_dated_only.events.tl.jsonl")
)
.unwrap();
assert_eq!(standard.lines().count(), 1);
assert!(
    serde_json::from_str::<serde_json::Value>(standard.trim()).unwrap()["timestamp"]
        .is_null()
);
assert!(analytics_timeline.is_empty());
```

This proves an allowed non-analytics option can cause shared placeholder construction without leaking null timestamps into analytics output.

- [ ] **Step 5: Strengthen the EVTX join check and run timeline tests**

In `evtx_analytics_jsonl_writes_joinable_rows`, retain the metadata-ID assertions and add:

```rust
assert_eq!(timeline_row["data_id"], data_row["ogre_id"]);
assert_eq!(timeline_row["computer"], data_row["computer"]);
assert_eq!(timeline_row["data_type"], data_row["data_type"]);
assert!(!timeline_row["timestamp"].is_null());
```

Run:

```bash
cargo test --locked timeline_json_context_data_type_override_wins_without_mutation
cargo test --locked analytics_timeline_writer_overrides
cargo test --locked analytics_jsonl_writes_enveloped_joinable_rows
cargo test --locked mixed_undated_construction_does_not_emit_analytics_timeline
cargo test --locked evtx_analytics_jsonl_writes_joinable_rows
cargo test --locked timeline::tests
```

Expected: all commands PASS. The analytics join tuple matches on every asserted dimension, while normalized JSON and all CSV timeline classifications remain unchanged.

- [ ] **Step 6: Format and commit timeline alignment**

Run:

```bash
cargo fmt
cargo fmt --check
git add src/timeline.rs src/format_json.rs src/output.rs src/parser/evtx.rs
git commit -m "Align analytics timeline dimensions"
```

Expected: the commit contains only the analytics JSON data-type projection, join tests, and mixed-undated filtering coverage.

### Task 5: Rewrite the ClickHouse Data-Model Documentation

**Files:**
- Modify: `docs/analytics-jsonl-clickhouse-data-model.md:1-245`

**Interfaces:**
- Consumes: the five-field analytics envelope and timeline guarantees implemented in Tasks 1-4.
- Produces: a directly ingestible ClickHouse schema and documented three-column timeline-to-data join.
- Preserves: the metadata sidecar table and metadata-ID lookup relationship.

- [ ] **Step 1: Rewrite the row contracts and relationships**

Replace the flat main-row section with this representative shape:

```json
{
  "ogre_id": "<generated-data-id>",
  "computer": "host-01",
  "data_type": "evtx",
  "ogre_md_id": "<metadata-id>",
  "data": {
    "timestamp": "2026-08-06T12:00:00.000000+00:00",
    "system": {"computer": "event-host"},
    "event_data": {}
  }
}
```

Document these exact relationships:

```text
analytics_data.ogre_md_id -> analytics_metadata.id
analytics_timeline.metadata_id -> analytics_metadata.id
(analytics_timeline.data_type, analytics_timeline.computer, analytics_timeline.data_id)
    -> (analytics_data.data_type, analytics_data.computer, analytics_data.ogre_id)
```

State that artifact fields with envelope names remain under `data.*`, that analytics timeline `computer` and `data_type` are metadata-derived, and that analytics never emits an undated timeline row.

- [ ] **Step 2: Replace the ClickHouse data and timeline DDL exactly**

Use these definitions:

```sql
CREATE TABLE analytics_data
(
    ogre_id String,
    computer String,
    data_type String,
    ogre_md_id String,
    data JSON
)
ENGINE = ReplacingMergeTree()
ORDER BY (data_type, computer, ogre_id);

CREATE TABLE analytics_timeline
(
    id String,
    computer String,
    timestamp DateTime64(6, 'UTC'),
    timestamp_meaning String,
    data_type String,
    related_user String,
    description String,
    additional_description String,
    metadata_id String,
    data_id String
)
ENGINE = ReplacingMergeTree()
ORDER BY (computer, data_type, timestamp, data_id, id);
```

Keep the existing `analytics_metadata` columns and `ReplacingMergeTree() ORDER BY id`. Remove `ogre_md_id` from the data ordering key, remove `metadata_id` from the timeline ordering key, and remove nullable timestamp text.

- [ ] **Step 3: Document direct ingestion, views, and joins**

State that each line maps directly to columns with `JSONEachRow`; `data` alone is the native JSON object. Include these views:

```sql
CREATE VIEW analytics_data_evtx AS
SELECT ogre_id, ogre_md_id, computer, data_type, data
FROM analytics_data
WHERE data_type = 'evtx';

CREATE VIEW analytics_data_evtx_typed AS
SELECT
    ogre_id,
    ogre_md_id,
    computer,
    data.system.computer.:String AS event_computer
FROM analytics_data
WHERE data_type = 'evtx';
```

Use this join and no metadata-ID predicate:

```sql
SELECT t.*, d.data, m.original_filename
FROM analytics_timeline AS t
LEFT JOIN analytics_data AS d
    ON d.data_type = t.data_type
   AND d.computer = t.computer
   AND d.ogre_id = t.data_id
LEFT JOIN analytics_metadata AS m
    ON m.id = t.metadata_id;
```

Explain that `ReplacingMergeTree` replacement occurs during merges and does not enforce immediate uniqueness; mention `FINAL` or an ingestion/query deduplication strategy where immediate logical deduplication is required.

- [ ] **Step 4: Scan the rewritten document for obsolete flat-model guidance**

Run:

```bash
rg -n "payload JSON|MATERIALIZED|getSubcolumn|timeline-events|Nullable\(DateTime|ENGINE = MergeTree$|ORDER BY \(data_id, metadata_id|d\.ogre_md_id = t\.metadata_id" docs/analytics-jsonl-clickhouse-data-model.md
rg -n "data JSON|ReplacingMergeTree|ORDER BY \(data_type, computer, ogre_id\)|ORDER BY \(computer, data_type, timestamp, data_id, id\)|d\.data_type = t\.data_type|d\.computer = t\.computer|d\.ogre_id = t\.data_id" docs/analytics-jsonl-clickhouse-data-model.md
```

Expected: the first command returns no matches; the second finds every approved schema and join invariant.

- [ ] **Step 5: Commit the documentation change**

Run:

```bash
git diff --check
git add docs/analytics-jsonl-clickhouse-data-model.md
git commit -m "Update analytics ClickHouse data model"
```

Expected: the commit contains only the updated tracked ClickHouse document.

### Task 6: Verify the Complete Feature

**Files:**
- Verify only; modify files only to correct a failure attributable to Tasks 1-5.

**Interfaces:**
- Consumes: all implementation and documentation commits.
- Produces: evidence that formatting, unit/integration tests, strict linting, generated EVTX output, and repository scope are correct.

- [ ] **Step 1: Run focused analytics and compatibility tests**

Run:

```bash
cargo test --locked analytics
cargo test --locked normalized
cargo test --locked mixed_formats_preserve_existing_id_projection
cargo test --locked timeline_include_undated
cargo test --locked evtx_analytics_jsonl_writes_joinable_rows
```

Expected: all commands PASS.

- [ ] **Step 2: Inspect the generated EVTX analytics row shape**

Run:

```bash
sed -n '1p' .tmp/analytics/evtx_kernel_pnp_analytics.evtx.jsonl
sed -n '1p' .tmp/analytics/evtx_kernel_pnp_analytics.evtx.tl.jsonl
```

Expected: the data line has only the four technical strings plus `data`; the timeline line has a non-null timestamp and matching `computer`, `data_type`, and `data_id` values.

- [ ] **Step 3: Run the repository-required checks**

Run:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Expected: all commands exit successfully. Do not run Python tests.

- [ ] **Step 4: Audit scope and commit history**

Run:

```bash
git diff --check main...HEAD
git status --short --branch
git log --oneline --decorate main..HEAD
```

Expected: no whitespace errors, no unintended files, a clean `feature/analytics-jsonl-data-envelope` worktree, and the design plus small implementation commits described above. Do not push or merge.
