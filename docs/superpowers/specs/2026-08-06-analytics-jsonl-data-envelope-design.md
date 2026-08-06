# Analytics JSONL Data Envelope Design

## Summary

Change the main data-row shape produced by `analytics_jsonl` and tighten its timeline
contract. Instead of mixing generated identifiers, metadata dimensions, and parsed
artifact fields in one object, write a fixed technical envelope whose `data` field
contains the complete parsed and mapped record:

```json
{
  "ogre_id": "<generated-data-id>",
  "computer": "<metadata-computer>",
  "data_type": "<metadata-data-type>",
  "ogre_md_id": "<generated-metadata-id>",
  "data": {
    "timestamp": "2016-04-26T21:30:13.520562+00:00",
    "system": {},
    "event_data": {}
  }
}
```

Field order is not part of the contract. The separation prevents artifact fields from
colliding with the four technical envelope names and gives ClickHouse a stable generic
table shape while retaining arbitrary artifact data in a native `JSON` column.

Analytics timeline rows remain separate. Their `computer` and `data_type` dimensions are
both taken from the same metadata as the main data-row envelope so that timeline-to-data
joins can use `(data_type, computer, data_id)` without `metadata_id`.

## Goals

- Put the entire parsed and mapped artifact record under a top-level `data` object.
- Keep `ogre_id`, `computer`, `data_type`, and `ogre_md_id` as the fixed top-level
  analytics envelope.
- Keep the parsed record unchanged: do not remove, rename, or overwrite fields merely
  because they have the same names as envelope fields.
- Keep the generated data identifier separate from `LineData.data` and use
  `LineData.data_id` as its authoritative in-memory representation.
- Guarantee that every analytics timeline `data_id` equals the generated `ogre_id` of
  its source analytics data row.
- Source analytics timeline `computer` and `data_type` from metadata.
- Reject `timeline_include_undated: true` for `analytics_jsonl`, guaranteeing that
  analytics timeline rows always have a timestamp.
- Preserve the existing public Rust and Python configuration APIs and the serialized
  contracts of existing output formats.
- Update `docs/analytics-jsonl-clickhouse-data-model.md` for the new envelope and the
  agreed ClickHouse tables, keys, joins, and views.

## Non-Goals

- Do not change how data IDs, metadata IDs, or timeline IDs are calculated.
- Do not change parser or field-mapping behavior.
- Do not change the metadata sidecar shape or filenames.
- Do not add an analytics CSV format.
- Do not change append, gzip, date-codec, reporting, or `include_empty` behavior except
  where this design explicitly forbids undated analytics timeline rows.
- Do not make timeline `metadata_id` part of the data join or either table's sorting key.
- Do not add or run Python tests in this repository.

## External API and Validation

No public type or function signature changes. `OutputConfiguration.format` remains a
Rust `String` and Python `str`, and `timeline_include_undated` remains available for all
existing non-analytics formats.

An output configuration is invalid when both of these conditions hold:

- `format == "analytics_jsonl"`
- `timeline_include_undated == true`

This is a strict option-level restriction, so it also applies when `with_timeline` is
false. Validation returns `Error::ConfigurationError` before any output directory or
writer is created. Other formats retain their current undated-timeline behavior.

If another output in the same run requests undated timeline construction, the analytics
writer still excludes those rows. Consequently every serialized analytics timeline row
has a non-null timestamp.

## Main Data-Row Contract

Every main `analytics_jsonl` row has exactly these envelope fields:

| Field | JSON type | Source and meaning |
| --- | --- | --- |
| `ogre_id` | string | Generated `LineData.data_id`; identifies the artifact row |
| `computer` | string | `Metadata.computer` |
| `data_type` | string | `Metadata.data_type` |
| `ogre_md_id` | string | Generated `LineBuilder.metadata_id` |
| `data` | object | Complete parsed and mapped `LineData.data` record |

All four technical string fields are emitted even when `include_empty` is false. The
existing date codec and `include_empty` rules apply recursively while serializing the
record inside `data`.

### Name collisions

The serializer does not filter the parsed record. A parsed field named `ogre_id`,
`computer`, `data_type`, `ogre_md_id`, or `data` remains inside the nested `data` object
with its parsed value. It is independent of the same-named envelope field. For example:

```json
{
  "ogre_id": "<generated>",
  "computer": "metadata-host",
  "data_type": "evtx",
  "ogre_md_id": "<metadata-id>",
  "data": {
    "ogre_id": "artifact-supplied-id",
    "computer": "event-host",
    "data_type": "artifact-classification"
  }
}
```

No source value wins or loses: envelope values are technical metadata, while values
under `data` are artifact data.

## Identifier Ownership and Data Flow

`LineBuilder` continues to hash the mapped artifact fields and selected metadata exactly
as it does today. After finalizing the hash it:

1. Stores the generated value only in `LineData.data_id`.
2. Copies that value to each generated `TimeLine.data_id`.
3. Keeps assigning the generated metadata ID and metadata computer needed by timeline
   identity construction.
4. Does not insert or overwrite `ogre_id` in `LineData.data`.

The analytics data serializer reads the envelope `ogre_id` from `LineData.data_id`. The
timeline serializer reads `data_id` from the timeline populated from that same value.
Tests compare the emitted values directly, making the join invariant explicit.

Removing the generated ID from the record must not change existing normalized output
schemas. `normalized_jsonl` and `normalized_csv` synthesize their existing top-level
`ogre_id` field or column from `LineData.data_id` at their serialization boundary. Any
non-normalized output path that currently observes the generated ID in a mixed-format
run must likewise receive it as serialization context rather than relying on mutation of
the shared parsed record. Standalone non-normalized output remains unchanged because it
does not request ID computation.

## Analytics Serialization

Replace the current flat analytics projection with a streaming envelope serializer in
`format_json.rs`. It uses `JsonObjectWriter` to write the four string fields and a raw
`data` object serialized by the existing `Record`/`Value` JSON machinery. This avoids a
record clone and is the shortest path that retains current escaping, date formatting,
null handling, nested-object handling, and non-finite-float behavior.

The serializer follows the existing rollback discipline: record the starting buffer
length, truncate back to it if nested record serialization fails, and propagate the
original error. Field order may follow whichever sequence makes this implementation
simplest.

Metadata serialization, data and timeline filenames, gzip handling, append behavior,
reports, and writer closing remain those of the normalized JSONL family.

## Analytics Timeline Contract

The timeline JSON shape remains:

| Field | JSON type | Analytics source |
| --- | --- | --- |
| `id` | string | Existing generated timeline ID |
| `computer` | string | `Metadata.computer` override |
| `timestamp` | formatted string | Extracted event time; never null for analytics output |
| `timestamp_meaning` | string | Existing timeline mapping |
| `data_type` | string | `Metadata.data_type` override |
| `related_user` | string | Existing timeline mapping |
| `description` | string | Existing timeline mapping |
| `additional_description` | string | Existing timeline mapping |
| `metadata_id` | string | Generated metadata ID, retained for metadata lookup |
| `data_id` | string | Same generated value as the source data row's `ogre_id` |

Extend the crate-private timeline serialization context with an optional borrowed
`data_type` override, parallel to the existing computer override. Analytics JSON passes
both metadata values. Standard JSON, normalized JSON, and CSV pass no data-type override
and retain their current behavior. The override is a serialization projection and does
not mutate shared timeline entries or change the existing timeline-ID hashing algorithm.

## ClickHouse Documentation Model

Revise `docs/analytics-jsonl-clickhouse-data-model.md` so the recommended data table maps
the fixed envelope directly and keeps only the extracted record dynamic:

```sql
CREATE TABLE analytics_data
(
    ogre_id String,
    computer String,
    data_type String,
    ogre_md_id String,
    data JSON
)
ENGINE = ReplacingMergeTree()
ORDER BY (data_type, computer, ogre_id);
```

`ogre_md_id` remains available for metadata lookup but is deliberately absent from the
sorting/replacement key.

Document the timeline table as:

```sql
CREATE TABLE analytics_timeline
(
    id String,
    computer String,
    timestamp DateTime64(6, 'UTC'),
    timestamp_meaning String,
    data_type String,
    related_user String,
    description String,
    additional_description String,
    metadata_id String,
    data_id String
)
ENGINE = ReplacingMergeTree()
ORDER BY (computer, data_type, timestamp, data_id, id);
```

The non-null timestamp is valid because analytics rejects
`timeline_include_undated: true` and filters undated rows produced for any co-configured
format.

The documented timeline-to-data join is exactly:

```sql
LEFT JOIN analytics_data AS d
    ON d.data_type = t.data_type
   AND d.computer = t.computer
   AND d.ogre_id = t.data_id
```

Neither `timeline.metadata_id` nor `data.ogre_md_id` participates in this join.
`metadata_id` and `ogre_md_id` remain available for their respective joins to
`analytics_metadata.id`.

The document also explains:

- loading the new objects with `JSONEachRow` into direct columns;
- `ReplacingMergeTree` replacement being merge-time rather than an immediate uniqueness
  constraint;
- using `FINAL` or another ingestion/query deduplication strategy when immediate logical
  deduplication is required;
- generic per-`data_type` views that retain the `data JSON` column;
- typed views that project known paths such as `data.event` from the generic table.

## Testing

Add or update inline Rust tests for these behaviors:

- The analytics serializer emits exactly the five-field envelope and puts parsed fields
  under `data`.
- Envelope metadata values round-trip when they contain quotes, backslashes, control
  characters, or newlines.
- Parsed top-level fields named `ogre_id`, `ogre_md_id`, `computer`, `data_type`, and
  `data` remain unchanged under the envelope's `data` object.
- `include_empty` and date serialization retain their existing behavior inside `data`.
- `LineBuilder` stores the generated ID in `LineData.data_id`, does not inject it into a
  record that lacks an artifact-supplied `ogre_id`, and assigns the same ID to every
  timeline row.
- `normalized_jsonl` still emits its generated top-level `ogre_id` and `ogre_md_id`.
- `normalized_csv` still emits the generated `ogre_id` and metadata-ID columns.
- Existing standard output behavior is preserved in both standalone and mixed-format
  configurations.
- Analytics timeline JSON uses metadata `computer` and metadata `data_type`, even when
  the timeline mapping specifies a different data type.
- Analytics timeline `data_id` equals the main envelope `ogre_id`; the emitted
  `(data_type, computer)` values also equal those on the data row.
- Analytics rejects `timeline_include_undated: true` before creating output files,
  including when `with_timeline` is false.
- Non-analytics formats continue accepting `timeline_include_undated: true`.
- A mixed-output run that constructs undated entries for another format does not emit an
  undated analytics timeline row.
- The fixture-backed EVTX analytics test continues writing under `.tmp/analytics`, reads
  EVTX fields from `row["data"]`, and verifies the envelope/timeline join invariants.

Run the repository's required Rust checks after implementation:

- `cargo fmt --check`
- `cargo test --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`

Do not add or run Python unit tests.

## Risks and Mitigations

The main compatibility risk is that removing `ogre_id` from the shared record could
silently remove it from normalized or mixed-format output. Keep generated-ID projection
at writer boundaries and cover normalized JSONL, normalized CSV, and mixed output with
regression tests.

The main join risk is allowing timeline dimensions to diverge from the data envelope.
Pass both metadata values through the analytics timeline serialization context and assert
the complete three-column join tuple in tests.

The main collision risk is accidentally filtering artifact fields out of the nested
record. Serialize `LineData.data` wholesale beneath `data`; do not maintain a reserved-key
filter.

The main ClickHouse risk is assuming `ReplacingMergeTree` deduplicates synchronously.
Document its merge-time behavior and the need for explicit query-time or ingestion-time
deduplication where immediate uniqueness matters.
