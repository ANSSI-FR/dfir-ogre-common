# Schema-Driven CSV Output Design

## Goal

Keep regular and normalized CSV rows aligned with their declared headers when a plugin emits top-level fields that are not present in its `FieldMapping`.

CSV output will serialize only the top-level record fields declared by the mapping. JSON output will continue to include unmapped fields, and mapped object or array values will retain their complete nested content.

## Current Behavior

`LineBuilder` constructs one ordered `Record` that is shared by every configured output writer. It places mapped fields first and preserves remaining unmapped fields afterward.

`CsvFormatter` builds its header from `FieldMapping::field_parser_tree.output_fields`, but it currently serializes every value in the shared record. An unmapped top-level field therefore creates an unlabeled CSV cell before the metadata or metadata-ID cell. The row becomes wider than the header and shifts the meaning of subsequent columns.

Filtering in `LineBuilder` is not appropriate because the same built record may also be sent to JSON output. Removing unmapped fields there would change JSON behavior and would also remove them from the JSON `data` cell used by non-normalized timeline CSV.

## Scope

In scope:

- Make regular CSV rows follow the mapped top-level field schema exactly.
- Make normalized CSV data rows follow the mapped top-level field schema exactly.
- Ignore unmapped top-level fields while serializing schema-driven CSV rows.
- Keep mapped fields in the same order as the CSV header.
- Emit an empty cell for a mapped field whose value is absent.
- Keep generated identifiers and metadata fields in their existing positions.
- Preserve all nested keys inside a mapped object or array value.
- Preserve existing JSON and timeline behavior.
- Preserve the existing headerless pass-through behavior when `CsvFormatter` is constructed without a `FieldMapping`.

Out of scope:

- Filtering unmapped fields from `LineBuilder` or JSON output.
- Recursively filtering nested object or array content.
- Dynamically extending CSV headers from plugin records.
- Rejecting records merely because they contain unmapped fields.
- Changing CSV filenames, delimiters, escaping, date formatting, or metadata serialization.
- Python tests.

## CSV Schema

`CsvFormatter` will derive an optional ordered list of record-backed column names when it is constructed:

- With a `FieldMapping`, start with each top-level `output_fields` name in mapping order.
- For normalized CSV, append `ogre_id` after the mapped fields.
- The final metadata column is not record-backed. It remains `ogre_md` for regular CSV and `ogre_md_id` for normalized CSV.
- Without a `FieldMapping`, retain the current behavior: write no header and serialize every record value in record order before the final metadata value.

The same ordered list must drive both header construction and row serialization. This prevents header and row ordering from diverging.

## Row Serialization

For schema-driven CSV output, row serialization will iterate the formatter's ordered record-backed column names rather than iterating the complete `Record`:

1. Look up each column name in the built record.
2. Serialize the value into one CSV cell when present.
3. Emit an empty CSV cell when absent.
4. Ignore every other top-level record entry.
5. Append the existing metadata value or metadata identifier as the final cell.

Regular CSV example:

```text
Mapping: mapped
Plugin record: mapped=A, extra=B

Header: mapped;ogre_md
Row:    A;{metadata}
```

Normalized CSV example:

```text
Mapping: mapped
Plugin record: mapped=A, extra=B

Header: mapped;ogre_id;ogre_md_id
Row:    A;{generated data ID};{metadata ID}
```

An object or array stored under `mapped` remains one CSV cell and is serialized using the existing value serializer. Unmapped keys inside that value remain present because they cannot alter the top-level CSV column count.

## Other Output Formats

The filtering boundary is `CsvFormatter`; `LineBuilder::line_data.data` remains unchanged.

- JSONL and normalized JSONL continue to serialize unmapped top-level fields.
- Non-normalized timeline CSV continues to serialize the complete built record into its single JSON `data` cell.
- Normalized timeline CSV keeps its fixed timeline schema.
- When CSV and JSON outputs are configured together, CSV uses its declared schema while JSON receives the complete shared record.

## Tests

Add Rust tests next to the existing formatter/output tests:

- Regular CSV with a mapped field and an unmapped top-level field produces a row whose width and values match the header; the unmapped value is absent and metadata remains under `ogre_md`.
- Normalized CSV with a mapped field and an unmapped top-level field produces a row whose width and values match the header; generated `ogre_id` and `ogre_md_id` remain aligned.
- A mapped object value retains an unmapped nested key while occupying one CSV cell.
- A combined JSON and CSV output test confirms that CSV filters an unmapped top-level field while JSON retains it.
- Existing tests constructed without a `FieldMapping` continue to verify headerless pass-through serialization.

Tests must parse output using the `csv` crate with normal fixed-width behavior so an unequal header and row width fails the regression test.

## Validation

Required validation:

```bash
cargo test <new-targeted-test-name>
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

Do not add or run Python-related unit tests in this repository.
