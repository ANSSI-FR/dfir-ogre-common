# Analytics JSONL Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `analytics_jsonl`, a normalized JSONL output that places authoritative metadata `computer` and `data_type` dimensions on every data row and the metadata `computer` on every timeline row.

**Architecture:** Replace the formatter's private normalized boolean with a crate-private `JsonMode` enum, route the new format through the normalized writer lifecycle, and add an analytics-only record projection that never mutates shared `LineBuilder` data. Extend the borrowed timeline serialization context with an analytics-only computer override while leaving public Rust and Python signatures unchanged.

**Tech Stack:** Rust 2024, PyO3, `serde_json`, `flate2`, inline Rust tests, Cargo

## Global Constraints

- Work only on the existing non-`main` branch `design/analytics-jsonl-output`; never implement on `main`.
- Never push this branch or any commits to a remote repository.
- Preserve all public Rust exports, Python classes, constructor signatures, and `dfir_ogre_common.pyi`.
- The only external API addition is acceptance of `"analytics_jsonl"` through the existing string-valued `OutputConfiguration.format` field.
- Preserve normalized IDs, metadata and timeline sidecars, filenames, gzip support, append behavior, reports, timeline filtering, and close behavior.
- Metadata wins top-level `computer` and `data_type` collisions; nested same-named fields remain unchanged.
- Always emit analytics `computer` and `data_type`, independently of `include_empty`.
- Preserve the timeline-specific `data_type`; only timeline `computer` is overridden from metadata.
- Do not change ID computation or recompute IDs after analytics projection.
- Do not add dependencies, fixtures, Python tests, or Python type-stub changes.
- Do not stage or restore unrelated workspace changes. In particular, leave the pre-existing deletions under `docs/superpowers/specs/` untouched and use path-specific `git add` commands.

---

## File Structure

- Modify `src/format_json.rs`: define `JsonMode`, construct normalized-family writers, serialize analytics data rows, pass the timeline computer override, and add formatter-level tests.
- Modify `src/output.rs`: parse and dispatch `analytics_jsonl`, enable hashes and normalized metadata lifecycle, and add end-to-end output tests.
- Modify `src/timeline.rs`: add a borrowed computer override to timeline JSON serialization and test precedence, escaping, and non-mutation.
- Modify `src/format_csv.rs`: initialize the new timeline context field to `None`; CSV behavior must not change.
- Do not modify `src/lib.rs`, `src/lib_py.rs`, `dfir_ogre_common.pyi`, or any Python-facing constructor.

### Task 1: Add the Internal JSON Mode and Normalized-Family Routing

**Files:**
- Modify: `src/format_json.rs:69-203`
- Modify: `src/format_json.rs:405-895`
- Modify: `src/output.rs:56-170`
- Modify: `src/output.rs:239-347`
- Test: `src/output.rs:494-633`
- Test: `src/output.rs:1001-1019`

**Interfaces:**
- Consumes: `JsonFormatter::new`, `OutputFormat::from_str`, `OutputWriter`, and normalized output lifecycle already present in the crate.
- Produces: `pub(crate) enum JsonMode { Standard, Normalized, Analytics }` and acceptance of the `"analytics_jsonl"` configuration string.
- Produces: `JsonFormatter::new(output_type: OutputType, output_folder: &str, file_name: &str, output_date_codec: DateOutputCodec, include_empty: bool, include_timeline: bool, compression_level: u32, file_report: FileReport, mode: JsonMode) -> Result<JsonFormatter, Error>` for Tasks 2 and 3.

- [ ] **Step 1: Write failing parser and lifecycle tests**

Add this assertion to `output_format_from_str_accepts_known_values`:

```rust
assert!(OutputFormat::from_str("analytics_jsonl").is_ok());
```

Add this integration test to `src/output.rs` using the existing
`normalized_output_config`, `mapped_plugin_config`, `event_field_mapping`, and
`test_output_folder` helpers:

```rust
#[test]
fn analytics_jsonl_uses_normalized_lifecycle() {
    let output_folder = test_output_folder("output_analytics_lifecycle");
    let output_conf = normalized_output_config(
        "analytics_lifecycle",
        &output_folder,
        "analytics_jsonl",
    );
    let mut output = Output::new(
        RunConfiguration::new(vec![output_conf], true, None),
        mapped_plugin_config("events", event_field_mapping()),
        Metadata::new("host-output".into()),
        None,
    )
    .unwrap();
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));

    output.write(&mut record).unwrap();
    output.finalize().unwrap();
    let reports = output.get_report().file_reports;
    drop(output);

    let data: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("analytics_lifecycle.events.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("ogre_metadata.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();

    assert!(data["ogre_id"].as_str().unwrap().len() > 10);
    assert!(data["ogre_md_id"].as_str().unwrap().len() > 10);
    assert_eq!(metadata["computer"], "host-output");
    assert_eq!(metadata["data_type"], "events");
    assert_eq!(reports.len(), 2);
    assert!(
        reports
            .iter()
            .all(|report| report.format == "analytics_jsonl")
    );

    remove_dir_if_exists(&output_folder);
}
```

- [ ] **Step 2: Run the new tests and verify the format is rejected**

Run:

```bash
cargo test analytics_jsonl --locked
```

Expected: FAIL because `OutputFormat::from_str("analytics_jsonl")` returns
`Error::InvalidOutputFormat` and the lifecycle test unwraps that error.

- [ ] **Step 3: Introduce the crate-private mode in the JSON formatter**

Add this mode near the top of `src/format_json.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsonMode {
    Standard,
    Normalized,
    Analytics,
}

impl JsonMode {
    fn uses_normalized_files(self) -> bool {
        matches!(self, Self::Normalized | Self::Analytics)
    }
}
```

Add the mode to `JsonFormatter`:

```rust
pub struct JsonFormatter {
    data_writer: Box<dyn Writer + Send + Sync>,
    metadata_writer: Option<Box<dyn Writer + Send + Sync>>,
    timeline_writer: Option<Box<dyn Writer + Send + Sync>>,
    metadata_serialized: Option<MetadataSerialized>,
    line_buffer: String,
    output_date_codec: DateOutputCodec,
    include_empty: bool,
    include_timeline: bool,
    mode: JsonMode,
    pub file_report: FileReport,
    metadata_file_report: Option<FileReport>,
    timeline_file_report: Option<FileReport>,
}
```

Change the final `JsonFormatter::new` argument from `normalized: bool` to
`mode: JsonMode`. Change the normalized writer allocation condition so the current
normalized branch executes when `mode.uses_normalized_files()` and the current
non-normalized branch executes otherwise. Keep every path, writer, report, and compression
statement inside those existing branches unchanged. Store `mode` in the returned struct:

```rust
Ok(Self {
    data_writer,
    metadata_writer,
    timeline_writer,
    line_buffer: String::new(),
    output_date_codec,
    metadata_serialized: None,
    include_empty,
    include_timeline,
    mode,
    file_report,
    metadata_file_report,
    timeline_file_report,
})
```

Update every `JsonFormatter::new` call in `src/format_json.rs` tests with this exact
mapping:

```rust
// Existing final argument false:
JsonMode::Standard,

// Existing final argument true:
JsonMode::Normalized,
```

- [ ] **Step 4: Route `analytics_jsonl` through the normalized lifecycle**

Change the formatter import in `src/output.rs`:

```rust
use crate::format_json::{JsonFormatter, JsonMode};
```

Add the new parsed and writer variants:

```rust
#[pyclass]
pub enum OutputFormat {
    Jsonl,
    NormalizedJsonl,
    AnalyticsJsonl,
    Csv,
    NormalizedCsv,
}

impl OutputFormat {
    fn from_str(format: &str) -> Result<OutputFormat, Error> {
        match format {
            "jsonl" => Ok(OutputFormat::Jsonl),
            "normalized_jsonl" => Ok(OutputFormat::NormalizedJsonl),
            "analytics_jsonl" => Ok(OutputFormat::AnalyticsJsonl),
            "csv" => Ok(OutputFormat::Csv),
            "normalized_csv" => Ok(OutputFormat::NormalizedCsv),
            _ => Err(Error::InvalidOutputFormat(format.to_owned())),
        }
    }
}

#[derive(Clone)]
enum OutputWriter {
    Jsonl(Arc<Mutex<JsonFormatter>>),
    NormalizedJsonl(Arc<Mutex<JsonFormatter>>),
    AnalyticsJsonl(Arc<Mutex<JsonFormatter>>),
    Csv(Arc<Mutex<CsvFormatter>>),
    NormalizedCsv(Arc<Mutex<CsvFormatter>>),
}
```

In `OutputWriter::write`, group analytics with normalized JSON:

```rust
Self::NormalizedJsonl(json_output) | Self::AnalyticsJsonl(json_output) => {
    let mut out = json_output.lock().expect("mutex lock panicked");
    out.write_normalized(data)
}
```

Use the same alternative pattern in `write_metadata` and call `out.write_metadata(data)`.
In `result`, group `Jsonl`, `NormalizedJsonl`, and `AnalyticsJsonl` and return
`out.reports()`. In `close`, group the same three variants and call `out.close()?`.

Classify analytics as hash-producing:

```rust
if matches!(
    &output_format,
    OutputFormat::NormalizedJsonl
        | OutputFormat::AnalyticsJsonl
        | OutputFormat::NormalizedCsv
) {
    compute_hash = true;
}
```

Pass `JsonMode::Standard` from the `Jsonl` construction arm and
`JsonMode::Normalized` from the `NormalizedJsonl` arm. Add this construction arm directly
after `NormalizedJsonl`:

```rust
OutputFormat::AnalyticsJsonl => {
    let json_output = JsonFormatter::new(
        output_type,
        output_folder,
        &base_file_name,
        output_date_codec,
        output_conf.include_empty,
        output_conf.with_timeline,
        compression_level,
        file_report,
        JsonMode::Analytics,
    )?;
    OutputWriter::AnalyticsJsonl(Arc::new(Mutex::new(json_output)))
}
```

- [ ] **Step 5: Run the routing tests and verify they pass**

Run:

```bash
cargo test analytics_jsonl --locked
```

Expected: PASS. At this point analytics output intentionally matches normalized output;
Tasks 2 and 3 add its extra fields.

- [ ] **Step 6: Verify the mode refactor did not change normalized JSON behavior**

Run:

```bash
cargo test format_json::tests --locked
cargo test normalized_outputs_dispatch_data_and_write_metadata --locked
cargo test normalized_jsonl_alone_generates_data_and_timeline_ids --locked
```

Expected: all commands PASS with existing assertions unchanged.

- [ ] **Step 7: Format the routing changes**

Run:

```bash
cargo fmt
cargo fmt --check
```

Expected: `cargo fmt --check` exits successfully.

- [ ] **Step 8: Commit the routing deliverable**

```bash
git add src/format_json.rs src/output.rs
git commit -m "Add analytics JSONL routing"
```

### Task 2: Enrich Analytics Data Rows Without Mutating Shared Records

**Files:**
- Modify: `src/format_json.rs:13-58`
- Modify: `src/format_json.rs:276-319`
- Test: `src/format_json.rs:349-895`
- Test: `src/output.rs:529-633`

**Interfaces:**
- Consumes: `JsonMode::Analytics` and `JsonFormatter::write_normalized` from Task 1.
- Produces: private `json_serialise_analytics_record(record, metadata, metadata_id, buffer, date_codec, include_empty) -> Result<(), Error>`.
- Guarantees: source records are borrowed and unchanged; top-level metadata dimensions replace input collisions exactly once.

- [ ] **Step 1: Write focused failing serializer tests**

Add these tests to `src/format_json.rs`:

```rust
#[test]
fn analytics_record_replaces_top_level_dimensions_and_preserves_nested_values() {
    let mut nested = Record::new();
    nested.add("computer", Value::String("nested-host".to_owned()));
    nested.add("data_type", Value::String("nested-type".to_owned()));
    let mut nested_array_item = Record::new();
    nested_array_item.add("computer", Value::String("array-host".to_owned()));
    nested_array_item.add("data_type", Value::String("array-type".to_owned()));

    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    record.add("computer", Value::String("input-host".to_owned()));
    record.add("data_type", Value::String("input-type".to_owned()));
    record.add("nested", Value::Object(nested));
    record.add(
        "items",
        Value::Array(vec![Value::Object(nested_array_item)]),
    );

    let mut metadata = Metadata::new("meta\"host\n".to_owned());
    metadata.data_type = "events\\kind".to_owned();
    let mut buffer = String::new();

    json_serialise_analytics_record(
        &record,
        &metadata,
        "md-id",
        &mut buffer,
        &DateOutputCodec::Iso(),
        false,
    )
    .unwrap();

    assert_eq!(
        buffer,
        "{\"event\":\"login\",\"nested\":{\"computer\":\"nested-host\",\"data_type\":\"nested-type\"},\"items\":[{\"computer\":\"array-host\",\"data_type\":\"array-type\"}],\"computer\":\"meta\\\"host\\n\",\"data_type\":\"events\\\\kind\",\"ogre_md_id\":\"md-id\"}"
    );
    assert_eq!(record.get("computer"), Some(&Value::String("input-host".to_owned())));
    assert_eq!(record.get("data_type"), Some(&Value::String("input-type".to_owned())));
}

#[test]
fn analytics_record_emits_empty_metadata_dimensions_when_empty_fields_are_excluded() {
    let mut record = Record::new();
    record.add("empty", Value::Null());
    let metadata = Metadata::new(String::new());
    let mut buffer = String::new();

    json_serialise_analytics_record(
        &record,
        &metadata,
        "md-id",
        &mut buffer,
        &DateOutputCodec::Iso(),
        false,
    )
    .unwrap();

    assert_eq!(
        buffer,
        "{\"computer\":\"\",\"data_type\":\"\",\"ogre_md_id\":\"md-id\"}"
    );
}
```

- [ ] **Step 2: Run the serializer tests and verify they fail to compile**

Run:

```bash
cargo test analytics_record_ --locked
```

Expected: FAIL because `json_serialise_analytics_record` is not defined.

- [ ] **Step 3: Implement the minimal analytics record projection**

Add these constants and helper beside `json_serialise_record_with_extra_field`:

```rust
const ANALYTICS_COMPUTER_FIELD: &str = "computer";
const ANALYTICS_DATA_TYPE_FIELD: &str = "data_type";

fn json_serialise_analytics_record(
    record: &Record,
    metadata: &Metadata,
    metadata_id: &str,
    buffer: &mut String,
    date_codec: &DateOutputCodec,
    include_empty: bool,
) -> Result<(), Error> {
    let start_len = buffer.len();
    let result = (|| {
        let mut object = JsonObjectWriter::new(buffer);
        for (key, value) in record.iter() {
            if key.as_str() == ANALYTICS_COMPUTER_FIELD
                || key.as_str() == ANALYTICS_DATA_TYPE_FIELD
            {
                continue;
            }
            let should_emit = include_empty || !matches!(value, Value::Null());
            if should_emit {
                object.try_field_raw(key, |buffer| {
                    Value::json_serialise_value(value, buffer, date_codec, include_empty)
                })?;
            }
        }
        object.string_field(ANALYTICS_COMPUTER_FIELD, &metadata.computer);
        object.string_field(ANALYTICS_DATA_TYPE_FIELD, &metadata.data_type);
        object.string_field(OUTPUT_METADATA_ID_FIELD, metadata_id);
        object.finish();
        Ok(())
    })();
    if result.is_err() {
        buffer.truncate(start_len);
    }
    result
}
```

Do not insert or remove fields on `record`; the iterator filter is the collision boundary.

- [ ] **Step 4: Run the serializer tests and verify they pass**

Run:

```bash
cargo test analytics_record_ --locked
```

Expected: both tests PASS, including exact key order, escaping, shallow replacement, and
source-record non-mutation.

- [ ] **Step 5: Write failing formatter and cross-output tests**

Extend `analytics_jsonl_uses_normalized_lifecycle` in `src/output.rs` with these assertions
immediately after parsing `data`:

```rust
assert_eq!(data["computer"], "host-output");
assert_eq!(data["data_type"], "events");
```

Add this mixed-output regression test to `src/output.rs`:

```rust
#[test]
fn analytics_dimensions_do_not_leak_into_standard_jsonl() {
    let output_folder = test_output_folder("output_analytics_mixed");
    let run_config = RunConfiguration::new(
        vec![
            normalized_output_config("analytics", &output_folder, "analytics_jsonl"),
            json_output_config("standard", &output_folder, HashMap::new()),
        ],
        true,
        None,
    );
    let mut output = Output::new(
        run_config,
        mapped_plugin_config("events", event_field_mapping()),
        Metadata::new("host-output".into()),
        None,
    )
    .unwrap();
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));

    output.write(&mut record).unwrap();
    output.finalize().unwrap();
    drop(output);

    let analytics: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("analytics.events.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    let standard: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("standard.events.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();

    assert_eq!(analytics["computer"], "host-output");
    assert_eq!(analytics["data_type"], "events");
    assert!(standard.get("computer").is_none());
    assert!(standard.get("data_type").is_none());
    assert_eq!(standard["ogre_md"]["computer"], "host-output");
    assert_eq!(standard["ogre_md"]["data_type"], "events");

    remove_dir_if_exists(&output_folder);
}
```

Add this gzip smoke test to `src/format_json.rs`:

```rust
#[test]
fn analytics_gzip_finishes_enriched_data_and_metadata_streams() {
    let output_folder = Path::new(TMP_FOLDER).join("test_analytics_json_gzip_dir");
    remove_dir_if_exists(&output_folder);
    std::fs::create_dir_all(&output_folder).unwrap();
    let output_folder_str = output_folder.to_str().unwrap();
    let data_path = output_folder.join("analytics.events.jsonl.gz");
    let metadata_path = output_folder.join("ogre_metadata.jsonl.gz");

    let mut metadata = Metadata::new("host-json-gz".into());
    metadata.data_type = "events".into();
    let mut line_builder = LineBuilder::new(
        metadata,
        None,
        FieldMapping::new(vec![], None),
        true,
        false,
        true,
    );
    let mut output = JsonFormatter::new(
        OutputType::Gzip,
        output_folder_str,
        "analytics.events",
        DateOutputCodec::Iso(),
        false,
        false,
        7,
        FileReport {
            format: "analytics_jsonl".to_owned(),
            ..Default::default()
        },
        JsonMode::Analytics,
    )
    .unwrap();
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    line_builder.build(&mut record).unwrap();

    output.write_metadata(&line_builder).unwrap();
    output.write_normalized(&line_builder).unwrap();
    output.close().unwrap();

    let mut data_decoder = GzDecoder::new(File::open(&data_path).unwrap());
    let mut data = String::new();
    data_decoder.read_to_string(&mut data).unwrap();
    let data: serde_json::Value = serde_json::from_str(data.trim()).unwrap();

    let mut metadata_decoder = GzDecoder::new(File::open(&metadata_path).unwrap());
    let mut metadata = String::new();
    metadata_decoder.read_to_string(&mut metadata).unwrap();
    let metadata: serde_json::Value = serde_json::from_str(metadata.trim()).unwrap();

    assert_eq!(data["computer"], "host-json-gz");
    assert_eq!(data["data_type"], "events");
    assert_eq!(metadata["computer"], "host-json-gz");
    assert_eq!(metadata["data_type"], "events");

    remove_dir_if_exists(&output_folder);
}
```

- [ ] **Step 6: Run the formatter tests and verify main-row enrichment still fails**

Run:

```bash
cargo test analytics_jsonl_uses_normalized_lifecycle --locked
cargo test analytics_dimensions_do_not_leak_into_standard_jsonl --locked
cargo test analytics_gzip_finishes_enriched_data_and_metadata_streams --locked
```

Expected: the tests compile, then FAIL because `write_normalized` still calls the ordinary
normalized serializer and does not emit top-level analytics dimensions.

- [ ] **Step 7: Select the analytics projection in `write_normalized`**

Replace the existing main-data serializer call in `JsonFormatter::write_normalized` with:

```rust
self.line_buffer.clear();
if self.mode == JsonMode::Analytics {
    json_serialise_analytics_record(
        &data.data,
        &line_builder.metadata,
        &line_builder.metadata_id,
        &mut self.line_buffer,
        &self.output_date_codec,
        self.include_empty,
    )?;
} else {
    json_serialise_record_with_extra_field(
        &data.data,
        OUTPUT_METADATA_ID_FIELD,
        JsonExtraField::String(&line_builder.metadata_id),
        &mut self.line_buffer,
        &self.output_date_codec,
        self.include_empty,
    )?;
}
```

Keep the following newline, writer call, and `file_report.num_lines` increment unchanged.

- [ ] **Step 8: Run all analytics data-row tests and verify they pass**

Run:

```bash
cargo test analytics_ --locked
```

Expected: serializer, lifecycle, mixed-output, and gzip tests PASS.

- [ ] **Step 9: Run JSON and output-module regressions**

Run:

```bash
cargo test format_json::tests --locked
cargo test output::tests --locked
```

Expected: all existing and new tests PASS.

- [ ] **Step 10: Format and commit the analytics data-row deliverable**

```bash
cargo fmt
cargo fmt --check
git add src/format_json.rs src/output.rs
git commit -m "Enrich analytics JSONL data rows"
```

### Task 3: Source Analytics Timeline Computer From Metadata

**Files:**
- Modify: `src/timeline.rs:56-121`
- Modify: `src/timeline.rs:1438-1512`
- Modify: `src/format_json.rs:218-319`
- Modify: `src/format_json.rs:576-791`
- Modify: `src/format_csv.rs:219-300`
- Test: `src/output.rs:707-777`

**Interfaces:**
- Consumes: `JsonMode::Analytics`, `LineBuilder.metadata`, and the existing borrowed `TimelineSerializationContext`.
- Produces: `TimelineSerializationContext::computer_override: Option<&str>`.
- Guarantees: JSON serialization prefers the override, CSV ignores it, and stored `TimeLine` values are not mutated.

- [ ] **Step 1: Write failing timeline-context and formatter tests**

Add this unit test to `src/timeline.rs`:

```rust
#[test]
fn timeline_json_context_computer_override_wins_without_mutation() {
    let tl = timeline_for_serialization_tests();
    let stored_computer = tl.computer.clone();
    let mut json = String::new();

    tl.json_serialise_with_context(
        &mut json,
        &DateOutputCodec::IsoUtc(),
        false,
        TimelineSerializationContext {
            computer_override: Some("metadata\"host\n"),
            ..TimelineSerializationContext::default()
        },
    )
    .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["computer"], "metadata\"host\n");
    assert_eq!(tl.computer, stored_computer);
}
```

Add this formatter test to `src/format_json.rs`:

```rust
#[test]
fn analytics_timeline_writer_overrides_stale_computer_from_metadata() {
    let output_folder = Path::new(TMP_FOLDER).join("test_analytics_timeline_override_dir");
    remove_dir_if_exists(&output_folder);
    std::fs::create_dir_all(&output_folder).unwrap();
    let output_folder_str = output_folder.to_str().unwrap();
    let timeline_path = output_folder.join("analytics.events.tl.jsonl");

    let timeline_builder = TimeLineBuilder::new(
        TimeLineType::Standard,
        "timeline-events".to_owned(),
        100,
        None,
        None,
    );
    let mut metadata = Metadata::new("metadata-host".into());
    metadata.data_type = "events".into();
    let mut line_builder = LineBuilder::new(
        metadata,
        Some(timeline_builder),
        timeline_field_mapping_for_two_dates(),
        true,
        false,
        true,
    );
    let mut record = Record::new();
    record.add(
        "when_a",
        Value::Date(
            crate::parse_date("2026-08-06T12:00:00Z", &DateInputCodec::Iso()).unwrap(),
        ),
    );
    record.add("message", Value::String("login".to_owned()));
    line_builder.build(&mut record).unwrap();
    line_builder.line_data.timeline[0].computer = Some("stale-host".to_owned());

    let mut output = JsonFormatter::new(
        OutputType::File,
        output_folder_str,
        "analytics.events",
        DateOutputCodec::Iso(),
        false,
        true,
        7,
        FileReport {
            format: "analytics_jsonl".to_owned(),
            with_timeline: true,
            ..Default::default()
        },
        JsonMode::Analytics,
    )
    .unwrap();

    output.write_normalized(&line_builder).unwrap();
    output.close().unwrap();

    let timeline: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&timeline_path).unwrap().trim(),
    )
    .unwrap();
    assert_eq!(timeline["computer"], "metadata-host");
    assert_eq!(
        line_builder.line_data.timeline[0].computer.as_deref(),
        Some("stale-host")
    );

    remove_dir_if_exists(&output_folder);
}
```

- [ ] **Step 2: Run the new timeline tests and verify the context field is missing**

Run:

```bash
cargo test timeline_json_context_computer_override_wins_without_mutation --locked
cargo test analytics_timeline_writer_overrides_stale_computer_from_metadata --locked
```

Expected: FAIL to compile because `TimelineSerializationContext` has no
`computer_override` field.

- [ ] **Step 3: Add the borrowed computer override with existing behavior as fallback**

Change the context definition in `src/timeline.rs`:

```rust
#[derive(Clone, Copy, Default)]
pub(crate) struct TimelineSerializationContext<'a> {
    pub(crate) metadata: Option<&'a MetadataSerialized>,
    pub(crate) data: Option<&'a Record>,
    pub(crate) metadata_id: Option<&'a str>,
    pub(crate) computer_override: Option<&'a str>,
}
```

Change only the computer field selection in `json_serialise_with_context`:

```rust
object.optional_string_field(
    "computer",
    context.computer_override.or(self.computer.as_deref()),
);
```

Add `computer_override: None` to the explicit context literals in
`TimeLine::json_serialise`, `TimeLine::csv_serialise`, both formatter paths in
`src/format_csv.rs`, and the standard `JsonFormatter::write` path. The CSV serializer keeps
reading `self.computer` and therefore remains byte-for-byte unchanged.

- [ ] **Step 4: Run the direct timeline-context test**

Run:

```bash
cargo test timeline_json_context_computer_override_wins_without_mutation --locked
```

Expected: PASS, including JSON escaping and the non-mutation assertion.

- [ ] **Step 5: Pass metadata computer only for analytics timeline writes**

Set the new field in the normalized-family timeline context inside
`JsonFormatter::write_normalized`:

```rust
TimelineSerializationContext {
    metadata: timeline.metadata.as_ref(),
    data: timeline.data.as_ref(),
    metadata_id: Some(&line_builder.metadata_id),
    computer_override: match self.mode {
        JsonMode::Analytics => Some(line_builder.metadata.computer.as_str()),
        JsonMode::Standard | JsonMode::Normalized => None,
    },
}
```

Do not assign to `timeline.computer`; analytics authority is a serialization concern.

- [ ] **Step 6: Run the formatter override test**

Run:

```bash
cargo test analytics_timeline_writer_overrides_stale_computer_from_metadata --locked
```

Expected: PASS. The emitted timeline contains `metadata-host`, while the stored timeline
still contains `stale-host`.

- [ ] **Step 7: Add the end-to-end timeline contract test**

Add this test to `src/output.rs`:

```rust
#[test]
fn analytics_jsonl_enriches_main_and_timeline_rows() {
    let output_folder = test_output_folder("output_analytics_timeline");
    let event_field = Field::Single {
        name: FieldName::new("event".to_owned(), false, None, None, None),
        parser: Parser::String(),
        default_value: None,
    };
    let timestamp_field = Field::Single {
        name: FieldName::new("when".to_owned(), false, None, None, None),
        parser: Parser::DateTime(DateInputCodec::Iso()),
        default_value: None,
    };
    let mut plugin_config = mapped_plugin_config(
        "events",
        FieldMapping::new(vec![event_field, timestamp_field], None),
    );
    let mut timeline = TimeLineBuilder::new(
        TimeLineType::Standard,
        "timeline-events".to_owned(),
        0,
        None,
        None,
    );
    timeline.add_description_ouput_path(vec!["event".to_owned()]);
    plugin_config.data_type_configs[0].timeline = Some(timeline);

    let output_conf = OutputConfiguration::new(
        "analytics_timeline".to_owned(),
        output_folder.display().to_string(),
        "file".to_owned(),
        "analytics_jsonl".to_owned(),
        "iso".to_owned(),
        true,
        true,
        HashMap::new(),
    );
    let mut output = Output::new(
        RunConfiguration::new(vec![output_conf], true, None),
        plugin_config,
        Metadata::new("metadata-host".into()),
        None,
    )
    .unwrap();
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));
    record.add(
        "when",
        Value::Date(
            crate::parse_date("2026-08-06T12:00:00Z", &DateInputCodec::Iso()).unwrap(),
        ),
    );

    output.write(&mut record).unwrap();
    output.finalize().unwrap();
    let reports = output.get_report().file_reports;
    drop(output);

    let data: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("analytics_timeline.events.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    let timeline: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("analytics_timeline.events.tl.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("ogre_metadata.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();

    assert_eq!(data["computer"], "metadata-host");
    assert_eq!(data["data_type"], "events");
    assert_eq!(timeline["computer"], "metadata-host");
    assert_eq!(timeline["data_type"], "timeline-events");
    assert_eq!(timeline["data_id"], data["ogre_id"]);
    assert_eq!(timeline["metadata_id"], data["ogre_md_id"]);
    assert_eq!(metadata["computer"], "metadata-host");
    assert_eq!(metadata["data_type"], "events");
    assert_eq!(reports.len(), 3);
    assert!(
        reports
            .iter()
            .all(|report| report.format == "analytics_jsonl")
    );

    remove_dir_if_exists(&output_folder);
}
```

- [ ] **Step 8: Run timeline and formatter regression suites**

Run:

```bash
cargo test analytics_jsonl_enriches_main_and_timeline_rows --locked
cargo test timeline::tests --locked
cargo test format_json::tests --locked
cargo test format_csv::tests --locked
```

Expected: all commands PASS. Existing normalized JSON and all CSV output remain unchanged.

- [ ] **Step 9: Format and commit the timeline deliverable**

```bash
cargo fmt
cargo fmt --check
git add src/timeline.rs src/format_json.rs src/format_csv.rs src/output.rs
git commit -m "Add analytics timeline computer"
```

### Task 4: Full Verification and External API Audit

**Files:**
- Verify: `src/format_json.rs`
- Verify: `src/output.rs`
- Verify: `src/timeline.rs`
- Verify: `src/format_csv.rs`
- Verify unchanged: `src/lib.rs`
- Verify unchanged: `src/lib_py.rs`
- Verify unchanged: `dfir_ogre_common.pyi`

**Interfaces:**
- Consumes: all completed deliverables from Tasks 1-3.
- Produces: a fully verified Rust implementation with no Python-facing signature changes.

- [ ] **Step 1: Verify formatting**

Run:

```bash
cargo fmt --check
```

Expected: exit status 0 with no formatting diff.

- [ ] **Step 2: Run the complete Rust test suite**

Run:

```bash
cargo test --locked
```

Expected: all unit and fixture-backed Rust tests PASS.

- [ ] **Step 3: Run strict linting**

Run:

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Expected: exit status 0 with no warnings.

- [ ] **Step 4: Verify the public API files are untouched**

Run:

```bash
git diff main...HEAD -- src/lib.rs src/lib_py.rs dfir_ogre_common.pyi
```

Expected: no output.

- [ ] **Step 5: Verify patch hygiene without disturbing unrelated deletions**

Run:

```bash
git diff --check
git status --short --branch
git log --oneline main..HEAD
```

Expected: `git diff --check` succeeds; feature source files have no uncommitted changes;
the branch contains the design, plan, routing, data-enrichment, and timeline commits. The
unrelated deleted design specs may remain visible in status and must not be staged,
restored, or committed.

- [ ] **Step 6: Stop without pushing or merging**

The completed implementation remains on `design/analytics-jsonl-output`. Report the test
and lint evidence to the user and offer local integration choices; do not run `git push`
and do not merge into `main` without an explicit user request.
