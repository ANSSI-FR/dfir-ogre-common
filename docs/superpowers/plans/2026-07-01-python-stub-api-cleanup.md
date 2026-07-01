# Python Stub API Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Synchronize `dfir_ogre_common.pyi` with the conservative public Rust/PyO3 API and remove the single approved internal Python export.

**Architecture:** Keep the runtime API stable except for removing `ParserType` from module-level imports. Fix stubs to describe already-exported or intentionally-exported APIs, and validate with Rust build/test/lint plus source scans because this repository must not add or run Python unit tests.

**Tech Stack:** Rust 2024, PyO3, `.pyi` Python type stubs, ripgrep, Cargo.

---

### Task 1: Update Python Type Stub

**Files:**
- Modify: `dfir_ogre_common.pyi`

- [ ] **Step 1: Add the missing date codec stub**

Insert this method in `class DateInputCodec` after `Timestamp`:

```python
    @classmethod
    def TimestampHex(cls):
        """A codec that parses Unix timestamps in hexadecimal seconds (e.g., '0x5010ad0a')."""
        ...
```

- [ ] **Step 2: Add the missing hive values parser stub**

Insert this function after `parse_hive_keys`:

```python
def parse_hive_values(
    input_file: str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    root_name: Optional[str] = None,
    regexp: Optional[str] = None,
) -> RunReport:
    """Extract values from a Windows Hive file.

    Args:
        input_file: Path to the Hive file to be parsed.
        run_config: RunConfiguration specifying parsing settings.
        plugin_config: Plugin mapping configuration.
        metadata: Metadata associated with the parsing task.
        root_name: Optional string to prepend to paths in the output.
        regexp: Optional regexp to filter path.
    """
    ...
```

- [ ] **Step 3: Fix the sqlite typo**

Change the `parse_sqlite` signature from:

```python
    run_donfig: RunConfiguration,
```

to:

```python
    run_config: RunConfiguration,
```

- [ ] **Step 4: Run stub source checks**

Run:

```bash
rg "TimestampHex|parse_hive_values|run_donfig|run_config" dfir_ogre_common.pyi -n
```

Expected:
- `TimestampHex` appears once.
- `parse_hive_values` appears once.
- `run_donfig` does not appear.
- `parse_sqlite` uses `run_config`.

### Task 2: Align Runtime Module Exports

**Files:**
- Modify: `src/lib_py.rs`

- [ ] **Step 1: Export OutputReport**

Add `OutputReport` to the existing import from `crate::output`:

```rust
output::{COMPRESSION_LEVEL, Output, OutputReport},
```

Add this class registration after `FileReport`:

```rust
    m.add_class::<OutputReport>()?;
```

- [ ] **Step 2: Remove ParserType from module-level exports**

Change the `field_mapping` import from:

```rust
field_mapping::{FieldMapping, FieldParser, FieldParserTree, ParserType},
```

to:

```rust
field_mapping::{FieldMapping, FieldParser, FieldParserTree},
```

Delete this registration:

```rust
    m.add_class::<ParserType>()?;
```

- [ ] **Step 3: Run runtime export source checks**

Run:

```bash
rg "OutputReport|ParserType" src/lib_py.rs -n
```

Expected:
- `OutputReport` appears in the import and one `m.add_class::<OutputReport>()?` registration.
- `ParserType` does not appear in `src/lib_py.rs`.

### Task 3: Verify Sibling Usage and Cargo Checks

**Files:**
- No source edits.

- [ ] **Step 1: Confirm ParserType is unused by sibling source**

Run:

```bash
rg "ParserType" ../dfir-ogre ../dfir-ogre-plugin-windows -n --glob '*.py' --glob '!.venv/**'
```

Expected: no matches.

- [ ] **Step 2: Run formatting check**

Run:

```bash
cargo fmt --check
```

Expected: exit code 0.

- [ ] **Step 3: Run full Rust tests**

Run:

```bash
cargo test
```

Expected: all Rust tests pass.

- [ ] **Step 4: Run strict clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: exit code 0 with no warnings.

- [ ] **Step 5: Commit implementation**

Run:

```bash
git add dfir_ogre_common.pyi src/lib_py.rs
git commit -m "Update Python stubs and API exports"
```

Expected: one implementation commit containing only the stub/runtime export changes.
