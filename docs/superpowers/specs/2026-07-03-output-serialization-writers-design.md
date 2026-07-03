# Output Serialization Writers Design

## Goal

Fix two output serialization correctness issues and reduce the chance of similar bugs by centralizing the fragile delimiter and escaping logic used by JSON and CSV serialization.

Current issues:

- JSON arrays can become invalid when `include_empty = false` and an array contains `Value::Null`. The null value writes nothing, but comma placement still uses the original element index.
- Metadata JSON/CSV does not escape `computer` or `data_type`, while optional metadata strings are escaped.

This pass uses a small central writer API for serialization fragments. It does not refactor formatter file I/O, normalized output flow, timeline output, metadata caching, or report accounting.

## Scope

In scope:

- Fix JSON array serialization when null elements are omitted.
- Escape `Metadata.computer` and `Metadata.data_type` in JSON and CSV output.
- Add `src/serialise.rs` for JSON object/array delimiter handling and CSV field writing.
- Use the serializer helpers from `Value` and `Metadata`.
- Use the helpers from JSON/CSV extra-field code when it is local and straightforward.
- Add focused Rust unit tests for the two reported bugs and serializer behavior.

Out of scope:

- Replacing the full output stack with `serde_json` or the `csv` crate.
- Deduplicating `JsonFormatter` and `CsvFormatter` writer setup or control flow.
- Changing output filenames, normalized/non-normalized behavior, timeline behavior, metadata caching, or line counting.
- Changing metadata field order or CSV headers.
- Python tests.

## Serializer Module

Add `src/serialise.rs` and expose it inside the crate only.

The module owns string-fragment helpers, not output destinations. All APIs write into a caller-owned `String`.

JSON helpers:

- `JsonObjectWriter`
  - starts and ends `{...}`
  - tracks whether any field has been emitted
  - writes commas only between emitted fields
  - escapes field names
  - supports callers skipping fields before writing

- `JsonArrayWriter`
  - starts and ends `[...]`
  - tracks emitted element count rather than source index
  - writes commas only between values actually emitted

CSV helpers:

- write a quoted, CSV-escaped string field
- write an optional quoted string field, leaving an empty field when missing
- write delimiters consistently using the existing `CSV_DELIMITER`

The helpers should stay small and direct. They are not intended to model a full streaming serializer.

## JSON Behavior

`include_empty` semantics remain the same for records:

- object fields with `Value::Null` are omitted when `include_empty = false`
- object fields with `Value::Null` are emitted as `null` when `include_empty = true`

Array behavior becomes consistent with object behavior:

- array elements with `Value::Null` are omitted when `include_empty = false`
- array elements with `Value::Null` are emitted as `null` when `include_empty = true`

Examples when `include_empty = false`:

```text
[Null]              -> []
[Null, "a"]         -> ["a"]
["a", Null, "b"]    -> ["a","b"]
[1, Null, 2]        -> [1,2]
```

Nested arrays and objects follow the same recursive rule because the existing `Value::json_serialise_value` call path is preserved.

## Metadata Behavior

`Metadata.computer` and `Metadata.data_type` must be serialized through the same escaping rules as the optional metadata string fields.

JSON metadata:

- escape quotes, backslashes, newlines, tabs, and control characters in `computer`
- escape the same characters in `data_type`
- preserve existing field order
- keep optional fields omitted when absent

CSV metadata:

- quote and CSV-escape `computer`
- quote and CSV-escape `data_type`
- preserve existing header order and row order
- keep missing optional fields as empty cells

CSV rows may become more quoted than before for the first two metadata columns. This is acceptable because the parsed CSV values remain the same for ordinary strings and become correct for strings containing delimiters, quotes, or newlines.

## Formatter Interaction

`JsonFormatter` and `CsvFormatter` keep their current high-level flow:

- writer creation
- metadata writer creation
- timeline writer creation
- normalized and non-normalized write paths
- metadata caching
- report line counting
- close handling

The JSON/CSV extra-field helpers in `format_json.rs` and `format_csv.rs` should use the new serializer primitives for their local key/value or field quoting work. The implementation must not refactor formatter control flow while doing this.

## Tests

Add tests next to the changed code.

`src/value.rs`:

- JSON array containing only `Value::Null` with `include_empty = false` serializes as `[]`.
- JSON array with leading, middle, and trailing null elements serializes without invalid commas.
- Nested arrays/objects with omitted null values still parse with `serde_json`.
- Existing `include_empty = true` behavior continues to emit `null`.

`src/metadata.rs`:

- JSON metadata with quotes, backslashes, newlines, tabs, and control characters in `computer` and `data_type` parses with `serde_json` and returns the original field values, except for carriage returns which current JSON escaping drops.
- CSV metadata with delimiter characters, quotes, and newlines in `computer` and `data_type` parses with the `csv` crate into the original field values.
- Existing metadata ordering/header tests remain.

Formatter tests:

- Existing `JsonFormatter` and `CsvFormatter` tests should remain green.
- Add narrow formatter tests only if extra-field helper behavior changes.

## Validation

Required validation:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
LD_LIBRARY_PATH=/home/asalais/.local/share/uv/python/cpython-3.13.5-linux-x86_64-gnu/lib cargo test
```

Do not add or run Python unit tests in this repository.
