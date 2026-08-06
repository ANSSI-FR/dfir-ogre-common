# `analytics_jsonl` data model and ClickHouse mapping

This document describes the rows produced by `analytics_jsonl` and a generic
ClickHouse schema for ingesting them. The examples assume a ClickHouse version
with the native `JSON` type enabled and use `date_format: "iso_utc"` so timeline
timestamps load into `DateTime64(6, 'UTC')` consistently.

## Output files

For a base name `<base>` and selected artifact type `<data_type>`, the formatter
appends one JSON object per line to:

| File | Contents | Present when |
| --- | --- | --- |
| `<base>.<data_type>.jsonl` | Enveloped artifact data rows | Always |
| `ogre_metadata.jsonl` | Artifact-source metadata | Always |
| `<base>.<data_type>.tl.jsonl` | Timeline rows | `with_timeline: true` |

Gzip output uses the same names with a `.gz` suffix. Main rows use the same
envelope whether or not timeline output is enabled.

`analytics_jsonl` rejects `timeline_include_undated: true`. Consequently, every
emitted analytics timeline row has a timestamp.

## Main data rows

Each main row has exactly five top-level fields:

```json
{
  "ogre_id": "<generated-data-id>",
  "computer": "host-01",
  "data_type": "evtx",
  "ogre_md_id": "<metadata-id>",
  "data": {
    "timestamp": "2026-08-06T12:00:00.000000+00:00",
    "system": {"computer": "event-host"},
    "event_data": {}
  }
}
```

| Field | JSON type | Contract |
| --- | --- | --- |
| `ogre_id` | string | Generated identifier for this parsed data row |
| `computer` | string | Sourced from `Metadata.computer` |
| `data_type` | string | Sourced from `Metadata.data_type` |
| `ogre_md_id` | string | Metadata identifier |
| `data` | object | Complete parsed and mapped artifact record |

The `data` object is serialized without filtering or overwriting it. If an
artifact itself contains fields named `ogre_id`, `computer`, `data_type`,
`ogre_md_id`, or `data`, those values remain available under `data.*`. They are
independent of the technical fields in the outer envelope.

With `include_empty: false`, null fields and null array elements inside `data`
are omitted according to the normal JSON serializer. The outer five fields are
always emitted.

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
| `id` | string | Generated timeline-row identifier |
| `computer` | string | Sourced from the same metadata as the main row |
| `timestamp` | formatted string | Timeline event time; never null for analytics output |
| `timestamp_meaning` | string | Meaning or source of the timestamp |
| `data_type` | string | Sourced from the same metadata as the main row |
| `related_user` | string | Related user, possibly empty |
| `description` | string | Human-readable description, possibly empty |
| `additional_description` | string | Additional description, possibly empty |
| `metadata_id` | string | Metadata identifier |
| `data_id` | string | Generated identifier of the source main row |

The analytics formatter projects metadata `computer` and `data_type` into each
timeline row. It does not change the stored timeline object or its generated
`id`. The timeline `data_id` is the same generated value exposed as `ogre_id` by
the source main-row envelope.

## Relationships

The exact logical relationships are:

```text
analytics_data.ogre_md_id -> analytics_metadata.id
analytics_timeline.metadata_id -> analytics_metadata.id
(analytics_timeline.data_type, analytics_timeline.computer, analytics_timeline.data_id)
    -> (analytics_data.data_type, analytics_data.computer, analytics_data.ogre_id)
```

One data row may produce zero, one, or several timeline rows. IDs are opaque
strings. The timeline-to-data join deliberately does not include a metadata ID;
it uses `data_type`, `computer`, and the generated data ID for lookup locality
and performance. Metadata remains independently reachable from either row type.

## ClickHouse tables

Each JSONL line maps directly to table columns with the `JSONEachRow` input
format. The outer envelope becomes ordinary columns, and only the nested `data`
object is stored in the native JSON column.

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

The metadata sidecar can be loaded into this table:

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
    orc_start_date Nullable(String),
    creation_date Nullable(String),
    modif_date Nullable(String)
)
ENGINE = ReplacingMergeTree()
ORDER BY id;
```

Analytics timeline timestamps are guaranteed to be present, so the timeline
table uses a non-nullable timestamp:

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

For direct ingestion, for example:

```sql
INSERT INTO analytics_data FORMAT JSONEachRow
-- stream <base>.<data_type>.jsonl here

INSERT INTO analytics_metadata FORMAT JSONEachRow
-- stream ogre_metadata.jsonl here

INSERT INTO analytics_timeline FORMAT JSONEachRow
-- stream <base>.<data_type>.tl.jsonl here
```

ClickHouse can parse the configured ISO-8601 timeline timestamps with an
appropriate date-time input setting. Optional metadata dates remain strings so
their configured representation is preserved; parse them in a view or transform
when a typed metadata date is useful.

## Per-`data_type` views

The generic table retains `data_type`, making it easy to expose one view per
artifact family while preserving the common join columns:

```sql
CREATE VIEW analytics_data_evtx AS
SELECT ogre_id, ogre_md_id, computer, data_type, data
FROM analytics_data
WHERE data_type = 'evtx';
```

A typed view can project frequently queried artifact paths while the generic
table remains schema-flexible:

```sql
CREATE VIEW analytics_data_evtx_typed AS
SELECT
    ogre_id,
    ogre_md_id,
    computer,
    data.system.computer.:String AS event_computer
FROM analytics_data
WHERE data_type = 'evtx';
```

## Timeline, data, and metadata join

Join timeline rows to main rows with `data_type`, `computer`, and the generated
data ID. Do not add a metadata-ID predicate to that join:

```sql
SELECT t.*, d.data, m.original_filename
FROM analytics_timeline AS t
LEFT JOIN analytics_data AS d
    ON d.data_type = t.data_type
   AND d.computer = t.computer
   AND d.ogre_id = t.data_id
LEFT JOIN analytics_metadata AS m
    ON m.id = t.metadata_id;
```

## Replacement and operational notes

`ReplacingMergeTree` replacement happens during background merges; it does not
enforce immediate uniqueness when rows are inserted. Use `FINAL` where its cost
is acceptable, or apply an ingestion/query deduplication strategy when queries
must observe immediate logical deduplication.

The ordering keys are designed around common filtering and join access:

- Main data: `(data_type, computer, ogre_id)`.
- Timeline: `(computer, data_type, timestamp, data_id, id)`.
- Metadata: `id`.

`ogre_md_id` and `metadata_id` remain regular columns for metadata lookup; they
are intentionally absent from the data and timeline ordering keys respectively.
Production partitioning, codecs, retention, and additional projections should
follow observed volume and query patterns.

ClickHouse flattens JSON paths internally. Literal dots in artifact field names
can therefore be interpreted as nested paths; test those keys before depending
on them in typed views. Monitor dynamic-path counts and promote frequently used
paths into typed views or projections as the schema stabilizes.

References:

- [ClickHouse JSON data type](https://clickhouse.com/docs/reference/data-types/newjson)
- [ClickHouse JSONEachRow format](https://clickhouse.com/docs/reference/formats/JSON/JSONEachRow)
- [ClickHouse views](https://clickhouse.com/docs/sql-reference/statements/create/view)
- [ClickHouse ReplacingMergeTree](https://clickhouse.com/docs/reference/engines/table-engines/mergetree-family/replacingmergetree)
