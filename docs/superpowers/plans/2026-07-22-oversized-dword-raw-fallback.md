# Oversized DWORD Raw Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve oversized registry DWORD payloads as raw hexadecimal data without emitting parsing errors.

**Architecture:** Keep `dfir-nt-hive::KeyValue::dword_data` unchanged and branch in both `dfir-ogre-common` value adapters before scalar conversion. Payloads larger than eight bytes take each adapter's existing raw-data path, while payloads of eight bytes or fewer retain current scalar behavior. Unit tests derive temporary hives from the existing sanitized SAM fixture, avoiding SAM-specific production logic and new binary fixtures.

**Tech Stack:** Rust 2024, `dfir-nt-hive`, existing `Record`/`Value` and `RegValue` abstractions, Cargo tests, PyO3/maturin, Python 3.13, JSON Lines.

## Global Constraints

- Work on branch `fix-oversized-dword-fallback` in the existing checkout; do not create a worktree.
- Apply the fallback to both `REG_DWORD_LE` and `REG_DWORD_BE` in the flat hive parser and the `RegValue` API.
- Preserve the declared DWORD type and use raw bytes only when `KeyValue::data_size() > 8`.
- Do not emit a value-level error, log a parsing error, or increment `RunReport::num_errors` for the oversized branch.
- Keep current scalar behavior for sizes 0, 1, 2, 4, and 8, and keep current error behavior for sizes 3, 5, 6, and 7.
- Do not add SAM membership recognition, RID decoding, path matching, dependencies, or public API changes.
- Do not modify `dfir-nt-hive` or commit the supplied `sam.2012.data` artifact.
- Use the existing sanitized `test_data/hive/SAM.hive` to derive temporary test hives at runtime; do not add or modify binary fixtures.
- Never push or merge this branch.

## File Map

- Create `src/test_utils.rs`: test-only helper that changes one sanitized `REG_BINARY` value's declared type in a temporary hive copy and returns its original bytes.
- Modify `src/lib.rs`: compile `test_utils` only for crate tests.
- Modify `src/parser/hive.rs`: flat-output regression test and oversized DWORD raw-hex branch.
- Modify `src/registry_api.rs`: `RegValue` regression test and oversized DWORD raw-byte branch.
- Reference `docs/superpowers/specs/2026-07-22-oversized-dword-raw-fallback-design.md`: approved behavior contract.

---

### Task 1: Flat hive parser fallback

**Files:**

- Create: `src/test_utils.rs`
- Modify: `src/lib.rs:1-20`
- Modify: `src/parser/hive.rs:584-601,720-760`
- Test: `src/parser/hive.rs` inline test module

**Interfaces:**

- Consumes: `KeyValue::data_size() -> u32`, `KeyValue::dword_data() -> Result<u64>`, and existing `get_hex_value(&KeyValue, usize) -> Result<Value, Error>`.
- Produces: flat `data: Value::String("0x...")` for DWORD payloads larger than eight bytes, with the original `REG_DWORD_LE`/`REG_DWORD_BE` type and no parse error.
- Test support produces: `crate::test_utils::write_oversized_dword_fixture(&Path, KeyValueDataType) -> Vec<u8>`.

- [ ] **Step 1: Add the test-only sanitized fixture helper**

Add this module declaration after the production modules in `src/lib.rs`:

```rust
#[cfg(test)]
mod test_utils;
```

Create `src/test_utils.rs` with this complete content:

```rust
use std::{fs, path::Path};

use dfir_nt_hive::{Hive, KeyValueDataType};

const KEY_VALUE_DATA_TYPE_OFFSET: usize = 12;
const HIVE_BASE_BLOCK_SIZE: usize = 4096;

pub(crate) fn write_oversized_dword_fixture(
    target: &Path,
    data_type: KeyValueDataType,
) -> Vec<u8> {
    assert!(matches!(
        data_type,
        KeyValueDataType::RegDWord | KeyValueDataType::RegDWordBigEndian
    ));

    let mut hive_bytes = fs::read("test_data/hive/SAM.hive").unwrap();
    let (data_type_offset, raw_data) = {
        let hive = Hive::without_validation(hive_bytes.as_slice()).unwrap();
        let key = hive
            .root_key_node()
            .unwrap()
            .subpath("SAM\\Domains\\Account\\Users\\000001F4")
            .unwrap()
            .unwrap();
        let value = key.value("F").unwrap().unwrap();

        assert_eq!(value.data_type().unwrap(), KeyValueDataType::RegBinary);
        assert_eq!(value.data_size(), 80);

        (
            HIVE_BASE_BLOCK_SIZE + value.header_range.start + KEY_VALUE_DATA_TYPE_OFFSET,
            value.data().unwrap().into_vec().unwrap(),
        )
    };

    hive_bytes[data_type_offset..data_type_offset + std::mem::size_of::<u32>()]
        .copy_from_slice(&(data_type as u32).to_le_bytes());
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, hive_bytes).unwrap();

    raw_data
}
```

- [ ] **Step 2: Write the failing flat-parser test**

Append this test to the inline test module in `src/parser/hive.rs`:

```rust
#[test]
fn oversized_dword_values_fall_back_to_hex_without_errors() {
    for (data_type, expected_type, suffix) in [
        (KeyValueDataType::RegDWord, "REG_DWORD_LE", "le"),
        (
            KeyValueDataType::RegDWordBigEndian,
            "REG_DWORD_BE",
            "be",
        ),
    ] {
        let output_folder = ".tmp";
        let fixture = Path::new(output_folder).join(format!("oversized_dword_{suffix}.hive"));
        let raw_data = crate::test_utils::write_oversized_dword_fixture(&fixture, data_type);
        let base_file_name = format!("hive_values_oversized_dword_{suffix}");
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name,
            output_folder.to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            HashMap::new(),
        );
        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_values(
            fixture.to_str().unwrap(),
            RunConfiguration::new(vec![output_conf], true, None),
            plugin_config,
            Metadata::new("test".into()),
            None,
            Some(r"^\\SAM\\Domains\\Account\\Users\\000001F4$".to_owned()),
        );

        assert_eq!(report.num_errors, 0);
        assert_eq!(report.last_error, None);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let value = jsonl
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|line| line["name"] == "F")
            .unwrap();

        assert_eq!(value["type"], expected_type);
        assert_eq!(value["size"], 80);
        assert_eq!(value["data"], format!("0x{}", hex::encode(raw_data)));
        assert!(value.get("error").is_none());
    }
}
```

- [ ] **Step 3: Run the focused test and verify RED**

Run:

```bash
cargo test parser::hive::tests::oversized_dword_values_fall_back_to_hex_without_errors -- --exact
```

Expected: FAIL on the first `assert_eq!(report.num_errors, 0)` because the existing parser calls `dword_data()` for the 80-byte value and records one `InvalidWordSize` error.

- [ ] **Step 4: Implement the flat raw-hex branch**

Add this module constant near the existing hive value field constants in `src/parser/hive.rs`:

```rust
const MAX_DWORD_SCALAR_SIZE: u32 = 8;
```

Replace the separate `RegDWord` and `RegDWordBigEndian` match arms with:

```rust
KeyValueDataType::RegDWord | KeyValueDataType::RegDWordBigEndian => {
    let value_type = match value_type {
        KeyValueDataType::RegDWord => "REG_DWORD_LE",
        KeyValueDataType::RegDWordBigEndian => "REG_DWORD_BE",
        _ => unreachable!(),
    };
    record.add(HIVE_VALUE_TYPE, Value::String(value_type.to_owned()));

    let parsed_data = if value.data_size() > MAX_DWORD_SCALAR_SIZE {
        get_hex_value(&value, truncate_after)?
    } else {
        Value::Int(value.dword_data()? as i64)
    };
    record.add(HIVE_VALUE_DATA, parsed_data);
}
```

- [ ] **Step 5: Verify GREEN and ordinary DWORD behavior**

Run:

```bash
cargo fmt
cargo test parser::hive::tests::oversized_dword_values_fall_back_to_hex_without_errors -- --exact
cargo test parser::hive::tests::test_parse_hive_values -- --exact
```

Expected: formatting succeeds and both test commands exit 0. The first validates both endian variants as raw hex with no errors; the second retains its existing assertion that a normal DWORD serializes as integer `42`.

- [ ] **Step 6: Commit the flat-parser change**

```bash
git add src/lib.rs src/test_utils.rs src/parser/hive.rs
git commit -m "Handle oversized DWORDs in hive output"
```

---

### Task 2: RegValue API fallback

**Files:**

- Modify: `src/registry_api.rs:680-697,810-1050`
- Test: `src/registry_api.rs` inline test module

**Interfaces:**

- Consumes: the test helper from Task 1 and existing `get_bin_value(&KeyValue) -> Result<Vec<u8>, Error>`.
- Produces: `RegValue` with its original DWORD `DataType`, `int_data: None`, `bin_data: Some(raw_bytes)`, and `error: None` when its payload exceeds eight bytes. `RegValue::to_record()` serializes those bytes as `data: "0x..."` through existing behavior.

- [ ] **Step 1: Write the failing RegValue test**

Add these imports to the inline test module in `src/registry_api.rs`:

```rust
use std::path::Path;
```

Append this test:

```rust
#[test]
fn oversized_dword_values_use_raw_bytes_without_errors() {
    for (data_type, expected_type, suffix) in [
        (KeyValueDataType::RegDWord, "REG_DWORD_LE", "le"),
        (
            KeyValueDataType::RegDWordBigEndian,
            "REG_DWORD_BE",
            "be",
        ),
    ] {
        let fixture = Path::new(".tmp").join(format!("registry_oversized_dword_{suffix}.hive"));
        let raw_data = crate::test_utils::write_oversized_dword_fixture(&fixture, data_type);
        let registry = Registry::load_registry(fixture.to_str().unwrap(), "").unwrap();
        let key = registry
            .glob_keys(r"\SAM\Domains\Account\Users\000001F4")
            .unwrap()
            .pop()
            .unwrap();
        let value = key.value("F").unwrap();

        assert_eq!(value.data_type.to_string(), expected_type);
        assert_eq!(value.int_data, None);
        assert_eq!(value.bin_data.as_deref(), Some(raw_data.as_slice()));
        assert_eq!(value.error, None);

        let record = value.to_record();
        assert_eq!(
            record.get("data"),
            Some(&Value::String(format!("0x{}", hex::encode(raw_data))))
        );
        assert!(record.get("error").is_none());
    }
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test registry_api::tests::oversized_dword_values_use_raw_bytes_without_errors -- --exact
```

Expected: FAIL because the current `RegValue` adapter calls `dword_data()`, leaving `bin_data` unset and storing an `InvalidWordSize` message in `error`.

- [ ] **Step 3: Implement the RegValue raw-byte branch**

Add this module constant near the imports in `src/registry_api.rs`:

```rust
const MAX_DWORD_SCALAR_SIZE: u32 = 8;
```

Replace the separate `RegDWord` and `RegDWordBigEndian` match arms with:

```rust
KeyValueDataType::RegDWord | KeyValueDataType::RegDWordBigEndian => {
    hive_value.data_type = match value_type {
        KeyValueDataType::RegDWord => DataType::REG_DWORD_LE,
        KeyValueDataType::RegDWordBigEndian => DataType::REG_DWORD_BE,
        _ => unreachable!(),
    };

    if value.data_size() > MAX_DWORD_SCALAR_SIZE {
        hive_value.bin_data = Some(get_bin_value(&value)?);
    } else {
        hive_value.int_data = Some(value.dword_data()? as i64);
    }
}
```

- [ ] **Step 4: Verify GREEN and ordinary RegValue behavior**

Run:

```bash
cargo fmt
cargo test registry_api::tests::oversized_dword_values_use_raw_bytes_without_errors -- --exact
cargo test registry_api::tests::value_data -- --exact
```

Expected: formatting succeeds and both test commands exit 0. The first validates both oversized endian variants and their record serialization; the second retains normal DWORD integer behavior.

- [ ] **Step 5: Commit the RegValue change**

```bash
git add src/registry_api.rs
git commit -m "Handle oversized DWORDs in registry API"
```

---

### Task 3: Supplied SAM regression and complete verification

**Files:**

- Create temporarily: `/tmp/dfir_ogre_sam_regression.py`
- Read: `/home/asalais/dev/dfir-ogre/manual_tests/debug_data/sam.2012.data`
- Read: `test_data/hive/hive.xml`
- Verify: all tracked branch changes

**Interfaces:**

- Consumes: `dfir_ogre_common.parse_hive_values(...) -> RunReport` from the rebuilt extension.
- Produces: one flat value row with type `REG_DWORD_LE`, the exact raw hexadecimal payload, and zero report/value errors for the supplied SAM key.

- [ ] **Step 1: Create the external integration verifier**

Use `apply_patch` to create `/tmp/dfir_ogre_sam_regression.py` with this complete content:

```python
import json
import re
import tempfile
from pathlib import Path

from dfir_ogre_common import (
    Metadata,
    OutputConfiguration,
    PluginConfiguration,
    RunConfiguration,
    parse_hive_values,
)

HIVE = "/home/asalais/dev/dfir-ogre/manual_tests/debug_data/sam.2012.data"
MAPPING = "/home/asalais/dev/dfir-ogre/dfir-ogre-common/test_data/hive/hive.xml"
KEY = r"\HKLM\Sam\SAM\Domains\Account\Aliases\Members\S-1-5\00000014"
EXPECTED_DATA = "0xe9030000ee030000ed030000ef030000"

output_dir = Path(tempfile.mkdtemp(prefix="dfir-ogre-sam-"))
output = OutputConfiguration(
    "sam_regression",
    str(output_dir),
    format="jsonl",
    include_empty=True,
)
report = parse_hive_values(
    HIVE,
    RunConfiguration([output]),
    PluginConfiguration.load(MAPPING),
    Metadata("regression"),
    r"\HKLM\Sam",
    f"^{re.escape(KEY)}$",
)
rows = [
    json.loads(line)
    for line in (output_dir / "sam_regression.hive.jsonl").read_text().splitlines()
]

summary = {
    "last_error": report.last_error,
    "num_errors": report.num_errors,
    "rows": rows,
}
print(json.dumps(summary, sort_keys=True))

assert report.last_error is None, summary
assert report.num_errors == 0, summary
assert len(rows) == 1, summary
assert rows[0]["type"] == "REG_DWORD_LE", summary
assert rows[0]["size"] == 16, summary
assert rows[0]["data"] == EXPECTED_DATA, summary
assert rows[0].get("error") is None, summary
```

- [ ] **Step 2: Format and run all Rust tests**

Run:

```bash
cargo fmt --check
cargo test --locked
```

Expected: both commands exit 0; all unit and documentation tests pass.

- [ ] **Step 3: Run strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Expected: exit 0 with no warnings.

- [ ] **Step 4: Rebuild the Python extension in the manual-test environment**

Run from `/home/asalais/dev/dfir-ogre/dfir-ogre-common`:

```bash
env VIRTUAL_ENV=/home/asalais/dev/dfir-ogre/manual_tests/.venv uvx --cache-dir /tmp/dfir-ogre-uv-cache --from 'maturin>=1.9,<2.0' maturin develop --locked
```

Expected: maturin builds the branch and installs it into `/home/asalais/dev/dfir-ogre/manual_tests/.venv`.

- [ ] **Step 5: Run the supplied SAM regression verifier**

Run:

```bash
/home/asalais/dev/dfir-ogre/manual_tests/.venv/bin/python /tmp/dfir_ogre_sam_regression.py
```

Expected: exit 0 with one row, `num_errors` equal to 0, no `last_error` or value `error`, and `data` equal to `0xe9030000ee030000ed030000ef030000`.

- [ ] **Step 6: Inspect the final branch state**

Run:

```bash
git diff --check
git status --short --branch
git log --oneline --decorate main..HEAD
```

Expected: no whitespace errors; no uncommitted tracked changes; the branch contains the design, implementation plan, flat-parser implementation, and RegValue implementation commits, with no binary fixture, dependency, or `dfir-nt-hive` changes.
