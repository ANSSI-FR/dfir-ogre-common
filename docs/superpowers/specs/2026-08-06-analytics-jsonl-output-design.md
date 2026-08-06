# Analytics JSONL Output Design

## Summary

Add `analytics_jsonl` as an accepted output-format string. It behaves like
`normalized_jsonl`: it computes stable identifiers, writes normalized data and metadata
files, and writes a separate normalized timeline file when `with_timeline` is enabled.
The analytics format additionally places authoritative metadata dimensions directly on
records that analytics systems commonly query:

- Every main data row contains top-level `computer` and `data_type` fields sourced from
  `Metadata`.
- Every timeline row contains a top-level `computer` field sourced from `Metadata`.

These additions apply to main data rows whether `with_timeline` is `true` or `false`.

## Goals

- Accept `"analytics_jsonl"` in `OutputConfiguration.format`.
- Preserve all normalized JSONL identifiers, sidecar files, file naming, compression,
  reporting, and timeline behavior.
- Add metadata-derived `computer` and `data_type` to every analytics data row.
- Guarantee that the analytics timeline `computer` comes from the same `Metadata`.
- Make metadata authoritative when an input record contains top-level `computer` or
  `data_type` fields.
- Preserve existing `jsonl`, `normalized_jsonl`, `csv`, and `normalized_csv` behavior.
- Keep the existing public Rust and Python API signatures unchanged.

## Non-Goals

- Do not add `analytics_csv`.
- Do not flatten any metadata fields other than `computer` and `data_type`.
- Do not change the shape or contents of `ogre_metadata.jsonl`.
- Do not replace the timeline-specific `data_type` with `Metadata.data_type`.
- Do not change how IDs are computed.
- Do not change existing output paths, append behavior, or compression settings.
- Do not change nested fields named `computer` or `data_type`.
- Do not add Python tests or change the Python type stub; `format` remains a string.

## External API Compatibility

The public configuration API remains unchanged. `OutputConfiguration.format` is still a
`String` in Rust and `str` in Python, and all constructor signatures remain the same. The
only externally observable API addition is that the existing format argument accepts the
new value `"analytics_jsonl"`.

`OutputFormat`, `OutputWriter`, and `JsonFormatter` are contained in private crate modules
and are not re-exported or registered with Python. Their internal representations and
method signatures can therefore change without altering the public Rust or Python API.

## Output Contract

`analytics_jsonl` uses the same paths as `normalized_jsonl` for a given base name and data
type:

| Configuration | File | Behavior |
| --- | --- | --- |
| Any | `<base>.<data_type>.jsonl` | Normalized data rows plus metadata-derived `computer` and `data_type` |
| Any | `ogre_metadata.jsonl` | Unchanged normalized metadata rows |
| `with_timeline: true` | `<base>.<data_type>.tl.jsonl` | Normalized timeline rows with metadata-derived `computer` |

The corresponding files use the existing `.jsonl.gz` names when `output_type` is `gzip`.
Main data rows retain `ogre_id` and `ogre_md_id`. Timeline rows retain their timeline ID,
`metadata_id`, `data_id`, and timeline-specific `data_type`.

The authoritative fields are always emitted, even when `include_empty` is false. Both
metadata values are strings; `Output::new` sets `Metadata.data_type` from the selected
`DataTypeMapping` before constructing the line builder and output writers.

## Architecture

### Explicit JSON mode

Replace the crate-private `normalized: bool` passed to `JsonFormatter::new` with a
crate-private mode enum:

- `Standard`
- `Normalized`
- `Analytics`

The mode owns format-specific behavior. Both `Normalized` and `Analytics` create the
metadata writer, optionally create the separate timeline writer, and use the normalized
write path. Only `Analytics` enables the extra top-level dimensions.

Add `AnalyticsJsonl` to the crate-private `OutputFormat` parser and `OutputWriter`
dispatcher. Treat it as a normalized-family format when deciding whether `LineBuilder`
must compute hashes. Dispatch its metadata writes, data writes, reports, and close calls
through the same lifecycle as `NormalizedJsonl`.

### Analytics data serialization

Keep the existing standard and normalized record serialization semantics unchanged. Add
an analytics-specific serialization projection in `format_json.rs` that:

1. Iterates through the built data record in its existing order.
2. Omits only top-level source fields named `computer` or `data_type`.
3. Writes metadata-derived `computer` and `data_type` exactly once using
   `JsonObjectWriter` string-field escaping.
4. Writes `ogre_md_id` with the same value and escaping used by normalized JSONL.

The generated analytics fields are appended in the deterministic order `computer`,
`data_type`, then `ogre_md_id`. The generated `ogre_id` remains in the built record and is
serialized in its existing position.

Filtering during serialization avoids mutating `LineBuilder.line_data`. This matters when
one `Output` instance has several output configurations: analytics-only fields must not
leak into standard or normalized writers that consume the same line builder.

### Timeline computer override

Extend the crate-private JSON timeline serialization context with an optional borrowed
computer override. The timeline JSON serializer prefers that override over
`TimeLine.computer`. Analytics passes `line_builder.metadata.computer`; existing standard
and normalized JSON paths pass no override and therefore retain their current behavior.

Only timeline JSON serialization uses this analytics override. Timeline `data_type`
continues to come from `TimeLineBuilder`, as it does for normalized output. The override
does not mutate stored timeline entries. Classifying analytics as a normalized-family
format also preserves the current invariant that `LineBuilder` assigns this same metadata
computer before computing each timeline ID.

## Data Flow

For every analytics output configuration:

1. `Output::new` selects the data-type mapping and assigns its data type to `Metadata`.
2. Format parsing selects `AnalyticsJsonl`.
3. Analytics is classified as a normalized-family output, enabling metadata, data, and
   timeline IDs.
4. `JsonFormatter` opens the main data file, `ogre_metadata.jsonl`, and, when requested,
   the timeline file.
5. The existing metadata write serializes one complete metadata row to the metadata
   sidecar.
6. Each built record is serialized to the main file with normalized IDs and the two
   authoritative analytics dimensions.
7. When timeline output is enabled, each eligible timeline is serialized to the timeline
   file with the metadata computer override.

Undated timeline filtering, `timeline_include_undated`, line counts, reports, and writer
closing retain normalized JSONL semantics.

## Collision Semantics

`computer` and `data_type` are reserved top-level keys in analytics data rows. If parsed
input supplies either key, its value is excluded from that output row and the metadata
value is emitted once. This prevents duplicate JSON object keys and guarantees that
metadata wins.

The replacement is shallow. Values inside nested objects or arrays are ordinary artifact
data and remain untouched. Collision behavior for every other key, including existing
normalized identifier keys, remains outside this change. Filtering happens after line
building and ID computation, so it does not alter the normalized hashing algorithm.

## Error Handling

No new error variant is required. Unknown format strings continue to return
`Error::InvalidOutputFormat`, while `"analytics_jsonl"` follows the existing configuration,
date-codec, compression-level, file-opening, write, and close error paths.

Analytics record serialization uses the existing rollback discipline: if serializing a
record value fails, truncate the output buffer to its pre-call length and return the
original error. Metadata strings use `JsonObjectWriter` escaping so quotes, backslashes,
control characters, and newlines produce valid JSON.

## Testing

Add inline Rust tests covering:

- `OutputFormat::from_str("analytics_jsonl")` selects `AnalyticsJsonl`.
- A non-timeline analytics output writes normalized IDs, the unchanged metadata sidecar,
  and metadata-derived `computer` and `data_type` in its main row.
- A timeline-enabled analytics output enriches its main row and writes the metadata
  computer in each timeline row while preserving the timeline-specific `data_type`.
- Source top-level `computer` and `data_type` values are replaced and each serialized
  top-level key occurs exactly once. Assert this with an exact-output unit test or a
  duplicate-aware top-level check rather than a parser that silently accepts duplicates.
- Same-named fields inside nested objects or arrays remain unchanged.
- Metadata values containing JSON-special characters round-trip correctly.
- `FileReport.format` remains `"analytics_jsonl"` and reports all normalized-family files.
- A gzip smoke test confirms analytics data and sidecar streams finish successfully.

Existing tests for all output formats must continue to pass unchanged. Do not add or run
Python unit tests in this repository.

Implementation verification commands:

- `cargo fmt --check`
- `cargo test --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`

## Risks and Mitigations

The main compatibility risk is allowing the internal mode refactor to change standard or
normalized output. Keep enrichment behind `JsonMode::Analytics`, preserve existing paths
for the other modes, and run the full Rust test suite.

The main correctness risk is producing duplicate reserved keys. Filter the two keys before
writing generated fields and verify top-level uniqueness with exact serialized output or
a duplicate-aware check rather than relying only on a JSON parser that accepts duplicates.

The main cross-output risk is mutating the shared built record. Perform enrichment only in
the analytics serializer and include a mixed-output regression test proving analytics
fields do not leak into a standard or normalized writer using the same `Output`.
