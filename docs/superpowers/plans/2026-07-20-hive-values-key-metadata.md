# Hive Values Key Metadata and Complete-Key Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `parse_hive_values` emit one dedicated flat row for every real registry value and one explicit placeholder for every empty descendant key, with key modification time and security descriptor attached in best-effort mode.

**Architecture:** Keep the existing direct `KeyNode` traversal. Build a best-effort base record once per key, clone it into real or partial value rows, and create one marked placeholder only when no source value row exists. Split the test mappings so `hive.xml` defines flat value output while `hive_keys.xml` preserves the existing nested key output.

**Tech Stack:** Rust 2024, `dfir-nt-hive`, `chrono`, existing `Record`/`Value`/`Output` abstractions, XML `PluginConfiguration`, `serde_json`, and the `csv` crate.

## Global Constraints

- Work only on the existing non-`main` branch `hive-values-key-metadata`; do not implement on `main`.
- Never push this repository to a remote.
- Do not merge into local `main` unless the user explicitly requests it after completion.
- Preserve the public Rust and Python signatures of `parse_hive_values` and `parse_hive_keys`.
- Exclude the hive root node; only successfully enumerated descendants are candidates for output.
- Keep the mapping data type exactly `hive` and keep existing value truncation and genuine registry type behavior.
- Preserve output-writer and finalization failures as fatal errors.
- Do not add dependencies, Python tests, or new/modified binary hive fixtures.
- Use inline Rust tests in `src/parser/hive.rs` and the existing sanitized fixtures.
- Use `apply_patch` for repository file edits and preserve unrelated working-tree changes.

## File Map

- Modify `src/parser/hive.rs`: value-row constants, best-effort key metadata construction, placeholder construction, traversal behavior, and parser tests.
- Modify `test_data/hive/hive.xml`: dedicated flat `parse_hive_values` mapping and value-oriented timeline fields.
- Create `test_data/hive/hive_keys.xml`: unchanged nested mapping used by `parse_hive_keys` tests.
- Reference `docs/superpowers/specs/2026-07-20-hive-values-key-metadata-design.md`: approved behavioral and compatibility contract.

---

### Task 1: Preserve the key parser's dedicated mapping

**Files:**
- Create: `test_data/hive/hive_keys.xml`
- Modify: `src/parser/hive.rs:670-940,1110-1160` (key-parser test configuration paths only)

**Interfaces:**
- Consumes: Existing `PluginConfiguration::from_str` and the current contents of `test_data/hive/hive.xml`.
- Produces: `test_data/hive/hive_keys.xml`, a key-oriented schema used only by `parse_hive_keys` tests. Task 3 can then replace `hive.xml` without changing key output.

- [ ] **Step 1: Point one key test at the not-yet-created mapping**

In `test_parse_hive_keys`, make this exact change:

```rust
let xml = fs::read_to_string("test_data/hive/hive_keys.xml").unwrap();
let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();
```

- [ ] **Step 2: Run the focused test and verify the missing fixture fails**

Run:

```bash
cargo test parser::hive::tests::test_parse_hive_keys -- --exact
```

Expected: FAIL because `test_data/hive/hive_keys.xml` does not exist.

- [ ] **Step 3: Create the complete key-oriented XML mapping**

Create `test_data/hive/hive_keys.xml` with this complete content, preserving the current key schema:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Hive" file_encoding="UTF_8">
  <mapping data_type="hive">
    <description>Parse data the NTFS Info csv file</description>

    <default_parser value="Ignore" />
    <default_date_pattern value='%Y-%m-%d %H:%M:%S.%3f' />
    <include_values value="true" />
    <truncate_values_after value="512" />

    <timeline>
      <timeline_type value="Standard" />
      <max_date_meaning value="0" />
      <source_type value="file" />
      <related_user include_field_name="false" field_separator=" - ">
        <output_name value="descriptor.owner_sid" />
      </related_user>
      <description>
        <output_name value="path" />
      </description>
    </timeline>

    <fields>
      <field input="name" parser="String" qualifier="KEY_NAME" />
      <field input="path" parser="String" qualifier="KEY_PATH" />
      <field input="mtime" parser="DateTime" qualifier="DATE_MODIFICATION" />
      <array>
        <object input="values" ignore="false">
          <field input="name" parser="String" qualifier="VALUE_NAME" />
          <field input="data" parser="String" />
          <field input="type" parser="String" />
          <field input="size" parser="Int" />
          <field input="error" parser="String" />
        </object>
      </array>
      <object input="descriptor">
        <field input="owner_sid" parser="String" qualifier="USER_SID" />
        <field input="group_sid" parser="String" />
        <array>
          <field input="control_flags" parser="String" />
        </array>
        <array>
          <object input="sacl_aces" ignore="false">
            <field input="ace_type" parser="String" />
            <array>
              <field input="ace_flags" parser="String" />
            </array>
            <array>
              <field input="rights" parser="String" />
            </array>
            <field input="account_sid" parser="String" />
            <field input="ace_size" parser="Int" />
            <field input="object_type_guid" parser="String" />
            <field input="inherited_object_type_guid" parser="String" />
            <field input="raw_hex" parser="String" />
          </object>
        </array>
        <array>
          <object input="dacl_aces" ignore="false">
            <field input="ace_type" parser="String" />
            <array>
              <field input="ace_flags" parser="String" />
            </array>
            <array>
              <field input="rights" parser="String" />
            </array>
            <field input="account_sid" parser="String" />
            <field input="ace_size" parser="Int" />
            <field input="object_type_guid" parser="String" />
            <field input="inherited_object_type_guid" parser="String" />
            <field input="raw_hex" parser="String" />
          </object>
        </array>
      </object>
    </fields>
  </mapping>
</plugin>
```

- [ ] **Step 4: Move every remaining key-parser test to the key mapping**

In these five tests, replace only the `fs::read_to_string("test_data/hive/hive.xml")` call with `fs::read_to_string("test_data/hive/hive_keys.xml")`:

```text
test_parse_hive_key_values
test_filter_keys
test_bad_filter
test_root_name_filter
test_parse_nt_user_key_values
```

Leave `test_parse_hive_values`, `test_parse_sam_values`, and `test_parse_nt_user_values` on `test_data/hive/hive.xml`.

- [ ] **Step 5: Verify all hive tests still pass before changing behavior**

Run:

```bash
cargo test parser::hive::tests
```

Expected: PASS; key and value tests retain their existing counts and shapes.

- [ ] **Step 6: Commit the mapping split**

```bash
git add src/parser/hive.rs test_data/hive/hive_keys.xml
git commit -m "test: split hive parser mappings"
```

---

### Task 2: Emit complete best-effort value rows

**Files:**
- Modify: `src/parser/hive.rs:294-300` (constants)
- Modify: `src/parser/hive.rs:388-452` (value traversal and helpers)
- Modify: `src/parser/hive.rs:625-635` (test imports)
- Modify: `src/parser/hive.rs:944-1105` (value tests)

**Interfaces:**
- Consumes: `KeyNode::timestamp`, `KeyNode::security_descriptor`, `KeyNode::values`, `from_filetime`, `security_descriptor_from_bytes`, `Record`, `Value`, `Output::write`, and `RunReport::add_error`.
- Produces: `build_value_base_record<B>(&KeyNode<'_, B>, &str, &mut RunReport) -> Record`, `add_placeholder_value(&mut Record)`, and a best-effort `parse_subvalues` that returns `Err` only for fatal output/recursive errors.

- [ ] **Step 1: Replace the testhive value assertions with the complete-key contract**

Extend the test import to include `HashSet`:

```rust
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
```

In `test_parse_hive_values`, keep the setup and parser invocation, then replace the assertions after `let result = &report.output_reports[0];` with:

```rust
let expected_lines = 537;
assert_eq!(expected_lines, result.file_reports[0].num_lines);

let jsonl = fs::read_to_string(targetfile).unwrap();
let lines: Vec<serde_json::Value> = jsonl
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect();

assert_eq!(expected_lines, lines.len());

let paths: HashSet<&str> = lines
    .iter()
    .map(|line| line["path"].as_str().unwrap())
    .collect();
assert_eq!(527, paths.len());
assert!(!paths.contains(""));

let real_rows = lines
    .iter()
    .filter(|line| line["is_placeholder"].as_bool() == Some(false))
    .count();
let placeholder_rows = lines
    .iter()
    .filter(|line| line["is_placeholder"].as_bool() == Some(true))
    .count();
assert_eq!(12, real_rows);
assert_eq!(525, placeholder_rows);

let real = lines
    .iter()
    .find(|line| line["path"] == "\\big-data-test" && line["name"] == "A")
    .unwrap();
assert_eq!(real["is_placeholder"], false);
assert_eq!(real["type"], "REG_BINARY");
assert_eq!(real["size"], 16343);
assert!(real["data"].as_str().unwrap().ends_with("[..]"));
assert_eq!(real["mtime"], "2023-01-18T11:39:48.154060+00:00");
assert_eq!(
    real["security_descriptor"]["owner_sid"],
    "S-1-5-32-544"
);

let dword = lines
    .iter()
    .find(|line| line["path"] == "\\data-test" && line["name"] == "dword")
    .unwrap();
assert_eq!(dword["data"], 42);

let multi_string = lines
    .iter()
    .find(|line| line["path"] == "\\data-test" && line["name"] == "reg-multi-sz")
    .unwrap();
assert_eq!(multi_string["data"][0], "multi-sz-test");
assert_eq!(multi_string["data"][1], "line2");

let placeholder = lines
    .iter()
    .find(|line| line["path"] == "\\character-encoding-test")
    .unwrap();
assert_eq!(placeholder["name"], "Default");
assert_eq!(placeholder["data"], "");
assert_eq!(placeholder["type"], "REG_NONE");
assert_eq!(placeholder["size"], 0);
assert_eq!(placeholder["is_placeholder"], true);
assert!(placeholder["mtime"].as_str().is_some());
assert!(placeholder["security_descriptor"].is_object());
```

- [ ] **Step 2: Add genuine-default and malformed-value assertions**

In `test_parse_sam_values`, keep `expected_lines = 79`, then add this assertion after reading `lines`:

```rust
let genuine_default = lines
    .iter()
    .find(|line| line["path"] == "\\SAM\\Domains" && line["name"] == "Default")
    .unwrap();
assert_eq!(genuine_default["is_placeholder"], false);
assert_eq!(genuine_default["type"], "REG_FIRST_INVALID");
assert_eq!(genuine_default["size"], 0);
```

In `test_parse_nt_user_values`, change `expected_lines` from `4484` to `5685`, then add:

```rust
let malformed_path = "\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.controlcenter.uistate\\windows.data.controlcenter.uistate";
let malformed = lines
    .iter()
    .find(|line| line["path"] == malformed_path && line["name"] == "Data")
    .unwrap();
assert_eq!(malformed["is_placeholder"], false);
assert_eq!(
    malformed["error"],
    "Parsing error: Invalid offset:'-4'"
);

assert!(lines.iter().any(|line| {
    line["path"]
        == "\\System\\GameConfigStore\\Parents\\f5ab3c1ab0f1af22669f461bd5b93334277d9878"
}));
```

The last assertion proves traversal continued to a later subtree after the malformed value.

- [ ] **Step 3: Run the new contract tests and verify they fail**

Run:

```bash
cargo test parser::hive::tests::test_parse_hive_values -- --exact
```

Expected: FAIL because current output has 12 rows rather than 537 and lacks `is_placeholder`, `mtime`, and `security_descriptor` on value records.

Run:

```bash
cargo test parser::hive::tests::test_parse_sam_values -- --exact
```

Expected: FAIL because genuine values do not yet carry `is_placeholder: false`.

- [ ] **Step 4: Add the dedicated value constants and row helpers**

Add these constants beside the existing `HIVE_VALUE_*` constants:

```rust
const HIVE_VALUE_SECURITY_DESCRIPTOR: &str = "security_descriptor";
const HIVE_VALUE_IS_PLACEHOLDER: &str = "is_placeholder";
```

Add these helpers immediately before `parse_subvalues`:

```rust
fn build_value_base_record<B>(
    key_node: &KeyNode<'_, B>,
    path: &str,
    report: &mut RunReport,
) -> Record
where
    B: SplitByteSlice,
{
    let mut record = Record::with_capacity(3);
    record.add(HIVE_VALUE_PATH, Value::String(path.to_owned()));

    match from_filetime(key_node.timestamp()) {
        Ok(mtime) => record.add(HIVE_KEY_DATE, Value::Date(mtime)),
        Err(error) => {
            report.add_error(format!(
                "while parsing mtime for '{path}', error: {error}"
            ));
            record.add(HIVE_KEY_DATE, Value::Null());
        }
    }

    match key_node.security_descriptor() {
        Ok(bytes) => match security_descriptor_from_bytes(&bytes) {
            Ok(descriptor) => record.add(
                HIVE_VALUE_SECURITY_DESCRIPTOR,
                Value::Object(descriptor.to_record()),
            ),
            Err(error) => {
                report.add_error(format!(
                    "while parsing security descriptor for '{path}', error: {error}"
                ));
                record.add(HIVE_VALUE_SECURITY_DESCRIPTOR, Value::Null());
            }
        },
        Err(error) => {
            report.add_error(format!(
                "while reading security descriptor for '{path}', error: {error}"
            ));
            record.add(HIVE_VALUE_SECURITY_DESCRIPTOR, Value::Null());
        }
    }

    record
}

fn add_placeholder_value(record: &mut Record) {
    record.add(HIVE_VALUE_NAME, Value::String("Default".to_owned()));
    record.add(HIVE_VALUE_DATA, Value::String(String::new()));
    record.add(HIVE_VALUE_TYPE, Value::String("REG_NONE".to_owned()));
    record.add(HIVE_VALUE_SIZE, Value::Int(0));
    record.add(HIVE_VALUE_IS_PLACEHOLDER, Value::Bool(true));
}
```

- [ ] **Step 5: Replace `parse_subvalues` with the best-effort traversal**

Replace the complete existing `parse_subvalues` function with:

```rust
fn parse_subvalues<B>(
    key_node: KeyNode<B>,
    path: &str,
    output: &mut Output,
    regex: &Option<Regex>,
    truncate_after: usize,
    report: &mut RunReport,
) -> Result<(), Error>
where
    B: SplitByteSlice,
{
    let Some(subkeys) = key_node.subkeys() else {
        return Ok(());
    };

    let subkeys = match subkeys {
        Ok(subkeys) => subkeys,
        Err(error) => {
            report.add_error(format!(
                "while parsing sub_keys for '{path}' error: {error}"
            ));
            return Ok(());
        }
    };

    for key_node in subkeys {
        let key_node = match key_node {
            Ok(key_node) => key_node,
            Err(error) => {
                report.add_error(format!(
                    "while parsing a sub_key under '{path}', error: {error}"
                ));
                continue;
            }
        };

        let key_name = match key_node.name() {
            Ok(name) => name.to_string_lossy(),
            Err(error) => {
                report.add_error(format!(
                    "while parsing a sub_key name under '{path}', error: {error}"
                ));
                continue;
            }
        };

        let path = format!("{path}\\{key_name}");

        if let Some(reg) = regex
            && !reg.is_match(&path)
        {
            parse_subvalues(key_node, &path, output, regex, truncate_after, report)?;
            continue;
        }

        let base_record = build_value_base_record(&key_node, &path, report);
        let mut wrote_row = false;

        match key_node.values() {
            Some(Ok(values)) => {
                for value in values {
                    let mut value_record = base_record.clone();
                    value_record.add(HIVE_VALUE_IS_PLACEHOLDER, Value::Bool(false));

                    if let Err(error) = parse_key_value(value, &mut value_record, truncate_after) {
                        report.add_error(format!(
                            "while parsing values for '{path}', error: {error}"
                        ));
                        value_record.add(
                            HIVE_VALUE_ERROR,
                            Value::String(format!("Parsing error: {error}")),
                        );
                    }

                    output.write(&mut value_record)?;
                    wrote_row = true;
                }
            }
            Some(Err(error)) => {
                report.add_error(format!(
                    "while parsing values for '{path}', error: {error}"
                ));
                let mut placeholder = base_record.clone();
                add_placeholder_value(&mut placeholder);
                placeholder.add(
                    HIVE_VALUE_ERROR,
                    Value::String(format!("Parsing error: {error}")),
                );
                output.write(&mut placeholder)?;
                wrote_row = true;
            }
            None => {}
        }

        if !wrote_row {
            let mut placeholder = base_record;
            add_placeholder_value(&mut placeholder);
            output.write(&mut placeholder)?;
        }

        parse_subvalues(key_node, &path, output, regex, truncate_after, report)?;
    }

    Ok(())
}
```

This code intentionally catches child, name, timestamp, descriptor, iterator, and per-value errors. It intentionally continues to propagate `Output::write` and recursive fatal errors.

- [ ] **Step 6: Format and run the focused contract tests**

Run:

```bash
cargo fmt
cargo test parser::hive::tests::test_parse_hive_values -- --exact
cargo test parser::hive::tests::test_parse_sam_values -- --exact
cargo test parser::hive::tests::test_parse_nt_user_values -- --exact
```

Expected: PASS with 537 `testhive` rows, 79 SAM rows, and 5685 NTUSER rows. The NTUSER test still records the existing `Invalid offset:'-4'` error while proving later output exists.

- [ ] **Step 7: Run all hive tests**

Run:

```bash
cargo test parser::hive::tests
```

Expected: PASS; `parse_hive_keys` counts remain unchanged and all value rows now carry the new parser-level fields.

- [ ] **Step 8: Commit the parser behavior**

```bash
git add src/parser/hive.rs
git commit -m "feat: emit complete hive value rows"
```

---

### Task 3: Activate and verify the dedicated flat output schema

**Files:**
- Modify: `test_data/hive/hive.xml`
- Modify: `src/parser/hive.rs:944-1000` (JSON schema assertion)
- Modify: `src/parser/hive.rs` test module (new CSV schema test)

**Interfaces:**
- Consumes: Task 1's `hive_keys.xml`, Task 2's fields `mtime`, `security_descriptor`, and `is_placeholder`, plus the schema-driven `CsvFormatter` behavior.
- Produces: A value-only XML field order and tests proving aligned mapped data columns in regular and normalized CSV, including the normalized framework identifiers.

- [ ] **Step 1: Add assertions that expose the old shared schema**

In `test_parse_hive_values`, add this assertion after the placeholder checks:

```rust
assert!(lines.iter().all(|line| line.get("values").is_none()));
```

Add this new test to the hive test module:

```rust
#[test]
fn hive_values_csv_outputs_follow_flat_schema() {
    let output_folder = ".tmp";
    fs::create_dir_all(output_folder).unwrap();
    let csv_name = "hive_values_schema_csv";
    let normalized_name = "hive_values_schema_normalized_csv";
    let csv_path = format!("{output_folder}/{csv_name}.hive.csv");
    let normalized_path = format!("{output_folder}/{normalized_name}.hive.csv");

    for path in [&csv_path, &normalized_path] {
        if Path::new(path).exists() {
            fs::remove_file(path).unwrap();
        }
    }

    let outputs = vec![
        OutputConfiguration::new(
            csv_name.to_owned(),
            output_folder.to_owned(),
            "file".to_owned(),
            "csv".to_owned(),
            "iso".to_owned(),
            false,
            false,
            HashMap::new(),
        ),
        OutputConfiguration::new(
            normalized_name.to_owned(),
            output_folder.to_owned(),
            "file".to_owned(),
            "normalized_csv".to_owned(),
            "iso".to_owned(),
            false,
            false,
            HashMap::new(),
        ),
    ];

    let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
    let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();
    let report = parse_hive_values(
        "test_data/hive/testhive",
        RunConfiguration::new(outputs, true, None),
        plugin_config,
        Metadata::new("test".into()),
        None,
        None,
    );
    assert_eq!(report.last_error, None);

    let data_fields = [
        "path",
        "mtime",
        "security_descriptor",
        "name",
        "data",
        "type",
        "size",
        "is_placeholder",
        "invalid_signature",
        "error",
    ];

    for (path, data_id_field, metadata_field) in [
        (&csv_path, None, "ogre_md"),
        (&normalized_path, Some("ogre_id"), "ogre_md_id"),
    ] {
        let data = fs::read_to_string(path).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .from_reader(data.as_bytes());
        let headers = reader.headers().unwrap().clone();
        let mut expected_headers = data_fields.to_vec();
        if let Some(data_id_field) = data_id_field {
            expected_headers.push(data_id_field);
        }
        expected_headers.push(metadata_field);
        assert_eq!(headers.iter().collect::<Vec<_>>(), expected_headers);

        let records: Vec<csv::StringRecord> =
            reader.records().map(|record| record.unwrap()).collect();
        assert_eq!(537, records.len());
        assert!(records.iter().all(|record| record.len() == headers.len()));

        let path_index = headers.iter().position(|field| field == "path").unwrap();
        let descriptor_index = headers
            .iter()
            .position(|field| field == "security_descriptor")
            .unwrap();
        let placeholder_index = headers
            .iter()
            .position(|field| field == "is_placeholder")
            .unwrap();
        let placeholder = records
            .iter()
            .find(|record| record.get(path_index) == Some("\\character-encoding-test"))
            .unwrap();
        assert_eq!(placeholder.get(placeholder_index), Some("true"));
        let descriptor: serde_json::Value =
            serde_json::from_str(placeholder.get(descriptor_index).unwrap()).unwrap();
        assert!(descriptor["owner_sid"].is_string());
    }
}
```

- [ ] **Step 2: Add a failing value-timeline schema test**

Add this test to prove the value-oriented timeline paths use the new schema:

```rust
#[test]
fn hive_values_timeline_uses_key_metadata_and_value_name() {
    let output_folder = ".tmp";
    fs::create_dir_all(output_folder).unwrap();
    let base_file_name = "hive_values_timeline";
    let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

    if Path::new(&targetfile).exists() {
        fs::remove_file(&targetfile).unwrap();
    }

    let output_conf = OutputConfiguration::new(
        base_file_name.to_owned(),
        output_folder.to_owned(),
        "file".to_owned(),
        "jsonl".to_owned(),
        "iso".to_owned(),
        true,
        false,
        HashMap::new(),
    );
    let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
    let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

    let report = parse_hive_values(
        "test_data/hive/testhive",
        RunConfiguration::new(vec![output_conf], true, None),
        plugin_config,
        Metadata::new("test".into()),
        None,
        None,
    );
    assert_eq!(report.last_error, None);

    let jsonl = fs::read_to_string(targetfile).unwrap();
    let lines: Vec<serde_json::Value> = jsonl
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(537, lines.len());

    let real = lines
        .iter()
        .find(|line| line["data"]["path"] == "\\big-data-test" && line["data"]["name"] == "A")
        .unwrap();
    assert_eq!(real["timestamp"], "2023-01-18T11:39:48.154060+00:00");
    assert_eq!(real["related_user"], "S-1-5-32-544");
    assert_eq!(real["description"], "path: \\big-data-test - name: A");
}
```

- [ ] **Step 3: Run the JSON and CSV tests against the old shared schema**

Run:

```bash
cargo test parser::hive::tests::hive_values_csv_outputs_follow_flat_schema -- --exact
```

Expected: FAIL because the current shared XML header contains `values` and `descriptor` and does not map the new flat fields.

Run:

```bash
cargo test parser::hive::tests::test_parse_hive_values -- --exact
```

Expected: FAIL because JSON output still contains the mapping-created `values: []` field.

- [ ] **Step 4: Run the timeline test against the old shared schema**

Run:

```bash
cargo test parser::hive::tests::hive_values_timeline_uses_key_metadata_and_value_name -- --exact
```

Expected: FAIL because the shared timeline mapping looks for `descriptor.owner_sid` and describes only `path`.

- [ ] **Step 5: Replace `hive.xml` with the complete flat value mapping**

Replace `test_data/hive/hive.xml` with:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Hive" file_encoding="UTF_8">
  <mapping data_type="hive">
    <description>Parse Windows Registry hive values</description>

    <default_parser value="Ignore" />
    <default_date_pattern value='%Y-%m-%d %H:%M:%S.%3f' />
    <truncate_values_after value="512" />

    <timeline>
      <timeline_type value="Standard" />
      <max_date_meaning value="0" />
      <source_type value="file" />
      <related_user include_field_name="false" field_separator=" - ">
        <output_name value="security_descriptor.owner_sid" />
      </related_user>
      <description>
        <output_name value="path" />
        <output_name value="name" />
      </description>
    </timeline>

    <fields>
      <field input="path" parser="String" qualifier="KEY_PATH" />
      <field input="mtime" parser="DateTime" qualifier="DATE_MODIFICATION" />
      <object input="security_descriptor">
        <field input="owner_sid" parser="String" qualifier="USER_SID" />
        <field input="group_sid" parser="String" />
        <array>
          <field input="control_flags" parser="String" />
        </array>
        <array>
          <object input="sacl_aces" ignore="false">
            <field input="ace_type" parser="String" />
            <array>
              <field input="ace_flags" parser="String" />
            </array>
            <array>
              <field input="rights" parser="String" />
            </array>
            <field input="account_sid" parser="String" />
            <field input="ace_size" parser="Int" />
            <field input="object_type_guid" parser="String" />
            <field input="inherited_object_type_guid" parser="String" />
            <field input="raw_hex" parser="String" />
          </object>
        </array>
        <array>
          <object input="dacl_aces" ignore="false">
            <field input="ace_type" parser="String" />
            <array>
              <field input="ace_flags" parser="String" />
            </array>
            <array>
              <field input="rights" parser="String" />
            </array>
            <field input="account_sid" parser="String" />
            <field input="ace_size" parser="Int" />
            <field input="object_type_guid" parser="String" />
            <field input="inherited_object_type_guid" parser="String" />
            <field input="raw_hex" parser="String" />
          </object>
        </array>
      </object>
      <field input="name" parser="String" qualifier="VALUE_NAME" />
      <field input="data" parser="String" />
      <field input="type" parser="String" />
      <field input="size" parser="Int" />
      <field input="is_placeholder" parser="Bool" />
      <field input="invalid_signature" parser="Bool" />
      <field input="error" parser="String" />
    </fields>
  </mapping>
</plugin>
```

- [ ] **Step 6: Run the JSON, CSV, and timeline schema tests**

Run:

```bash
cargo fmt
cargo test parser::hive::tests::test_parse_hive_values -- --exact
cargo test parser::hive::tests::hive_values_csv_outputs_follow_flat_schema -- --exact
cargo test parser::hive::tests::hive_values_timeline_uses_key_metadata_and_value_name -- --exact
```

Expected: PASS. JSON has no `values` field; regular CSV ends with `ogre_md`; normalized CSV ends with `ogre_id;ogre_md_id`; every data file contains 537 aligned rows; and timeline rows use `security_descriptor.owner_sid` plus both `path` and `name`.

- [ ] **Step 7: Re-run the key parser compatibility tests**

Run:

```bash
cargo test parser::hive::tests::test_parse_hive_keys -- --exact
cargo test parser::hive::tests::test_parse_hive_key_values -- --exact
cargo test parser::hive::tests::test_parse_nt_user_key_values -- --exact
```

Expected: PASS with the existing key record shapes and counts loaded from `hive_keys.xml`.

- [ ] **Step 8: Commit the dedicated value schema and format tests**

```bash
git add src/parser/hive.rs test_data/hive/hive.xml
git commit -m "test: validate flat hive value schema"
```

---

### Task 4: Run repository-wide verification

**Files:**
- Verify only; no planned source changes.

**Interfaces:**
- Consumes: All commits from Tasks 1-3.
- Produces: Evidence that the branch is formatted, passes all Rust tests, and is warning-free under strict Clippy.

- [ ] **Step 1: Verify formatting and whitespace**

Run:

```bash
cargo fmt --check
git diff --check
```

Expected: both commands exit successfully with no output.

- [ ] **Step 2: Run the complete hive test module**

Run:

```bash
cargo test parser::hive::tests
```

Expected: PASS with no failed hive tests.

- [ ] **Step 3: Run the full Rust test suite**

Run:

```bash
cargo test
```

Expected: PASS with zero failed tests.

- [ ] **Step 4: Run strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: exit status 0 with no warnings.

- [ ] **Step 5: Inspect the final branch state**

Run:

```bash
git status --short --branch
git log --oneline --decorate -5
```

Expected: a clean `hive-values-key-metadata` branch containing the design commit, plan commit, and three implementation commits, with no push or merge performed.
