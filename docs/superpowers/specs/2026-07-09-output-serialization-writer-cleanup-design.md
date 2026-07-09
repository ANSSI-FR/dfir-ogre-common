# Output Serialization Writer Cleanup Design

## Context

The output serialization writer branch introduced `JsonObjectWriter`, `JsonArrayWriter`, and
`CsvRowWriter` to centralize comma and delimiter handling. The review found that the direction is
sound, but a few call sites still duplicate escaping logic or allocate temporary `String` buffers
before copying serialized fragments into the final output.

One concrete correctness gap remains: `MetadataSerialized` now stores raw JSON for both JSON and
CSV formatters, but `Value::Metadata` still writes the raw metadata string inside CSV quotes without
CSV escaping. Formatter-specific CSV output is safe because it uses `CsvRowWriter::quoted_field`;
the generic `Value::Metadata` CSV path must do the same escaping.

This was verified against the current branch and `origin/main`: before the writer change, the CSV
formatter stored a CSV-escaped JSON string in `MetadataSerialized`; the current branch stores raw
JSON and escapes it when writing CSV fields. The cleanup must make the current invariant explicit:
`MetadataSerialized` contains raw JSON, and output-format escaping happens at the output boundary.

## Goals

- Fix CSV serialization of `Value::Metadata` so metadata JSON containing quotes remains parseable
  CSV.
- Extend the existing writer helpers enough to remove repeated temporary JSON buffers from the
  touched serialization paths.
- Replace repeated manual JSON string-field writing in metadata and timeline serialization with
  helper methods that own quoting and escaping.
- Keep the existing output shape and public serialization behavior, except for the corrected CSV
  escaping bug.
- Add focused tests that exercise the writer helpers and the metadata CSV regression.

## Non-Goals

- Do not migrate output generation to `serde_json` or the `csv` crate writer APIs.
- Do not consolidate all CSV header generation.
- Do not redesign `MetadataSerialized` beyond clarifying and enforcing its current raw-JSON
  invariant at the touched call sites.
- Do not change Python extension APIs or add Python tests.

## Architecture

`src/serialise.rs` remains the low-level serialization helper module. Its writers own structural
syntax only: JSON comma placement, JSON field names, CSV delimiter placement, and CSV quoting.
Callers remain responsible for deciding which fields to emit and for providing valid raw JSON when a
field is intentionally embedded as a JSON object.

`JsonObjectWriter` and `JsonArrayWriter` will gain fallible callback methods:

- `try_field_raw(name, write_value) -> Result<(), Error>`
- `try_element_raw(write_value) -> Result<(), Error>`

These methods insert the field or element prefix, call the supplied serializer directly against the
destination buffer, and propagate any existing `Error`. This avoids building a temporary
`String` for every nested value.

Fallible writes must not leave partial fragments behind. A fallible helper must capture the
destination buffer length before writing its prefix, and if the callback returns an error, truncate
the buffer back to that length and leave the writer's field or element count unchanged. Fallible
top-level serializers must also capture the caller buffer length on entry and truncate back to it
before returning an error, so callers never receive a partially serialized line.

`JsonObjectWriter` will also gain convenience methods for JSON string values:

- `string_field(name, value)`
- `optional_string_field(name, Option<&str>)`

These helpers will quote and escape values through the existing JSON escaping function. Raw JSON
fields, such as serialized metadata, will continue to use explicitly named raw methods so reviewers
can see where pre-serialized JSON is embedded.

`CsvRowWriter` remains the quoting authority for the touched CSV paths. The immediate metadata CSV
fix can use the existing CSV escaping behavior rather than adding a broader CSV abstraction.

## Component Changes

### `src/serialise.rs`

Add direct unit tests and extend the writer helper API. Tests must cover:

- empty object and array output,
- skipped fields and elements,
- JSON string field escaping,
- fallible callback propagation,
- CSV quoted field escaping and optional empty fields.

### `src/value.rs`

Update `Value::json_serialise_record` and `Value::Array` JSON serialization to write nested values
directly through the new fallible writer callbacks. The skip-null behavior controlled by
`include_empty` stays unchanged.

Fix `Value::Metadata` in `csv_serialise_value` so it writes a quoted CSV field with escaped embedded
quotes. This makes generic metadata CSV serialization match the safer formatter-specific path.

### `src/format_json.rs`

Update `json_serialise_record_with_extra_field` to serialize record values directly into
`JsonObjectWriter` through the new fallible field callback. The extra metadata field remains raw JSON
when it is a `MetadataSerialized` value and a JSON string when it is an ID.

### `src/metadata.rs`

Replace manual JSON string-field writing with the new `JsonObjectWriter` string helpers. Optional
string fields must use `optional_string_field`, and optional dates may keep a small local helper
that writes a JSON string only when the date exists.

### `src/timeline.rs`

Replace repeated manual JSON string-field writing with the new `JsonObjectWriter` string helpers.
Serialized metadata and serialized data remain explicit raw JSON fields. Timeline CSV behavior stays
unchanged except for any incidental use of existing `CsvRowWriter` helpers.

## Data Flow

Records, metadata, and timeline objects continue to serialize into caller-owned `String` buffers.
The writer helpers do not allocate output buffers for nested values. They insert separators and then
let nested serializers write directly into the same destination.

Raw serialized metadata remains stored as raw JSON in `MetadataSerialized`. JSON output embeds it as
raw JSON. CSV output quotes it as a CSV field and escapes embedded quotes at the point where it is
written to CSV. No call site may store CSV-escaped content inside `MetadataSerialized`.

## Error Handling

No new error variants are required. Fallible writer helpers return `Result<(), Error>` and propagate
errors from nested serialization callbacks. The helpers are crate-local and may use the crate's
`Error` type directly; a generic error abstraction is unnecessary for this module.

When a nested serializer returns an error, the line serialization must return that error and clean
the output buffer back to the length it had before the failed serialization started. The implementation
must use `String::truncate` for rollback rather than leaving a malformed partial object, array, or
CSV line in the caller-provided buffer.

## Testing

Required validation:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

Focused regression tests:

- `Value::Metadata` CSV serialization must parse correctly when the raw metadata JSON contains
  embedded quotes. This test must parse the output with `csv::ReaderBuilder` and assert the parsed
  field equals the raw metadata JSON.
- JSON record and array serialization must continue omitting nulls without dangling commas when
  `include_empty` is false.
- A deliberately failing nested serializer callback must return an error and leave the destination
  buffer exactly as it was before the failed write started.
- Metadata and timeline JSON serialization must still parse and preserve escaped values.

## Rollout

The implementation must be a targeted refactor on a non-`main` branch. It must keep changes
limited to `src/serialise.rs`, `src/value.rs`, `src/format_json.rs`, `src/metadata.rs`,
`src/timeline.rs`, and tests colocated in those files unless verification exposes a directly related
issue.
