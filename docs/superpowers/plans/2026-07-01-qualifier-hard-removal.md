# Qualifier Hard Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the qualifier runtime feature while keeping XML `qualifier="..."` attributes loadable as ignored descriptive metadata.

**Architecture:** XML parsing stops validating qualifiers first, then the field-name model and line builder lose all qualified-name state. The output layer moves from qualified/unqualified dual builders to one plain `LineBuilder`, and the Python type hints are updated to match the approved breaking API changes.

**Tech Stack:** Rust 2024, PyO3, inline Rust unit tests, XML fixtures under `test_data/`, Python `.pyi` type hints.

---

## File Structure

- Modify `src/configuration.rs`: stop creating or passing `Qualifiers`; ignore XML `qualifier` attributes on `field`, `object`, and `multi_input`; add free-form qualifier configuration test.
- Modify `src/field.rs`: remove qualifier fields from `FieldName`; change `FieldName::new` and `FieldName::name`; update local tests.
- Modify `src/line_builder.rs`: remove `require_qualifiers`; always write plain output names; update line builder tests that currently expect `field:qualifier`.
- Modify `src/output.rs`: remove dual line builders and writer qualifier routing; remove `FileReport.with_qualifiers`; update output tests.
- Modify `src/config_parser.rs`: remove `OutputConfiguration.with_qualifiers`; update constructor and tests.
- Modify `src/parser/windows_parsers.rs`: stop using `Qualifiers` to construct generated field names.
- Modify parser tests in `src/parser/*.rs`: remove the `with_qualifiers` constructor argument from `OutputConfiguration::new` calls and update CSV qualifier test expectations.
- Modify `src/field_mapping.rs` and `src/timeline.rs`: remove test-only `Qualifiers` usage and adjust `FieldName::new` calls.
- Modify `src/lib.rs`, `src/lib_py.rs`, and `src/errors.rs`: remove the `qualifiers` module/export and `UnknownQualifier`.
- Delete `src/qualifiers.rs`.
- Modify `dfir_ogre_common.pyi`: remove `with_qualifiers`, `Qualifiers`, `FieldName(..., qualifier=...)`, and `FieldName.name(with_qualifier)`.

---

### Task 1: Make XML Qualifier Attributes Free-Form And Ignored

**Files:**
- Modify: `src/configuration.rs`

- [ ] **Step 1: Add a failing configuration test**

Add this test inside `#[cfg(test)] mod tests` in `src/configuration.rs`, near the other `from_str_*` configuration tests:

```rust
#[test]
fn from_str_accepts_free_form_qualifier_attributes() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <fields>
         <field input="Known" output="known" parser="String" qualifier="DATE_CREATION" />
         <object input="Container" output="container" qualifier="ANY_OBJECT_LABEL">
            <field input="Child" output="child" parser="String" qualifier="ANY_CHILD_LABEL" />
         </object>
         <multi_input output="joined" qualifier="ANY_MULTI_LABEL" parser="Join" separator="-">
            <field input="First" parser="String" qualifier="ANY_FIRST_LABEL" />
            <field input="Second" parser="String" qualifier="ANY_SECOND_LABEL" />
         </multi_input>
      </fields>
   </mapping>
</plugin>"#;

    let config = PluginConfiguration::from_str(xml, None, None).unwrap();
    let mapping = config.data_type_configs[0]
        .field_mapping
        .as_ref()
        .expect("field mapping");

    assert_eq!(mapping.fields.len(), 3);
    assert_eq!(mapping.fields[0].output_name(), "known");
    assert_eq!(mapping.fields[1].output_name(), "container");
    assert_eq!(mapping.fields[2].output_name(), "joined");
}
```

- [ ] **Step 2: Run the new test and confirm current failure**

Run:

```bash
cargo test configuration::tests::from_str_accepts_free_form_qualifier_attributes -- --exact
```

Expected result before implementation:

```text
test configuration::tests::from_str_accepts_free_form_qualifier_attributes ... FAILED
```

The failure should come from an unknown qualifier such as `ANY_OBJECT_LABEL`.

- [ ] **Step 3: Stop passing `Qualifiers` through XML parsing**

In `src/configuration.rs`, remove `Qualifiers` from the top-level `use crate::{...}` list:

```rust
use crate::{
    DateInputCodec, Error, Field, FieldMapping, FieldName, FieldParserTree, MultiInputField,
    MultiParser, Parser, TimeLineBuilder, TimeLineType,
    field::{ArrayField, ParserExtension, PyParser},
    timeline::{ConditionalDescriptionConf, TimelineDisplayOptions},
};
```

In `PluginConfiguration::from_str`, remove the registry construction and call `parse_mapping` without it:

```rust
let root = Element::parse(xml.as_bytes())?;
```

and:

```rust
let mapping = parse_mapping(element, &python, &extension)?;
```

Change `parse_mapping` to:

```rust
fn parse_mapping(
    node: &Element,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
) -> Result<DataTypeMapping, Error> {
```

Change the `parse_field_node` call inside `parse_mapping` to:

```rust
let field = parse_field_node(
    elem,
    &config.default_date_pattern,
    python,
    extension,
    &mut contains_primary_key,
)?;
```

Update `parse_field_node`, `parse_field`, `parse_object`, `parse_array`, and `parse_multi_input` signatures to remove the `qualifiers: &Qualifiers` parameter, and remove that argument from all calls between these functions.

- [ ] **Step 4: Ignore qualifier attributes when building field names**

Replace `parse_field_name` in `src/configuration.rs` with:

```rust
fn parse_field_name(elem: &Element) -> Result<FieldName, Error> {
    let attributes = &elem.attributes;
    let input_name = attribute("input", elem)?;
    let primary_key = attributes.get("primary_key").cloned();

    let primary_key = primary_key.is_some();
    let output_name = attributes.get("output").cloned();
    let display_name = attributes.get("display_name").cloned();
    let description = attributes.get("description").cloned();

    Ok(FieldName::new(
        input_name,
        primary_key,
        output_name,
        None,
        display_name,
        description,
    ))
}
```

This still passes `None` for the old qualifier argument because `FieldName::new` is removed in a later task.

In `parse_field`, call:

```rust
let name = parse_field_name(elem)?;
```

In `parse_object`, call:

```rust
let field_name = parse_field_name(elem)?;
```

In `parse_multi_input`, remove qualifier lookup and build the output field with a `None` qualifier:

```rust
let output_field = FieldName::new(
    output_name.clone(),
    false,
    Some(output_name),
    None,
    display_name,
    description,
);
```

- [ ] **Step 5: Run the configuration test**

Run:

```bash
cargo test configuration::tests::from_str_accepts_free_form_qualifier_attributes -- --exact
```

Expected result:

```text
test configuration::tests::from_str_accepts_free_form_qualifier_attributes ... ok
```

- [ ] **Step 6: Commit Task 1**

```bash
git add src/configuration.rs
git commit -m "config: accept free-form qualifier attributes"
```

---

### Task 2: Remove Qualified Field Names From FieldName And LineBuilder

**Files:**
- Modify: `src/field.rs`
- Modify: `src/line_builder.rs`
- Modify: `src/field_mapping.rs`
- Modify: `src/timeline.rs`
- Modify: `src/parser/windows_parsers.rs`
- Modify: `src/output.rs`
- Modify: `src/format_csv.rs`
- Modify: parser test files containing `FieldName::new(...)`

- [ ] **Step 1: Remove qualifier state from `FieldName`**

In `src/field.rs`, replace the `FieldName` struct with:

```rust
pub struct FieldName {
    pub in_name: String,
    pub out_name: String,
    pub primary_key: bool,
    pub display_name: Option<String>,
    pub description: Option<String>,
}
```

Replace the `FieldName::new` constructor and `name` method with:

```rust
#[new]
#[pyo3(signature = (input_name, primary_key=false, output_name=None, display_name=None, description=None))]
pub fn new(
    input_name: String,
    primary_key: bool,
    output_name: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
) -> Self {
    let output_name = output_name.unwrap_or(input_name.clone());

    FieldName {
        in_name: input_name,
        primary_key,
        out_name: output_name,
        description,
        display_name,
    }
}

/// Returns the plain output field name.
pub fn name(&self) -> &str {
    &self.out_name
}
```

Replace `Field::name` with:

```rust
pub fn name(&self) -> &str {
    match self {
        Field::Single {
            name,
            parser: _,
            default_value: _,
        } => name.name(),
        Field::Multi(f) => f.output_field.name(),
        Field::Object {
            name,
            fields: _,
            ignore: _,
        } => name.name(),
        Field::Array(field) => field.0.name(),
    }
}
```

- [ ] **Step 2: Remove qualifier mode from `LineData` and `LineBuilder`**

In `src/line_builder.rs`, remove `pub require_qualifiers: bool` from `LineData`.

Change `LineData::new` to:

```rust
fn new(compute_hash: bool, has_primary_keys: bool) -> Self {
    Self {
        has_primary_keys,
        data: Record::new(),
        data_id: None,
        timeline: Vec::new(),
        data_id_hasher: Hasher::new(),
        compute_hash,
    }
}
```

Change `LineData::with_capacity` to:

```rust
fn with_capacity(size: usize, compute_hash: bool, has_primary_keys: bool) -> Self {
    Self {
        has_primary_keys,
        data: Record::with_capacity(size),
        data_id: None,
        timeline: Vec::new(),
        data_id_hasher: Hasher::new(),
        compute_hash,
    }
}
```

In `LineData::add_data`, replace:

```rust
self.data
    .add(field_name.name(self.require_qualifiers), value);
```

with:

```rust
self.data.add(field_name.name(), value);
```

Change `LineBuilder::new` signature to remove `require_qualifiers`:

```rust
pub fn new(
    mut metadata: Metadata,
    timeline_builder: Option<TimeLineBuilder>,
    field_mapping: FieldMapping,
    compute_hash: bool,
    has_primary_keys: bool,
    force_snake_case: bool,
) -> Self {
```

Inside that constructor, build line data with:

```rust
line_data: LineData::new(compute_hash, has_primary_keys),
```

In nested object/array/unmapped handling, replace each `LineData::with_capacity(...)` call with the new three-argument form. For example:

```rust
let mut inner_insert = LineData::with_capacity(field_mapping.len(), false, false);
```

and:

```rust
let mut inner_insert = LineData::with_capacity(fields.len(), false, false);
```

- [ ] **Step 3: Update production call sites for new signatures**

Use this search to find every remaining old call shape:

```bash
rg -n "FieldName::new\\(|LineBuilder::new\\(|\\.name\\((true|false)" src
```

Apply these concrete transformations:

```rust
FieldName::new("name".to_owned(), false, None, None, None, None)
```

becomes:

```rust
FieldName::new("name".to_owned(), false, None, None, None)
```

```rust
FieldName::new(
    "input".to_owned(),
    false,
    Some("output".to_owned()),
    Some(qualifiers.APP_ID),
    None,
    Some("description".to_owned()),
)
```

becomes:

```rust
FieldName::new(
    "input".to_owned(),
    false,
    Some("output".to_owned()),
    None,
    Some("description".to_owned()),
)
```

```rust
LineBuilder::new(metadata, timeline, field_mapping, true, compute_hash, has_primary_key, force_snake_case)
```

becomes:

```rust
LineBuilder::new(metadata, timeline, field_mapping, compute_hash, has_primary_key, force_snake_case)
```

```rust
field.name(false)
```

and:

```rust
field.name(true)
```

both become:

```rust
field.name()
```

In `src/parser/windows_parsers.rs`, remove `Qualifiers` from the import list and construct generated fields without qualifiers:

```rust
let sequence = FieldName::new(format!("{prefix}sequence_number"), false, None, None, None);
let record = FieldName::new(format!("{prefix}record_number"), false, None, None, None);
```

and:

```rust
let md5 = FieldName::new("file_pe_md5".to_string(), false, None, None, None);
let sha1 = FieldName::new("file_pe_sha1".to_string(), false, None, None, None);
let sha256 = FieldName::new("file_pe_sha256".to_string(), false, None, None, None);
```

- [ ] **Step 4: Update line builder tests to assert plain names**

In `src/line_builder.rs`, remove `Qualifiers` from the test import:

```rust
use crate::{
    DateInputCodec, FieldName, Parser,
    field::{ArrayField, Field},
    parse_date,
    timeline::TimeLineType,
};
```

Replace qualifier-backed assertions with plain names:

```rust
assert!(record.contains_key("output_greeting"));
assert!(record.contains_key("year"));
assert!(record.contains_key("no_values"));
```

In `nested_mapping`, assert:

```rust
assert!(record.contains_key("output_greeting"));

let lvl1 = match record.get("lvl1_output").unwrap() {
    Value::Object(val) => &val.0,
    _ => panic!("expected an Object"),
};

let lvl2 = match lvl1.get("lvl2_output").unwrap() {
    Value::Object(val) => &val.0,
    _ => panic!("expected an Object"),
};

assert!(lvl2.contains_key("lvl2_greeting"));
```

In `integer_array`, read:

```rust
let array = line_builder.line_data.data.get("int_array").unwrap();
```

Rename `nested_object_array_with_qualifiers` to:

```rust
fn nested_object_array_uses_plain_output_names()
```

and assert:

```rust
let array = line_builder.line_data.data.get("array").unwrap();
```

and:

```rust
record.0.get("some_str").unwrap();
```

In `mapped_array_objects_preserve_renamed_and_unmapped_fields`, assert:

```rust
assert_eq!(
    first.get("item_name"),
    Some(&Value::String("first".to_owned()))
);
```

- [ ] **Step 5: Update field mapping and timeline tests**

In `src/field_mapping.rs`, remove `Qualifiers` from the test import:

```rust
use crate::{DateInputCodec, field::ArrayField};
```

In the `field_mapping()` helper, remove qualifier arguments from each `FieldName::new` call. For example:

```rust
FieldName::new(
    "timestamp".to_owned(),
    false,
    None,
    None,
    Some("Event generation time".to_owned()),
)
```

In `src/timeline.rs`, remove `Qualifiers` from the test import and remove qualifier arguments from `FieldName::new` calls. For example:

```rust
let lvl2_name = FieldName::new(
    "lvl2".to_owned(),
    false,
    Some("lvl2_output".to_owned()),
    None,
    None,
);
```

- [ ] **Step 6: Run compile-guided cleanup**

Run:

```bash
cargo test line_builder::tests field_mapping::tests timeline::tests --no-fail-fast
```

Expected first result while editing:

```text
error[E0061]: this function takes 5 arguments but 6 arguments were supplied
```

Use each compiler location to remove the obsolete qualifier argument. Repeat until the command exits successfully.

Expected final result:

```text
test result: ok.
```

- [ ] **Step 7: Commit Task 2**

```bash
git add src/field.rs src/line_builder.rs src/field_mapping.rs src/timeline.rs src/parser/windows_parsers.rs src/output.rs src/format_csv.rs
git commit -m "refactor: remove qualified field names"
```

If the compile-guided cleanup touched additional Rust files with `FieldName::new` calls, include those exact files in the same commit.

---

### Task 3: Remove `with_qualifiers` And Use One Output Builder

**Files:**
- Modify: `src/config_parser.rs`
- Modify: `src/output.rs`
- Modify: parser tests using `OutputConfiguration::new`
- Modify: `src/parser/csv.rs`

- [ ] **Step 1: Remove `with_qualifiers` from output configuration types**

In `src/config_parser.rs`, remove this field from `OutputConfiguration`:

```rust
#[pyo3(get)]
pub with_qualifiers: bool,
```

Update the constructor signature to:

```rust
#[pyo3(signature = ( base_file_name, output_folder, output_type="file".into(), format="jsonl".into(), date_format="iso_utc".into(), with_timeline=false,include_empty=true, params=HashMap::new()))]
pub fn new(
    base_file_name: String,
    output_folder: String,
    output_type: String,
    format: String,
    date_format: String,
    with_timeline: bool,
    include_empty: bool,
    params: HashMap<String, String>,
) -> Self {
    Self {
        output_type,
        format,
        date_format,
        with_timeline,
        include_empty,
        output_folder,
        base_file_name,
        params,
    }
}
```

Update `output_configuration_new_stores_all_fields` so the constructor call omits the old boolean and the assertion no longer reads `config.with_qualifiers`:

```rust
let config = OutputConfiguration::new(
    "events".to_string(),
    "/tmp/output".to_string(),
    "gzip".to_string(),
    "normalized_jsonl".to_string(),
    "iso".to_string(),
    true,
    false,
    params.clone(),
);

assert!(config.with_timeline);
assert!(!config.include_empty);
```

- [ ] **Step 2: Remove `with_qualifiers` from file reports**

In `src/output.rs`, remove this field from `FileReport`:

```rust
pub with_qualifiers: bool,
```

When constructing `file_report`, remove:

```rust
with_qualifiers: output_conf.with_qualifiers,
```

- [ ] **Step 3: Simplify `Output` to one `LineBuilder`**

In `src/output.rs`, replace the `Output` struct with:

```rust
#[derive(Clone)]
#[pyclass(from_py_object)]
pub struct Output {
    outputs: Vec<OutputWriter>,
    line_builder: LineBuilder,
}
```

In `Output::new`, remove:

```rust
let mut compute_with_qualifiers = false;
let mut compute_without_qualifiers = false;
```

and remove the branch that checks `output_conf.with_qualifiers`.

Change:

```rust
outputs.push((output_writer, output_conf.with_qualifiers))
```

to:

```rust
outputs.push(output_writer)
```

Replace the two-builder construction block with:

```rust
let line_builder = LineBuilder::new(
    metadata,
    timeline_builder,
    field_mapping,
    compute_hash,
    data_type_conf.has_primary_key,
    run_config.force_snake_case,
);
```

Write metadata with:

```rust
for writer in &mut outputs {
    writer.write_metadata(&line_builder)?;
}
```

Return:

```rust
Ok(Self {
    outputs,
    line_builder,
})
```

Replace `Output::write` with:

```rust
pub fn write(&mut self, data: &mut Record) -> Result<(), Error> {
    self.line_builder.build(data)?;

    for output in &mut self.outputs {
        output.write(&self.line_builder)?;
    }

    data.0.clear();
    Ok(())
}
```

Replace loops over `(out, _)` with loops over `out`. For example:

```rust
for out in &self.outputs {
    report.file_reports.push(out.result());
}
```

and:

```rust
for out in &mut self.outputs {
    let _ = out.close();
}
```

- [ ] **Step 4: Update output tests**

In `src/output.rs`, change `json_output_config` to remove the `with_qualifiers` parameter:

```rust
fn json_output_config(
    base_file_name: &str,
    output_folder: &Path,
    params: HashMap<String, String>,
) -> OutputConfiguration {
    OutputConfiguration::new(
        base_file_name.to_string(),
        output_folder.display().to_string(),
        "file".to_string(),
        "jsonl".to_string(),
        "iso".to_string(),
        false,
        true,
        params,
    )
}
```

Update calls such as:

```rust
json_output_config("bad_compression", &output_folder, false, params)
```

to:

```rust
json_output_config("bad_compression", &output_folder, params)
```

Replace `mixed_qualifier_outputs_use_separate_line_builders` with:

```rust
#[test]
fn multiple_json_outputs_receive_plain_records() {
    let output_folder = test_output_folder("output_multiple_plain");
    let field_mapping = FieldMapping::new(
        vec![Field::Single {
            name: FieldName::new(
                "event".to_owned(),
                false,
                None,
                None,
                None,
            ),
            parser: Parser::String(),
            default_value: None,
        }],
        None,
    );
    let run_config = RunConfiguration::new(
        vec![
            json_output_config("first", &output_folder, HashMap::new()),
            json_output_config("second", &output_folder, HashMap::new()),
        ],
        true,
        None,
    );
    let plugin_config = mapped_plugin_config("events", field_mapping);
    let mut output = Output::new(
        run_config,
        plugin_config,
        Metadata::new("host-output".into()),
        None,
    )
    .unwrap();
    let mut record = Record::new();
    record.add("event", Value::String("login".to_owned()));

    output.write(&mut record).unwrap();
    for writer in &mut output.outputs {
        writer.close().unwrap();
    }
    drop(output);

    let first: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("first.events.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    let second: serde_json::Value = serde_json::from_str(
        fs::read_to_string(output_folder.join("second.events.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();

    assert_eq!(first["event"], "login");
    assert_eq!(second["event"], "login");

    remove_dir_if_exists(&output_folder);
}
```

- [ ] **Step 5: Update every `OutputConfiguration::new` call**

Run:

```bash
rg -n "OutputConfiguration::new\\(" src
```

For every constructor call, remove the argument immediately after `with_timeline`.

Examples:

```rust
OutputConfiguration::new(base, folder, ty, format, date_format, false, false, true, params)
```

becomes:

```rust
OutputConfiguration::new(base, folder, ty, format, date_format, false, true, params)
```

```rust
OutputConfiguration::new(base, folder, ty, format, date_format, false, true, true, params)
```

becomes:

```rust
OutputConfiguration::new(base, folder, ty, format, date_format, false, true, params)
```

After editing, this command must print no `with_qualifiers` references in Rust:

```bash
rg -n "with_qualifiers" src
```

Expected result:

```text
```

- [ ] **Step 6: Update the CSV qualifier fixture test**

In `src/parser/csv.rs`, rename:

```rust
fn ntfs_info_qualifiers_and_null_parse()
```

to:

```rust
fn ntfs_info_plain_names_and_null_parse()
```

Update its comment to describe plain field names, not qualified field names.

Change the output configuration call so the obsolete `with_qualifiers` argument is gone.

Replace:

```rust
let _subsystem = line
    .get("pe_subsystem:pe_subsystem")
    .unwrap()
    .as_str()
    .unwrap();

let file_path = line.get("file_path:file_path").unwrap().as_str().unwrap();
```

with:

```rust
let _subsystem = line.get("pe_subsystem").unwrap().as_str().unwrap();

let file_path = line.get("file_path").unwrap().as_str().unwrap();
```

- [ ] **Step 7: Run targeted output tests**

Run:

```bash
cargo test config_parser::tests output::tests parser::csv::tests::ntfs_info_plain_names_and_null_parse --no-fail-fast
```

Expected result:

```text
test result: ok.
```

- [ ] **Step 8: Commit Task 3**

```bash
git add src/config_parser.rs src/output.rs src/parser/csv.rs
git add src/parser src/format_csv.rs src/format_json.rs
git commit -m "refactor: remove qualifier output mode"
```

Only include parser files that were changed by `OutputConfiguration::new` call updates.

---

### Task 4: Remove Qualifier Registry, Error Variant, Python Export, And Type Hints

**Files:**
- Delete: `src/qualifiers.rs`
- Modify: `src/lib.rs`
- Modify: `src/lib_py.rs`
- Modify: `src/errors.rs`
- Modify: `dfir_ogre_common.pyi`
- Modify: `src/parser/hive.rs` if only commented qualifier references remain there

- [ ] **Step 1: Remove Rust module and exports**

Delete `src/qualifiers.rs`.

In `src/lib.rs`, remove:

```rust
mod qualifiers;
```

and:

```rust
pub use qualifiers::Qualifiers;
```

In `src/lib_py.rs`, remove `qualifiers::Qualifiers` from the `use crate::{...}` list and remove:

```rust
m.add_class::<Qualifiers>()?;
```

In `src/errors.rs`, remove:

```rust
#[error("Unknown Qualifier: '{0}'")]
UnknownQualifier(String),
```

- [ ] **Step 2: Remove stale commented qualifier examples**

Run:

```bash
rg -n "Qualifiers|UnknownQualifier|qualified_name|require_qualifiers|with_qualifiers" src
```

If `src/parser/hive.rs` only contains commented-out qualifier examples, delete the commented block that references `Qualifiers` so the command can reach a clean result.

Expected result after cleanup:

```text
```

- [ ] **Step 3: Update `dfir_ogre_common.pyi` FieldName**

Replace the `FieldName` class header and constructor area with:

```python
class FieldName:
    """Stores field metadata including input names, output names, and documentation."""

    def __init__(
        self,
        input_name: str,
        primary_key: bool = False,
        output_name: Optional[str] = None,
        display_name: Optional[str] = None,
        description: Optional[str] = None,
    ) -> None:
        """Initialize a FieldName instance.

        Args:
            input_name: The name of the field in the input data.
            primary_key: Whether the field contributes to primary-key based identifiers.
            output_name: The name of the field in the output data. If not provided, defaults to input_name.
            display_name: An optional short human-readable label.
            description: An optional human-readable description of the field.
        """
        ...
```

Replace:

```python
def name(self, with_qualifier: bool) -> str:
```

with:

```python
def name(self) -> str:
    """Return the plain output field name."""
    ...
```

- [ ] **Step 4: Update `dfir_ogre_common.pyi` output report and configuration**

Remove this from `FileReport`:

```python
with_qualifiers: bool
```

Remove this from `OutputConfiguration`:

```python
with_qualifiers: bool
```

Update `OutputConfiguration.__init__` to:

```python
def __init__(
    self,
    base_file_name: str,
    output_folder: str,
    output_type: str = "file",
    format: str = "jsonl",
    date_format: str = "iso_utc",
    with_timeline: bool = False,
    include_empty: bool = True,
    params: Dict[str, str] = {},
) -> None:
```

Remove the `with_qualifiers` argument description from the docstring.

- [ ] **Step 5: Remove `Qualifiers` from type hints**

Delete the entire `class Qualifiers:` block from `dfir_ogre_common.pyi`, from:

```python
class Qualifiers:
```

through the end of that class body.

After editing, run:

```bash
rg -n "with_qualifiers|Qualifiers|qualifier|qualified_name" dfir_ogre_common.pyi
```

Expected result:

```text
```

- [ ] **Step 6: Run cleanup searches**

Run:

```bash
rg -n "with_qualifiers|Qualifiers|UnknownQualifier|qualified_name|require_qualifiers|line_builder_with_qualifiers|line_builder_without_qualifiers" src dfir_ogre_common.pyi
```

Expected result:

```text
```

Run:

```bash
rg -n "qualifier" src dfir_ogre_common.pyi
```

Expected result:

```text
```

The XML fixtures under `test_data/` are allowed to keep `qualifier="..."`, so do not include `test_data/` in this cleanup search.

- [ ] **Step 7: Run compile check**

Run:

```bash
cargo test configuration::tests::from_str_accepts_free_form_qualifier_attributes -- --exact
```

Expected result:

```text
test configuration::tests::from_str_accepts_free_form_qualifier_attributes ... ok
```

- [ ] **Step 8: Commit Task 4**

```bash
git add src/lib.rs src/lib_py.rs src/errors.rs dfir_ogre_common.pyi
git add -u src/qualifiers.rs
git commit -m "refactor: remove qualifier registry"
```

If `src/parser/hive.rs` was changed to remove stale comments, include it in the same commit.

---

### Task 5: Full Verification And Final Cleanup

**Files:**
- Modify only files required by verification failures.

- [ ] **Step 1: Format the code**

Run:

```bash
cargo fmt
```

Then run:

```bash
cargo fmt --check
```

Expected result:

```text
```

with exit code `0`.

- [ ] **Step 2: Run the full Rust test suite**

Run:

```bash
cargo test
```

Expected result:

```text
test result: ok. 181 passed; 0 failed
```

The exact number may change after qualifier-specific test rewrites, but every Rust unit test and doc test must pass.

- [ ] **Step 3: Run strict clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected result:

```text
Finished
```

with exit code `0`.

- [ ] **Step 4: Run final qualifier cleanup searches**

Run:

```bash
rg -n "with_qualifiers|Qualifiers|UnknownQualifier|qualified_name|require_qualifiers|line_builder_with_qualifiers|line_builder_without_qualifiers" src dfir_ogre_common.pyi
```

Expected result:

```text
```

Run:

```bash
rg -n "qualifier" src dfir_ogre_common.pyi
```

Expected result:

```text
```

Run:

```bash
rg -n "qualifier=\"[^\"]+\"" test_data
```

Expected result: existing XML fixture hits remain, proving fixtures still exercise compatibility.

- [ ] **Step 5: Review the final diff**

Run:

```bash
git diff --stat
git diff -- src/configuration.rs src/field.rs src/line_builder.rs src/output.rs src/config_parser.rs src/lib.rs src/lib_py.rs src/errors.rs dfir_ogre_common.pyi
```

Confirm these properties in the diff:

- XML parser no longer calls a qualifier registry.
- `FieldName` has no qualifier fields.
- `OutputConfiguration` has no `with_qualifiers`.
- `Output` has one `LineBuilder`.
- Python type hints match the approved API breaks.
- XML fixtures were not stripped of `qualifier="..."`.

- [ ] **Step 6: Commit final verification fixes**

If formatting or verification required edits after Task 4, commit them:

```bash
git add .
git commit -m "test: update qualifier removal coverage"
```

Skip this commit if there are no changes after the previous task commits.
