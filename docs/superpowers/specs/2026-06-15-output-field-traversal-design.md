# Output Field Traversal Design

## Summary

Unify the field-shape traversal used by the output path while preserving existing runtime behavior. This refactor is limited to the `LineBuilder` path that transforms parsed `Record` values into output data and timeline records.

The change should not alter parser behavior, public Rust exports, Python bindings, Python type hints, fixture formats, or output schemas.

## Scope

In scope:

- Centralize recursive traversal over mapped `Field` shapes for output building.
- Keep `LineBuilder` responsible for output insertion, timeline enrichment, null handling, snake-case conversion, and hash behavior.
- Preserve current `Record` mutation behavior: mapped fields are consumed from input data, and remaining fields are treated as unmapped.
- Add focused tests around existing `LineBuilder` behavior before changing internals.
- Perform all implementation work on the local branch `refactor/output-field-traversal`.

Out of scope:

- Refactoring `FieldParserTree` construction or lookup behavior.
- Refactoring JSON, XML, CSV, EVTX, Hive, SQLite, SRUM, or regexp parser extraction logic.
- Changing public Rust API exports from `src/lib.rs`.
- Changing Python bindings in `src/lib_py.rs` or `dfir_ogre_common.pyi`.
- Changing output schemas or fixture expectations unless an existing behavior is explicitly proven accidental and approved separately.
- Pushing changes to a remote repository.

## Architecture

Add one private traversal unit used only by the output path. The traversal unit walks mapped `Field` definitions and delegates output-specific decisions to `LineBuilder` or a small adapter owned by `LineBuilder`.

`LineBuilder` remains the owner of:

- `LineData` mutation.
- Timeline date and field enrichment.
- Root versus nested null behavior.
- `force_snake_case` handling for unmapped keys.
- Data hash and timeline hash behavior.
- Final timeline emission through `TimeLineBuilder`.

The traversal unit owns only:

- Visiting mapped fields in order.
- Distinguishing flat fields, multi-input outputs, object fields, array fields, and remaining unmapped values.
- Recursing into nested `Record` values for mapped and unmapped objects.
- Returning existing errors for unsupported traversal shapes.

The preferred file shape is:

- Keep `src/line_builder.rs` as the public home for `LineBuilder` and `LineData`.
- Add a private helper module at `src/field_traversal.rs`.
- Register the module privately in `src/lib.rs`.

## Component Boundaries

The traversal helper should expose a small crate-private API. It should not become a public abstraction.

Expected responsibilities:

- A traversal context accepts mutable input data, mapped output fields, and root/unmapped traversal settings.
- A handler or adapter receives traversal events and performs output-specific operations.
- `LineBuilder` handler methods process flat fields, object fields, array fields, and unmapped values using the same behavior as today.

The key boundary is:

- Traversal decides which field shape or remaining value is being visited.
- `LineBuilder` decides what to add to `LineData` and what to send to `TimeLineBuilder`.

This boundary avoids moving timeline semantics into `Field`, `FieldMapping`, or parser modules.

## Data Flow

The external flow remains unchanged:

1. Parser modules produce a mutable `Record`.
2. `Output::write` passes the record to one or two `LineBuilder` instances depending on qualifier output settings.
3. `LineBuilder::build` transforms the record into `LineData`.
4. Formatters write JSONL, normalized JSONL, CSV, or normalized CSV output.

The internal `LineBuilder::build` flow becomes:

1. Clear previous `LineData`.
2. Traverse mapped output fields first.
3. For flat and multi-input output fields, remove the field's output key from the input record and process the value if present.
4. For mapped objects, remove the object's output key and recursively transform nested object data when the value is an object.
5. For mapped arrays, remove the array output key and recursively transform object elements when the array contains objects.
6. Traverse remaining unmapped fields after mapped fields are consumed.
7. Apply `force_snake_case` to unmapped keys when configured.
8. Recursively normalize unmapped object values.
9. Emit timeline records through the existing timeline builder.
10. Preserve existing data-id and timeline-id hashing behavior.

## Behavior To Preserve

Root mapped fields still emit `Value::Null()` when the field is absent.

Nested mapped fields do not automatically invent absent child fields. This follows the current `root = false` behavior in `LineBuilder::build_record`.

Ignored object mappings skip output and consume the mapped input value.

Arrays of primitive values keep their current behavior: a missing or non-array value becomes an empty output array for the mapped array field.

Arrays of objects keep recursive field renaming and unmapped nested fields.

Unsupported nested arrays continue to return `Error::UnsupportedNestingArray`.

Unmapped nested objects continue to recurse through the output-building path.

Unmapped null values keep the current behavior: they are skipped when a timeline builder is active and preserved when no timeline builder is active.

Timeline enrichment still sees:

- mapped date fields,
- mapped timeline description fields,
- mapped array timeline fields,
- unmapped date fields,
- unmapped timeline fields when their keys match timeline definitions.

## Error Handling

Existing error surfaces should be preserved.

- `Error::UnsupportedNestingArray` remains the error for unsupported nested arrays.
- Timeline formatting errors still propagate from `TimeLineBuilder`.
- Output-building errors still return through `LineBuilder::build`.
- Missing mapped root fields produce nulls, not errors.
- Object mappings that receive a non-object value keep the current silent-skip behavior.
- Array object mappings that receive non-object elements skip those elements, matching current behavior.

No new public error variants are required for this refactor.

## Testing

Add focused tests around `LineBuilder` before changing internals. These tests should lock current behavior and make the refactor safe.

Required behavior tests:

- Mapped root fields emit nulls when absent.
- Mapped nested object fields do not invent absent child fields unexpectedly.
- Ignored object mappings skip output.
- Ignored array object mappings skip output.
- Arrays of objects preserve recursive field renaming and unmapped fields.
- Unmapped nested objects recurse and respect `force_snake_case`.
- Timeline enrichment still sees mapped dates and unmapped dates.
- Unsupported nested arrays return `Error::UnsupportedNestingArray`.

Verification commands:

- `cargo test line_builder`
- `cargo test parser::json`
- `cargo test parser::xml`
- `cargo test`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Compatibility

This refactor is successful only if existing public behavior remains compatible.

No changes should be made to:

- `src/lib.rs` public exports, except adding the private `field_traversal` module declaration.
- `src/lib_py.rs`.
- `dfir_ogre_common.pyi`.
- Parser function signatures.
- Output file names.
- JSON or CSV field names.
- Timeline output schema.

## Branch And Repository Constraints

Implementation and spec work happen on the local branch `refactor/output-field-traversal`.

No remote push is allowed.

Generated build outputs, wheels, local virtual environments, and machine-specific files must not be committed.
