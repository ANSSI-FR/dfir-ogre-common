# `analytics_jsonl` data model and ClickHouse mapping

This document describes the rows produced by `analytics_jsonl` and a compact
ClickHouse model that retains each complete logical row. The examples target
version 25.3 or newer, where the native `JSON` type is production-ready.

## Output files

For a base name `<base>` and selected artifact type `<data_type>`, the formatter
appends one JSON object per line to:

| File | Contents | Present when |
| --- | --- | --- |
| `<base>.<data_type>.jsonl` | Artifact data rows | Always |
| `ogre_metadata.jsonl` | Artifact-source metadata | Always |
| `<base>.<data_type>.tl.jsonl` | Timeline rows | `with_timeline: true` |

Gzip output uses the same names with a `.gz` suffix. Main data rows are enriched
whether or not timeline output is enabled.

## Relationships

The three row types form these logical relationships:

| From | To | Meaning |
| --- | --- | --- |
| `data.ogre_md_id` | `metadata.id` | Metadata for the artifact row |
| `timeline.data_id` | `data.ogre_id` | Artifact row that produced the timeline row |
| `timeline.metadata_id` | `metadata.id` | Metadata for the timeline row |

One data row may produce zero, one, or several timeline rows. IDs should be
stored as opaque `String` values. ClickHouse primary and sorting keys do not
enforce uniqueness, so ingestion must choose its own replay/deduplication policy.

## Main data rows

Main rows combine a fixed envelope with fields defined by the selected artifact
mapping:

| Field | JSON type | Contract |
| --- | --- | --- |
| `ogre_id` | string | Stable data-row identifier used by `timeline.data_id` |
| `ogre_md_id` | string | Metadata identifier used by `metadata.id` |
| `computer` | string | Always emitted from `Metadata.computer` |
| `data_type` | string | Always emitted from `Metadata.data_type` |
| all other fields | any JSON value | Artifact-specific payload |

`computer` and `data_type` are reserved at the top level. If the parsed artifact
contains either field, the metadata value replaces it. Fields with those names
inside nested objects or arrays are preserved.

Example:

```json
{"event":"login","when":"2026-08-06T12:00:00.000000+00:00","ogre_id":"<data-id>","computer":"host-01","data_type":"events","ogre_md_id":"<metadata-id>"}
```

## Metadata rows

| Field | JSON type | Presence |
| --- | --- | --- |
| `id` | string | Always for `analytics_jsonl` |
| `computer` | string | Always; may be empty |
| `data_type` | string | Always; may be empty |
| `orc_id` | string | Optional |
| `folder` | string | Optional |
| `archive` | string | Optional |
| `subarchive` | string | Optional |
| `archive_filename` | string | Optional |
| `original_filename` | string | Optional |
| `vss` | string | Optional |
| `orc_start_date` | formatted string | Optional |
| `creation_date` | formatted string | Optional |
| `modif_date` | formatted string | Optional |

Optional metadata fields are omitted rather than emitted as `null`.

## Timeline rows

| Field | JSON type | Contract |
| --- | --- | --- |
| `id` | string | Timeline-row identifier |
| `computer` | string | Always sourced from the same metadata as the data row |
| `timestamp` | formatted string or `null` | Event time; `null` only for included undated rows |
| `timestamp_meaning` | string | Meaning or source of the timestamp |
| `data_type` | string | Timeline classification from the timeline mapping |
| `related_user` | string | Related user, possibly empty |
| `description` | string | Human-readable description, possibly empty |
| `additional_description` | string | Additional description, possibly empty |
| `metadata_id` | string | Joins to `metadata.id` |
| `data_id` | string | Joins to `data.ogre_id` |

Timeline `data_type` is deliberately independent of the main row's `data_type`.
For example, a main `events` row may produce a `timeline-events` row. Do not use
the two columns as a join key.

## JSON value conventions

Artifact payload fields can be strings, signed 64-bit integers, finite floats,
booleans, arrays, nested objects, dates serialized as strings, or nulls. With
`include_empty: false`, null object fields and null array elements are omitted.
Non-finite floats serialize as `null`.

All date fields use the configured `date_format`. The default `iso_utc` format is
an ISO-8601 string with six fractional digits and a UTC offset. A typed ClickHouse
timestamp schema should therefore keep `date_format` fixed; otherwise ingest dates
as `String` and parse them in a view or staging transform.

## Recommended ClickHouse model

Keep each complete data row in one native `JSON` column and materialize its four
stable envelope fields. This preserves the full object, enables efficient joins,
and supports per-`data_type` views without defining every artifact field up front.

```sql
CREATE TABLE analytics_data
(
    payload JSON(
        ogre_id String,
        ogre_md_id String,
        computer String,
        data_type String
    ),
    ogre_id String MATERIALIZED getSubcolumn(payload, 'ogre_id'),
    ogre_md_id String MATERIALIZED getSubcolumn(payload, 'ogre_md_id'),
    computer String MATERIALIZED getSubcolumn(payload, 'computer'),
    data_type String MATERIALIZED getSubcolumn(payload, 'data_type')
)
ENGINE = MergeTree
ORDER BY (data_type, computer, ogre_id);
```

Load each source line as one JSON object into `payload` with ClickHouse's
`JSONAsObject` input format or an equivalent client-side mapping. The explicit
JSON path hints keep the four envelope paths typed as strings.

Assuming the producer uses `date_format: "iso_utc"`, the sidecar tables can be:

```sql
CREATE TABLE analytics_metadata
(
    id String,
    computer String,
    data_type String,
    orc_id Nullable(String),
    folder Nullable(String),
    archive Nullable(String),
    subarchive Nullable(String),
    archive_filename Nullable(String),
    original_filename Nullable(String),
    vss Nullable(String),
    orc_start_date Nullable(DateTime64(6, 'UTC')),
    creation_date Nullable(DateTime64(6, 'UTC')),
    modif_date Nullable(DateTime64(6, 'UTC'))
)
ENGINE = ReplacingMergeTree()
ORDER BY id;

CREATE TABLE analytics_timeline
(
    id String,
    computer String,
    timestamp Nullable(DateTime64(6, 'UTC')),
    timestamp_meaning String,
    data_type String,
    related_user String,
    description String,
    additional_description String,
    metadata_id String,
    data_id String
)
ENGINE = MergeTree
ORDER BY (data_id, id);
```

Use ClickHouse's best-effort datetime input mode for the ISO-8601 strings when
loading these typed sidecar tables. Another safe option is to stage the dates as
`String` and convert them with `parseDateTime64BestEffortOrNull`.

`ReplacingMergeTree` is useful for metadata because append-mode output can repeat
the same metadata row across runs; replacement happens during background merges,
not as a uniqueness constraint at insert time. Use plain `MergeTree` instead if
every ingestion occurrence must remain visible. The example ordering keys favor
the documented joins; production partitioning, ordering, codecs, and retention
should follow the actual query volume and access patterns.

## Per-`data_type` views

A basic view filters the generic table while retaining its dynamic payload:

```sql
CREATE VIEW analytics_data_events AS
SELECT ogre_id, ogre_md_id, computer, data_type, payload
FROM analytics_data
WHERE data_type = 'events';
```

A more specific view can project known JSON paths with stable types while leaving
the canonical table unchanged:

```sql
CREATE VIEW analytics_data_events_typed AS
SELECT
    ogre_id,
    ogre_md_id,
    computer,
    payload.event.:String AS event,
    payload.`when`.:String AS event_time
FROM analytics_data
WHERE data_type = 'events';
```

Timeline-to-data lookup then uses the materialized identifiers:

```sql
SELECT t.*, d.payload, m.original_filename
FROM analytics_timeline AS t
LEFT JOIN analytics_data AS d ON d.ogre_id = t.data_id
LEFT JOIN analytics_metadata AS m ON m.id = t.metadata_id;
```

## ClickHouse notes

- Native `JSON` is production-ready in ClickHouse 25.3 and is intended for dynamic
  data that is queried by individual paths.
- `JSONAsObject` treats each input object as one `JSON` value, which matches the
  flat `analytics_jsonl` data rows.
- ClickHouse flattens JSON paths internally. Literal dots in artifact field names
  can therefore be interpreted as nested paths; normalize or explicitly test such
  keys before relying on them in views.
- Monitor the number of dynamic paths. Frequently queried paths can be given type
  hints or promoted into materialized columns later.

References:

- [ClickHouse JSON data type](https://clickhouse.com/docs/reference/data-types/newjson)
- [ClickHouse JSONAsObject format](https://clickhouse.com/docs/reference/formats/JSON/JSONAsObject)
- [ClickHouse JSONEachRow format](https://clickhouse.com/docs/reference/formats/JSON/JSONEachRow)
- [ClickHouse views](https://clickhouse.com/docs/reference/statements/create/view)
- [ClickHouse date and datetime parsing](https://clickhouse.com/blog/parsing-dates-datetimes)
