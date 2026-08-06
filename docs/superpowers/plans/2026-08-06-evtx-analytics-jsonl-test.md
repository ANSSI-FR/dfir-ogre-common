# EVTX Analytics JSONL Fixture Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an end-to-end Rust regression test that parses a real EVTX fixture into inspectable `analytics_jsonl` data, metadata, and timeline files under `.tmp/analytics`.

**Architecture:** Extend the existing inline EVTX test module and reuse its fixture configuration and temporary-folder helper. Exercise the public `parse_evtx` entry point, then decode all three JSONL files and assert their metadata dimensions and identifier joins. Commit the user-authorized `.gitignore` edit and 13 existing documentation deletions separately from the test.

**Tech Stack:** Rust 2024, built-in test harness, `serde_json`, existing DFIR-OGRE EVTX parser and output types, Cargo, Git.

## Global Constraints

- Work only on the current non-`main` branch; never push this repository.
- Reuse `test_data/evtx/kernel_pnp.evtx` and `test_data/evtx/evtx.xml`; add no fixture.
- Add and run Rust tests only; do not add or run Python tests.
- Remove `.tmp/analytics` before the test, but leave the newly generated files after success.
- Keep `.tmp/analytics` ignored and do not commit generated JSONL files.
- Do not change parser or output behavior unless the new regression test exposes a real defect.
- This is coverage for behavior that already exists. Do not manufacture a failing assertion merely to create a red phase; if the complete test fails, diagnose the real discrepancy before changing production code.
- Commit the existing `.gitignore` edit and exactly the 13 authorized specification deletions in a separate cleanup commit.

---

### Task 1: Add the EVTX-to-analytics JSONL regression test

**Files:**
- Modify: `src/parser/evtx.rs:221-331`
- Test: inline `src/parser/evtx.rs` test `evtx_analytics_jsonl_writes_joinable_rows`
- Runtime output: `.tmp/analytics/` (ignored, intentionally retained)

**Interfaces:**
- Consumes: `parse_evtx(input_file: &str, run_config: RunConfiguration, plugin_config: PluginConfiguration, metadata: Metadata) -> RunReport`
- Consumes: existing `test_folder(name: &str) -> PathBuf`
- Produces: test-only verification of data `ogre_id`, data `ogre_md_id`, metadata `id`, timeline `data_id`, and timeline `metadata_id`

- [ ] **Step 1: Confirm the existing fixture baseline**

Run:

```bash
cargo test parser::evtx::tests::evtx --locked -- --exact
```

Expected: one existing EVTX fixture test passes.

- [ ] **Step 2: Add the complete regression test**

Insert this test after the existing `evtx` test in `src/parser/evtx.rs`:

```rust
#[test]
fn evtx_analytics_jsonl_writes_joinable_rows() {
    let output_folder = test_folder("analytics");
    let base_file_name = "evtx_kernel_pnp_analytics";
    let data_path = output_folder.join(format!("{base_file_name}.evtx.jsonl"));
    let metadata_path = output_folder.join("ogre_metadata.jsonl");
    let timeline_path = output_folder.join(format!("{base_file_name}.evtx.tl.jsonl"));

    let output_conf = OutputConfiguration::new(
        base_file_name.to_owned(),
        output_folder.display().to_string(),
        "file".to_owned(),
        "analytics_jsonl".to_owned(),
        "iso".to_owned(),
        true,
        false,
        HashMap::new(),
    );
    let configuration = RunConfiguration::new(vec![output_conf], true, None);
    let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
    let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

    let report = parse_evtx(
        "test_data/evtx/kernel_pnp.evtx",
        configuration,
        plugin_config,
        Metadata::new("analytics-test-host".into()),
    );

    assert_eq!(report.last_error, None);
    assert_eq!(report.output_reports.len(), 1);
    let file_reports = &report.output_reports[0].file_reports;
    assert_eq!(file_reports.len(), 3);
    assert!(
        file_reports
            .iter()
            .all(|file_report| file_report.format == "analytics_jsonl")
    );

    let read_jsonl = |path: &Path| -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    };
    let data_rows = read_jsonl(&data_path);
    let metadata_rows = read_jsonl(&metadata_path);
    let timeline_rows = read_jsonl(&timeline_path);

    assert_eq!(data_rows.len(), 123);
    assert_eq!(metadata_rows.len(), 1);
    assert_eq!(timeline_rows.len(), 123);

    let data_row = &data_rows[3];
    let data_id = data_row["ogre_id"].as_str().expect("analytics data id");
    let metadata_id = data_row["ogre_md_id"]
        .as_str()
        .expect("analytics metadata id");
    assert!(!data_id.is_empty());
    assert!(!metadata_id.is_empty());
    assert_eq!(data_row["computer"].as_str(), Some("analytics-test-host"));
    assert_eq!(data_row["data_type"].as_str(), Some("evtx"));
    assert_eq!(data_row["system"]["computer"].as_str(), Some("MINWINPC"));

    let metadata_row = &metadata_rows[0];
    assert_eq!(metadata_row["id"].as_str(), Some(metadata_id));
    assert_eq!(
        metadata_row["computer"].as_str(),
        Some("analytics-test-host")
    );
    assert_eq!(metadata_row["data_type"].as_str(), Some("evtx"));

    let timeline_row = timeline_rows
        .iter()
        .find(|row| row["data_id"].as_str() == Some(data_id))
        .expect("timeline row linked to representative data row");
    assert_eq!(timeline_row["data_id"].as_str(), Some(data_id));
    assert_eq!(timeline_row["metadata_id"].as_str(), Some(metadata_id));
    assert_eq!(
        timeline_row["computer"].as_str(),
        Some("analytics-test-host")
    );
    assert_eq!(timeline_row["data_type"].as_str(), Some("evtx"));
}
```

Do not add `remove_dir_if_exists(&output_folder)` at the end; retained output is a requirement.

- [ ] **Step 3: Run the new test**

Run:

```bash
cargo test parser::evtx::tests::evtx_analytics_jsonl_writes_joinable_rows --locked -- --exact --nocapture
```

Expected: one test passes. If it fails, inspect the actual report and generated JSONL before changing an assertion or production code.

- [ ] **Step 4: Confirm the retained artifacts**

Run:

```bash
find .tmp/analytics -maxdepth 1 -type f -print -exec wc -l {} \;
```

Expected: exactly these three files remain, with line counts 123, 1, and 123 respectively:

```text
.tmp/analytics/evtx_kernel_pnp_analytics.evtx.jsonl
.tmp/analytics/ogre_metadata.jsonl
.tmp/analytics/evtx_kernel_pnp_analytics.evtx.tl.jsonl
```

- [ ] **Step 5: Run all EVTX parser tests**

Run:

```bash
cargo test parser::evtx::tests --locked
```

Expected: all EVTX parser tests pass and `.tmp/analytics` still exists.

- [ ] **Step 6: Commit only the regression test**

```bash
git add src/parser/evtx.rs
git diff --cached --check
git commit -m "Test EVTX analytics JSONL output"
```

Expected: the commit contains only `src/parser/evtx.rs`.

### Task 2: Commit the authorized repository cleanup

**Files:**
- Modify: `.gitignore`
- Delete: `docs/superpowers/specs/2026-06-15-output-field-traversal-design.md`
- Delete: `docs/superpowers/specs/2026-07-01-json-string-parser-application-design.md`
- Delete: `docs/superpowers/specs/2026-07-01-python-stub-api-cleanup-design.md`
- Delete: `docs/superpowers/specs/2026-07-01-qualifier-hard-removal-design.md`
- Delete: `docs/superpowers/specs/2026-07-01-reduce-crate-clippy-allowances-design.md`
- Delete: `docs/superpowers/specs/2026-07-03-output-serialization-writers-design.md`
- Delete: `docs/superpowers/specs/2026-07-03-security-descriptor-robustness-design.md`
- Delete: `docs/superpowers/specs/2026-07-09-output-serialization-writer-cleanup-design.md`
- Delete: `docs/superpowers/specs/2026-07-09-timeline-borrowed-serialization-design.md`
- Delete: `docs/superpowers/specs/2026-07-20-csv-schema-driven-output-design.md`
- Delete: `docs/superpowers/specs/2026-07-20-hive-values-key-metadata-design.md`
- Delete: `docs/superpowers/specs/2026-07-22-dfir-nt-hive-dependency-update-design.md`
- Delete: `docs/superpowers/specs/2026-07-22-oversized-dword-raw-fallback-design.md`

**Interfaces:**
- Consumes: the exact user-authorized working-tree cleanup already present before implementation
- Produces: one isolated Git commit containing one `.gitignore` modification and 13 deletions

- [ ] **Step 1: Reconfirm the exact cleanup diff**

Run:

```bash
git diff -- .gitignore
git diff --name-status --diff-filter=D
```

Expected: `.gitignore` changes `/docs` to `docs/`, and the deletion list matches the 13 files above with no additional path.

- [ ] **Step 2: Stage only the authorized cleanup paths**

```bash
git add .gitignore \
  docs/superpowers/specs/2026-06-15-output-field-traversal-design.md \
  docs/superpowers/specs/2026-07-01-json-string-parser-application-design.md \
  docs/superpowers/specs/2026-07-01-python-stub-api-cleanup-design.md \
  docs/superpowers/specs/2026-07-01-qualifier-hard-removal-design.md \
  docs/superpowers/specs/2026-07-01-reduce-crate-clippy-allowances-design.md \
  docs/superpowers/specs/2026-07-03-output-serialization-writers-design.md \
  docs/superpowers/specs/2026-07-03-security-descriptor-robustness-design.md \
  docs/superpowers/specs/2026-07-09-output-serialization-writer-cleanup-design.md \
  docs/superpowers/specs/2026-07-09-timeline-borrowed-serialization-design.md \
  docs/superpowers/specs/2026-07-20-csv-schema-driven-output-design.md \
  docs/superpowers/specs/2026-07-20-hive-values-key-metadata-design.md \
  docs/superpowers/specs/2026-07-22-dfir-nt-hive-dependency-update-design.md \
  docs/superpowers/specs/2026-07-22-oversized-dword-raw-fallback-design.md
```

- [ ] **Step 3: Review and commit the cleanup**

Run:

```bash
git diff --cached --name-status
git diff --cached --check
git commit -m "Clean obsolete design documentation"
```

Expected: one modified `.gitignore`, exactly 13 deleted specification files, and no source or generated files.

### Task 3: Run final verification

**Files:**
- Verify: `src/parser/evtx.rs`
- Verify: `.gitignore`
- Verify: `.tmp/analytics/` runtime output
- Verify: full branch relative to `main`

**Interfaces:**
- Consumes: the committed regression test and cleanup commits from Tasks 1 and 2
- Produces: evidence that formatting, tests, linting, retained artifacts, and Git scope are correct

- [ ] **Step 1: Check Rust formatting**

Run:

```bash
cargo fmt --check
```

Expected: exit code 0.

- [ ] **Step 2: Run the full Rust test suite**

Run:

```bash
cargo test --locked
```

Expected: all tests pass, including `evtx_analytics_jsonl_writes_joinable_rows`.

- [ ] **Step 3: Run strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Expected: exit code 0 with no warnings.

- [ ] **Step 4: Recheck retained output and branch scope**

Run:

```bash
find .tmp/analytics -maxdepth 1 -type f -print -exec wc -l {} \;
git diff --check main...HEAD
git status --short
git log --oneline main..HEAD
```

Expected:

- the three analytics files remain with line counts 123, 1, and 123;
- the committed diff has no whitespace errors;
- the working tree is clean because `.tmp/` is ignored;
- branch history contains the existing data-model/spec/plan commits, the EVTX test commit, and the isolated cleanup commit;
- no push is attempted.
