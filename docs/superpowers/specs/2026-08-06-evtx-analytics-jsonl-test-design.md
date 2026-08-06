# EVTX Analytics JSONL Fixture Test Design

## Summary

Add one inline Rust test that parses the existing sanitized `kernel_pnp.evtx`
fixture through the public EVTX parser and writes `analytics_jsonl` data,
metadata, and timeline files under `.tmp/analytics`. The test cleans that folder
before it runs and deliberately leaves the fresh output afterward for inspection.

The same branch will also include the user-authorized `.gitignore` edit and the
13 specification-file deletions already present in the working tree. Those
repository-cleanup changes will be committed separately from the parser test.

## Goals

- Exercise a real EVTX fixture through `parse_evtx` with `analytics_jsonl`.
- Cover both main data rows and separate timeline rows.
- Prove that the generated data, metadata, and timeline rows are joinable by
  their normalized identifiers.
- Prove that metadata owns top-level `computer` and `data_type` while nested EVTX
  computer data remains unchanged.
- Put inspectable output in exactly `.tmp/analytics`.
- Reuse the existing EVTX fixture and XML mapping without adding test data.

## Non-Goals

- Do not change parser or formatter behavior.
- Do not add a new EVTX fixture or expected-output snapshot.
- Do not add or run Python tests.
- Do not commit files generated under `.tmp/analytics`.
- Do not refactor unrelated EVTX tests.

## Test Structure

Add `evtx_analytics_jsonl_writes_joinable_rows` to the existing test module in
`src/parser/evtx.rs`.

The test will:

1. Use the existing `test_folder("analytics")` helper to remove and recreate
   `.tmp/analytics`.
2. Read `test_data/evtx/evtx.xml` as the plugin configuration.
3. Parse `test_data/evtx/kernel_pnp.evtx`.
4. Configure a file output with:
   - base name `evtx_kernel_pnp_analytics`;
   - format `analytics_jsonl`;
   - date format `iso`;
   - `with_timeline: true`;
   - `include_empty: false`.
5. Use metadata computer `analytics-test-host`; the selected mapping supplies
   metadata `data_type` value `evtx`.
6. Let `parse_evtx` finalize all writers before reading the output files.
7. Leave the completed `.tmp/analytics` directory in place.

Expected files:

- `.tmp/analytics/evtx_kernel_pnp_analytics.evtx.jsonl`
- `.tmp/analytics/ogre_metadata.jsonl`
- `.tmp/analytics/evtx_kernel_pnp_analytics.evtx.tl.jsonl`

## Assertions

The test will fail directly if parsing reports an error, a file is absent, a JSON
line is invalid, or an expected relationship does not hold.

It will assert:

- the main data file contains 123 rows;
- the metadata file contains exactly one row;
- the timeline file contains 123 rows;
- all three file reports retain format `analytics_jsonl`;
- a representative data row has non-empty string `ogre_id` and `ogre_md_id`;
- its top-level `computer` is `analytics-test-host`;
- its top-level `data_type` is `evtx`;
- its nested `system.computer` remains the computer value parsed from the EVTX;
- the metadata row has matching `id`, `computer`, and `data_type`;
- a timeline row selected by matching `data_id` has:
  - `data_id` equal to the data row's `ogre_id`;
  - `metadata_id` equal to both the data row's `ogre_md_id` and metadata `id`;
  - `computer` equal to `analytics-test-host`;
  - its timeline-specific `data_type` unchanged.

Selecting the timeline row by `data_id`, rather than assuming the same array
index, makes the join assertion explicit and resilient to timeline ordering.

## Temporary Output Lifecycle

Cleaning `.tmp/analytics` before the parse prevents append-mode writers from
duplicating rows across repeated test runs. The test intentionally performs no
successful-run cleanup, so developers can inspect the generated files. A failed
test may leave partial output, which the next run removes before parsing.

The directory is already covered by the repository's `.tmp/` ignore rule.

## Authorized Repository Cleanup

Include the existing `.gitignore` change from `/docs` to `docs/` and delete these
13 already-removed files in a separate cleanup commit:

- `docs/superpowers/specs/2026-06-15-output-field-traversal-design.md`
- `docs/superpowers/specs/2026-07-01-json-string-parser-application-design.md`
- `docs/superpowers/specs/2026-07-01-python-stub-api-cleanup-design.md`
- `docs/superpowers/specs/2026-07-01-qualifier-hard-removal-design.md`
- `docs/superpowers/specs/2026-07-01-reduce-crate-clippy-allowances-design.md`
- `docs/superpowers/specs/2026-07-03-output-serialization-writers-design.md`
- `docs/superpowers/specs/2026-07-03-security-descriptor-robustness-design.md`
- `docs/superpowers/specs/2026-07-09-output-serialization-writer-cleanup-design.md`
- `docs/superpowers/specs/2026-07-09-timeline-borrowed-serialization-design.md`
- `docs/superpowers/specs/2026-07-20-csv-schema-driven-output-design.md`
- `docs/superpowers/specs/2026-07-20-hive-values-key-metadata-design.md`
- `docs/superpowers/specs/2026-07-22-dfir-nt-hive-dependency-update-design.md`
- `docs/superpowers/specs/2026-07-22-oversized-dword-raw-fallback-design.md`

No other ignored or deleted files are in scope.

## Verification

Run the new test directly first, then the parser regression tests and full Rust
verification:

- `cargo test evtx_analytics_jsonl_writes_joinable_rows --locked -- --nocapture`
- `cargo test parser::evtx::tests --locked`
- `cargo fmt --check`
- `cargo test --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`

After the direct test, verify that `.tmp/analytics` contains the three expected
files and that the test leaves them in place.
