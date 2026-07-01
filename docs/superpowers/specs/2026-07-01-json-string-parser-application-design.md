# JSON String Parser Application Design

## Summary

Apply explicit configured field parsers to JSON string primitives before output building. This fixes JSON and EVTX fields mapped as `DateTime`, `Int`, `Bool`, Python parsers, and Rust extensions when the source value is a JSON string.

The primary confirmed bug is timeline extraction from JSON date strings. The JSON parser currently converts string primitives directly to `Value::String` and then calls `set_field_value`, which only stores an already-parsed `Value`. `LineBuilder` only creates timeline timestamps from `Value::Date`, so mapped date strings do not contribute timeline records.

EVTX is affected because it delegates event payload objects through `parser::json::parse_json_object`.

## Scope

In scope:

- Apply explicit mapped field parsers to JSON `String` primitive values.
- Keep native JSON `Number`, `Bool`, and `Null` conversion behavior unchanged.
- Preserve object and array traversal semantics.
- Cover EVTX through the shared JSON object parser, not through a separate EVTX-specific conversion path.
- Add regression tests for JSON timeline extraction and EVTX real fixture timeline extraction.

Out of scope:

- Changing Python-facing APIs.
- Changing output schemas or field names.
- Changing default-parser behavior for unmapped primitive fields.
- Reworking `FieldParserTree::set_field_value` for every caller.
- Applying configured parsers to native JSON numbers or booleans.
- Adding new fixture files when existing JSON and EVTX fixtures are sufficient.
- Pushing changes to a remote repository.

## Architecture

Keep the implementation local to `src/parser/json.rs`.

`parse_json_object` remains the shared entry point for JSON and EVTX object parsing. Before converting a mapped JSON string to `Value::String`, it should check whether the current `FieldParserTree` has an explicit `ParserType::Field` for that input name.

For explicit mapped string fields:

- Use `FieldParser::parse(Some(&value), record)` instead of `get_values` plus `set_field_value`.
- This preserves existing field parsing behavior, including output renaming, default-value handling, Python parser output, and Rust extension output.
- If parsing fails, return the existing parser error through the current `parse_json_object` error path.

For unmapped string fields:

- Keep the current behavior in this pass.
- Unknown/default parsing semantics are not broadened because that could change ignored-field behavior outside the confirmed bug.

For arrays:

- Preserve array traversal.
- When an array is explicitly mapped as an array of primitive fields, string elements should be parsed through the array field parser into array element values.
- Native JSON number, bool, and null array elements keep their current direct conversion.
- Python and Rust extension parsers inside primitive arrays are not expanded into multi-field array output in this pass because there is no existing one-to-many array schema for that behavior.

For objects:

- Preserve recursive calls to `parse_json_object`.
- Nested explicit string fields use the same parsing behavior because recursion passes the nested `FieldParserTree`.

## Data Flow

The updated mapped string flow is:

1. `parse_json` or `parse_jsonl` reads a JSON object, or EVTX extracts the event object.
2. `parse_json_object` iterates object fields.
3. For a JSON string with an explicit `ParserType::Field`, the field parser parses the raw string into the output record.
4. Parsed date strings become `Value::Date`, parsed numeric strings become `Value::Int` or `Value::Float`, parsed boolean strings become `Value::Bool`, and Python/Rust extension parsers can emit their configured output fields.
5. `Output::write` passes the record to `LineBuilder`.
6. `LineBuilder` sees mapped `Value::Date` values and creates timeline records as it already does for other parsers.

All other primitive values continue through the existing `get_values` and `set_field_value` path.

## Side-Effect Controls

The implementation should avoid changing behavior for values that are already correctly typed by JSON:

- JSON numbers stay `Value::Int` or `Value::Float`.
- JSON booleans stay `Value::Bool`.
- JSON null stays `Value::Null`.
- Explicit string parsing does not stringify numbers or booleans for parser coercion.

The implementation should also avoid moving parser application into `FieldParserTree::set_field_value`, because that method is used by other parsers that already pass parsed `Value`s.

Unknown/default parser behavior for primitive fields stays outside this change. If that behavior is incorrect, it should be handled as a separate, targeted fix with its own tests.

## Testing

Add tests that fail before the implementation and pass after it:

- JSON timeline regression using existing `test_data/json/json_file.json` and `test_data/json/json.xml`, with timeline output enabled. Assert that the mapped JSON date string produces a timeline record.
- JSON string primitive parser regression with a small test JSON file created under `.tmp`, mapping string values through `Int` and `Bool`. Assert output values are numeric and boolean, not strings.
- EVTX real-data regression using existing `test_data/evtx/kernel_pnp.evtx`. Use a test-only plugin configuration that ignores the synthetic top-level `timestamp` and maps `System.TimeCreated_attributes.SystemTime` as `DateTime` with a distinct display name. Assert timeline output contains that distinct timestamp meaning.

Existing tests should continue to cover native JSON number and boolean preservation through nested object and array parsing. Add explicit assertions only if the implementation touches those paths.

Verification commands:

- `cargo test parser::json`
- `cargo test parser::evtx`
- `cargo test`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Compatibility

No changes are expected to:

- Python context-manager output usage.
- `dfir_ogre_common.pyi`.
- Public parser function signatures.
- Existing output schemas.
- Existing fixture files.
- Native JSON number, bool, null, object, or non-string primitive behavior.

## Repository Constraints

Implementation and spec work happen on the current non-`main` local branch.

No remote push is allowed.

Generated build outputs, wheels, local virtual environments, and machine-specific files must not be committed.
