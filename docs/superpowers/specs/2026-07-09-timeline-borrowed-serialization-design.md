# Timeline Borrowed Serialization Design

## Summary

Optimize timeline serialization for JSON and CSV outputs by removing avoidable clones in formatter hot paths. The current formatters clone the full timeline vector and clone the complete data record into each timeline entry before serializing. On large records, this can multiply memory traffic by the number of timeline entries.

The change will preserve the public `TimeLine` struct and its existing `json_serialise` and `csv_serialise` methods. New crate-private borrowed serialization helpers will let formatters serialize timeline entries with borrowed metadata and data references.

## Goals

- Remove `data.timeline.clone()` from JSON and CSV formatter timeline paths.
- Remove `data.data.clone()` from non-normalized JSON and CSV timeline output.
- Keep the public Rust and Python-facing `TimeLine` API unchanged.
- Preserve the existing JSON and CSV output shape, field order, line counts, and error behavior.
- Keep `LineBuilder` reusable after writing, since formatters currently accept `&LineBuilder`.

## Non-Goals

- Do not change the ownership model of `LineBuilder` or `LineData`.
- Do not drain timeline entries during formatting.
- Do not add public API surface unless the implementation reveals a strong need.
- Do not add benchmark infrastructure in this pass.

## Current Problem

The JSON formatter currently uses this pattern when timeline output is enabled:

- Clone the timeline vector.
- Mutate each cloned timeline with serialized metadata.
- Clone the full output record into each cloned timeline.
- Serialize the cloned timeline.

CSV uses the same pattern. Normalized output also clones the timeline vector before adding or confirming metadata IDs. These clones are unnecessary because serialization only needs read access to the timeline entry and borrowed access to the metadata or data record.

## Approach

Add crate-private borrowed serialization helpers near `TimeLine`, keeping `TimeLine` as the owned data model. The helpers will serialize a borrowed `&TimeLine` with optional borrowed overrides:

- `metadata: Option<&MetadataSerialized>` for non-normalized timeline output.
- `data: Option<&Record>` for non-normalized timeline output.
- `metadata_id: Option<&str>` for normalized output fallback.

Existing public `TimeLine::json_serialise` and `TimeLine::csv_serialise` will delegate to the borrowed helpers by passing `self.metadata.as_ref()`, `self.data.as_ref()`, and `self.metadata_id.as_deref()`.

Formatters will iterate by reference:

- `for timeline in &data.timeline` in JSON timeline output.
- `for timeline in &data.timeline` in CSV timeline output.

They will pass borrowed metadata and data to the helper instead of mutating cloned timeline values.

## Data Flow

For non-normalized JSON and CSV timeline output:

1. The formatter computes or reuses serialized metadata once, as it does today.
2. The formatter iterates over `&data.timeline`.
3. The borrowed serializer writes scalar timeline fields from the timeline entry.
4. The serializer injects borrowed metadata and borrowed `data.data` while writing.
5. The original timeline entry remains unchanged.

For normalized JSON and CSV timeline output:

1. The formatter iterates over `&data.timeline`.
2. The serializer writes scalar timeline fields from the timeline entry.
3. The serializer uses the timeline's existing `metadata_id`, falling back to `line_builder.metadata_id` if needed.
4. Data records continue to be written once to the data writer with `ogre_md_id`.

## Error Handling

The borrowed helpers must preserve current rollback behavior. If nested record serialization fails, the destination buffer must be truncated back to the pre-write length and the original error returned.

JSON helpers will use `JsonObjectWriter::try_field_raw` for fallible nested values. CSV helpers will use `CsvRowWriter::try_raw_field` when writing fallible serialized data fields.

## Testing

Existing JSON and CSV timeline tests should pass unchanged.

Add focused regression tests that write timeline output through the formatter and assert that source timeline entries are not mutated. After writing:

- `line_builder.line_data.timeline[0].metadata` remains `None`.
- `line_builder.line_data.timeline[0].data` remains `None`.

Keep existing `TimeLine::json_serialise` and `TimeLine::csv_serialise` tests to confirm owned optional metadata and data still serialize correctly through the public methods.

Verification commands:

- `cargo fmt --check`
- `cargo test --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`

## Risks

The main risk is accidentally changing field omission or field order when duplicating serialization logic. The implementation will reduce that risk by extracting shared private helpers and having the public owned methods delegate to them.

Another risk is changing normalized output behavior when a timeline already has `metadata_id`. The helper will prefer the timeline's existing ID and only use the formatter-provided ID as a fallback.
