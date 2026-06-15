# Output Field Traversal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the output path so `LineBuilder` uses one private traversal helper for mapped fields and unmapped values while preserving current behavior.

**Architecture:** Add `src/field_traversal.rs` as a crate-private traversal driver. `field_traversal` owns the recursive walk over mapped `Field` shapes and remaining unmapped values; `LineBuilder` keeps all output, timeline, null, snake-case, and hash decisions through a small handler adapter.

**Tech Stack:** Rust 2024, existing `Field`, `Record`, `Value`, `LineBuilder`, `TimeLineBuilder`, `cargo test`, `cargo fmt`, `cargo clippy`.

---

## Scope And Branch

All work happens on the existing local branch:

```bash
git branch --show-current
```

Expected:

```text
refactor/output-field-traversal
```

No remote push is allowed.

## File Structure

- Create: `src/field_traversal.rs`
  - Owns crate-private traversal over `Field` shapes and remaining unmapped `Record` values.
  - Defines `FieldTraversalHandler` and `traverse_fields`.
- Modify: `src/lib.rs`
  - Adds private module declaration `mod field_traversal;`.
  - Does not add a public export.
- Modify: `src/line_builder.rs`
  - Adds characterization tests.
  - Adds a private `LineBuildTraversal` adapter implementing `FieldTraversalHandler`.
  - Replaces `LineBuilder::build_record`'s direct field-shape loop with `field_traversal::traverse_fields`.
  - Keeps `LineBuilder` helper methods responsible for output and timeline behavior.

---

### Task 1: Add Output Traversal Characterization Tests

**Files:**
- Modify: `src/line_builder.rs`

- [ ] **Step 1: Confirm branch and clean status**

Run:

```bash
git branch --show-current
git status --short
```

Expected:

```text
refactor/output-field-traversal
```

`git status --short` may show the plan file if this plan has not been committed yet. It must not show unrelated runtime edits.

- [ ] **Step 2: Add characterization tests to `src/line_builder.rs`**

Append these tests inside the existing `#[cfg(test)] mod tests` block, before `fn primary_key_mapping()`:

```rust
    #[test]
    fn mapped_nested_object_does_not_invent_missing_child_fields() {
        let mapping = FieldMapping::new(
            vec![Field::Object {
                name: FieldName::new("details".to_owned(), false, None, None, None, None),
                ignore: false,
                fields: vec![
                    Field::Single {
                        name: FieldName::new("present".to_owned(), false, None, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    },
                    Field::Single {
                        name: FieldName::new("missing".to_owned(), false, None, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    },
                ],
            }],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            mapping,
            false,
            false,
            false,
            false,
        );
        let mut details = Record::new();
        details.add("present", Value::String("kept".to_owned()));
        let mut record = Record::new();
        record.add("details", Value::Object(details));

        line_builder.build(&mut record).unwrap();

        let details = match line_builder.line_data.data.get("details").unwrap() {
            Value::Object(details) => details,
            value => panic!("expected details object, got {value:?}"),
        };
        assert_eq!(
            details.get("present"),
            Some(&Value::String("kept".to_owned()))
        );
        assert!(!details.contains_key("missing"));
    }

    #[test]
    fn ignored_object_mapping_skips_output() {
        let mapping = FieldMapping::new(
            vec![Field::Object {
                name: FieldName::new("details".to_owned(), false, None, None, None, None),
                ignore: true,
                fields: vec![Field::Single {
                    name: FieldName::new("hidden".to_owned(), false, None, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                }],
            }],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            mapping,
            false,
            false,
            false,
            false,
        );
        let mut details = Record::new();
        details.add("hidden", Value::String("secret".to_owned()));
        let mut record = Record::new();
        record.add("details", Value::Object(details));

        line_builder.build(&mut record).unwrap();

        assert!(!line_builder.line_data.data.contains_key("details"));
    }

    #[test]
    fn mapped_array_objects_preserve_renamed_and_unmapped_fields() {
        let mapping = FieldMapping::new(
            vec![Field::Array(ArrayField::new(Field::Object {
                name: FieldName::new("items".to_owned(), false, None, None, None, None),
                ignore: false,
                fields: vec![Field::Single {
                    name: FieldName::new(
                        "parsed_name".to_owned(),
                        false,
                        Some("item_name".to_owned()),
                        None,
                        None,
                        None,
                    ),
                    parser: Parser::String(),
                    default_value: None,
                }],
            }))],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            mapping,
            false,
            false,
            false,
            true,
        );
        let mut item = Record::new();
        item.add("item_name", Value::String("first".to_owned()));
        item.add("ExtraField", Value::Bool(true));
        let mut record = Record::new();
        record.add("items", Value::Array(vec![Value::Object(item)]));

        line_builder.build(&mut record).unwrap();

        let items = match line_builder.line_data.data.get("items").unwrap() {
            Value::Array(items) => items,
            value => panic!("expected items array, got {value:?}"),
        };
        let first = match &items[0] {
            Value::Object(first) => first,
            value => panic!("expected first item object, got {value:?}"),
        };
        assert_eq!(
            first.get("item_name"),
            Some(&Value::String("first".to_owned()))
        );
        assert_eq!(first.get("extra_field"), Some(&Value::Bool(true)));
    }

    #[test]
    fn unmapped_nested_objects_respect_force_snake_case() {
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut inner = Record::new();
        inner.add("InnerValue", Value::String("nested".to_owned()));
        let mut record = Record::new();
        record.add("OuterObject", Value::Object(inner));

        line_builder.build(&mut record).unwrap();

        let outer = match line_builder.line_data.data.get("outer_object").unwrap() {
            Value::Object(outer) => outer,
            value => panic!("expected outer object, got {value:?}"),
        };
        assert_eq!(
            outer.get("inner_value"),
            Some(&Value::String("nested".to_owned()))
        );
    }

    #[test]
    fn unmapped_null_is_preserved_without_timeline() {
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut record = Record::new();
        record.add("MissingValue", Value::Null());

        line_builder.build(&mut record).unwrap();

        assert_eq!(
            line_builder.line_data.data.get("missing_value"),
            Some(&Value::Null())
        );
    }

    #[test]
    fn timeline_build_records_unmapped_date() {
        let codec = DateInputCodec::Iso();
        let date = parse_date("2020-01-01T00:00:00Z", &codec).unwrap();
        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "test_data".to_owned(),
            usize::MAX,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            Some(timeline_builder),
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut record = Record::new();
        record.add("UnmappedDate", Value::Date(date));

        line_builder.build(&mut record).unwrap();

        assert_eq!(line_builder.line_data.timeline.len(), 1);
        assert_eq!(
            line_builder.line_data.timeline[0].timestamp_meaning,
            "unmapped_date"
        );
        assert!(line_builder.line_data.data.contains_key("unmapped_date"));
    }
```

- [ ] **Step 3: Run the characterization tests**

Run:

```bash
cargo test line_builder
```

Expected: all `line_builder` tests pass, including the six new characterization tests.

- [ ] **Step 4: Commit the characterization tests**

Run:

```bash
git add src/line_builder.rs
git commit -m "test: lock output traversal behavior"
```

Expected: one local commit containing only `src/line_builder.rs` test additions.

---

### Task 2: Add Private Field Traversal Driver

**Files:**
- Create: `src/field_traversal.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/field_traversal.rs`**

Add this file:

```rust
use crate::{Field, FieldName, Record, Value, errors::Error};

pub(crate) trait FieldTraversalHandler {
    fn visit_flat_field(
        &mut self,
        input_data: &mut Record,
        field_name: &FieldName,
        ignore: bool,
        root: bool,
    );

    fn visit_object_field(
        &mut self,
        input_data: &mut Record,
        field_name: &FieldName,
        fields: &[Field],
        ignore: bool,
        force_snake_case: bool,
    ) -> Result<(), Error>;

    fn visit_array_field(
        &mut self,
        input_data: &mut Record,
        inner_field: &Field,
        force_snake_case: bool,
    ) -> Result<(), Error>;

    fn visit_unmapped_field(
        &mut self,
        key: String,
        value: Value,
        force_snake_case: bool,
    ) -> Result<(), Error>;
}

pub(crate) fn traverse_fields<H>(
    input_data: &mut Record,
    fields: &[Field],
    handler: &mut H,
    force_snake_case: bool,
    root: bool,
) -> Result<(), Error>
where
    H: FieldTraversalHandler,
{
    for field in fields {
        match field {
            Field::Single {
                name,
                parser: _,
                default_value: _,
            } => {
                handler.visit_flat_field(input_data, name, field.ignore(), root);
            }
            Field::Multi(multi_input_field) => {
                handler.visit_flat_field(
                    input_data,
                    &multi_input_field.output_field,
                    field.ignore(),
                    root,
                );
            }
            Field::Array(array_field) => {
                handler.visit_array_field(input_data, array_field.0.as_ref(), force_snake_case)?;
            }
            Field::Object {
                name,
                fields,
                ignore,
            } => {
                handler.visit_object_field(
                    input_data,
                    name,
                    fields,
                    *ignore,
                    force_snake_case,
                )?;
            }
        }
    }

    let unmapped: Vec<(String, Value)> = input_data.drain().collect();
    for (key, value) in unmapped {
        handler.visit_unmapped_field(key, value, force_snake_case)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Register the private module in `src/lib.rs`**

Add this line after `mod field_mapping;`:

```rust
mod field_traversal;
```

The surrounding module list should include:

```rust
mod field;
mod field_mapping;
mod field_traversal;
mod format_csv;
```

- [ ] **Step 3: Verify the module compiles before wiring**

Run:

```bash
cargo test --no-run
```

Expected: compilation fails with warnings only if `field_traversal` is unused under strict settings, or succeeds. If it fails because `field_traversal` is unused, continue to Task 3 where it is wired. If it fails for any type or import error, fix `src/field_traversal.rs` before continuing.

- [ ] **Step 4: Commit the traversal module**

Run:

```bash
git add src/field_traversal.rs src/lib.rs
git commit -m "refactor: add output field traversal driver"
```

Expected: one local commit containing the new private traversal module and private module registration.

---

### Task 3: Wire `LineBuilder::build_record` Through The Traversal Driver

**Files:**
- Modify: `src/line_builder.rs`

- [ ] **Step 1: Add the traversal import**

In the top `use crate::{ ... };` block in `src/line_builder.rs`, add `field_traversal::{self, FieldTraversalHandler},`.

The block should become:

```rust
use crate::{
    Field, FieldMapping, FieldName, Metadata, Record, Value,
    errors::Error,
    field_traversal::{self, FieldTraversalHandler},
    timeline::{TimeLine, TimeLineBuilder, TimelineField},
};
```

- [ ] **Step 2: Add the private adapter before `impl LineBuilder`**

Insert this code after the `pub struct LineBuilder` definition and before `impl LineBuilder`:

```rust
struct LineBuildTraversal<'a> {
    line_data: &'a mut LineData,
    timeline_builder: Option<&'a TimeLineBuilder>,
    timeline_fields: Option<&'a HashMap<String, TimelineField>>,
}

impl FieldTraversalHandler for LineBuildTraversal<'_> {
    fn visit_flat_field(
        &mut self,
        input_data: &mut Record,
        field_name: &FieldName,
        ignore: bool,
        root: bool,
    ) {
        LineBuilder::process_flat_field(
            input_data,
            self.line_data,
            self.timeline_builder,
            self.timeline_fields,
            field_name,
            ignore,
            root,
        );
    }

    fn visit_object_field(
        &mut self,
        input_data: &mut Record,
        field_name: &FieldName,
        fields: &[Field],
        ignore: bool,
        force_snake_case: bool,
    ) -> Result<(), Error> {
        LineBuilder::process_object_field_mapping(
            input_data,
            self.line_data,
            self.timeline_builder,
            self.timeline_fields,
            field_name,
            fields,
            ignore,
            force_snake_case,
        )
    }

    fn visit_array_field(
        &mut self,
        input_data: &mut Record,
        inner_field: &Field,
        force_snake_case: bool,
    ) -> Result<(), Error> {
        LineBuilder::process_array_field_mapping(
            input_data,
            self.line_data,
            inner_field,
            self.timeline_builder,
            self.timeline_fields,
            force_snake_case,
        )
    }

    fn visit_unmapped_field(
        &mut self,
        key: String,
        value: Value,
        force_snake_case: bool,
    ) -> Result<(), Error> {
        LineBuilder::process_unmapped_field(
            key,
            value,
            self.line_data,
            self.timeline_builder,
            self.timeline_fields,
            force_snake_case,
        )
    }
}
```

- [ ] **Step 3: Replace `LineBuilder::build_record` with traversal delegation**

Replace the full body of `LineBuilder::build_record` and change `field_mapping` from `&Vec<Field>` to `&[Field]`.

The complete function should be:

```rust
    pub fn build_record(
        input_data: &mut Record,
        line_data: &mut LineData,
        timeline_builder: Option<&TimeLineBuilder>,
        timeline_fields: Option<&HashMap<String, TimelineField>>,
        field_mapping: &[Field],
        force_snake_case: bool,
        root: bool,
    ) -> Result<(), Error> {
        let mut traversal = LineBuildTraversal {
            line_data,
            timeline_builder,
            timeline_fields,
        };

        field_traversal::traverse_fields(
            input_data,
            field_mapping,
            &mut traversal,
            force_snake_case,
            root,
        )
    }
```

- [ ] **Step 4: Update recursive empty mapping call**

In `process_unmapped_field`, replace:

```rust
                    &vec![],
```

with:

```rust
                    &[],
```

- [ ] **Step 5: Change object helper signature to accept a slice**

Change `process_object_field_mapping` parameter:

```rust
        field_mapping: &Vec<Field>,
```

to:

```rust
        field_mapping: &[Field],
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test line_builder
```

Expected: all `line_builder` tests pass.

- [ ] **Step 7: Commit the wiring**

Run:

```bash
git add src/line_builder.rs
git commit -m "refactor: route line builder through field traversal"
```

Expected: one local commit containing only `src/line_builder.rs` changes.

---

### Task 4: Run Parser Regression Tests

**Files:**
- No source edits expected.

- [ ] **Step 1: Run JSON parser tests**

Run:

```bash
cargo test parser::json
```

Expected: all JSON parser tests pass.

- [ ] **Step 2: Run XML parser tests**

Run:

```bash
cargo test parser::xml
```

Expected: all XML parser tests pass.

- [ ] **Step 3: Run field mapping nested-array regression test**

Run:

```bash
cargo test set_field_value_rejects_nested_array_parser
```

Expected: the existing `Error::UnsupportedNestingArray` regression test passes. This confirms the existing nested-array error surface outside `LineBuilder` remains intact.

- [ ] **Step 4: Check status**

Run:

```bash
git status --short
```

Expected: no source changes from the regression test runs.

---

### Task 5: Full Verification And Cleanup

**Files:**
- Modify only files touched by `cargo fmt` if formatting changes are required.

- [ ] **Step 1: Run full Rust test suite**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 2: Run formatting check**

Run:

```bash
cargo fmt --check
```

Expected: formatting check passes.

- [ ] **Step 3: Format if needed**

If `cargo fmt --check` reports formatting differences, run:

```bash
cargo fmt
```

Then run:

```bash
cargo fmt --check
```

Expected: formatting check passes.

- [ ] **Step 4: Run strict clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clippy completes with no warnings.

- [ ] **Step 5: Commit formatting-only changes if any exist**

Run:

```bash
git status --short
```

If only formatting changes exist, commit them:

```bash
git add src/line_builder.rs src/field_traversal.rs src/lib.rs
git commit -m "style: format output traversal refactor"
```

Expected: either no commit is needed, or one local formatting-only commit is created.

---

### Task 6: Final Local Review

**Files:**
- No source edits expected.

- [ ] **Step 1: Inspect local commit stack**

Run:

```bash
git log --oneline main..HEAD
```

Expected commits include:

```text
docs: design output field traversal refactor
test: lock output traversal behavior
refactor: add output field traversal driver
refactor: route line builder through field traversal
```

An additional formatting commit may appear if `cargo fmt` changed files.

- [ ] **Step 2: Inspect changed files**

Run:

```bash
git diff --stat main..HEAD
```

Expected changed files:

```text
docs/superpowers/specs/2026-06-15-output-field-traversal-design.md
docs/superpowers/plans/2026-06-15-output-field-traversal.md
src/field_traversal.rs
src/lib.rs
src/line_builder.rs
```

- [ ] **Step 3: Confirm no public API files changed**

Run:

```bash
git diff --name-only main..HEAD
```

Expected: output must not include `src/lib_py.rs` or `dfir_ogre_common.pyi`.

- [ ] **Step 4: Confirm clean working tree**

Run:

```bash
git status --short
```

Expected: no output.

- [ ] **Step 5: Prepare final summary**

Report:

```text
Branch: refactor/output-field-traversal
Changed runtime files: src/field_traversal.rs, src/lib.rs, src/line_builder.rs
Public Python API changed: no
Remote pushed: no
Verification: cargo test; cargo fmt --check; cargo clippy --all-targets --all-features -- -D warnings
```
