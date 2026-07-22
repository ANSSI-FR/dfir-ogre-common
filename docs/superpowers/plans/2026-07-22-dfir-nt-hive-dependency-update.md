# dfir-nt-hive Dependency Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update `dfir-ogre-common` to the `dfir-nt-hive` revision that correctly parses large contiguous values in REGF 1.3 hives.

**Architecture:** Preserve the floating Git dependency in `Cargo.toml` and change only the resolved source revision in `Cargo.lock`. Use the installed Python extension and the supplied AmCache hive as an external red/green integration test, then run the full Rust verification suite before committing the lockfile.

**Tech Stack:** Rust 2024, Cargo, PyO3, maturin, uv, Python 3.13, JSON Lines.

## Global Constraints

- Work on branch `fix/update-dfir-nt-hive` in the existing `dfir-ogre-common` checkout; do not create a worktree.
- Keep `Cargo.toml`, Rust source files, and public APIs unchanged.
- Resolve `dfir-nt-hive` to exact commit `310be0f89ad1d6fb7c5ed0f15ef16a6a1137e3f0`.
- Use `/home/asalais/dev/dfir-ogre/manual_tests/debug_data/amcache.2012.data` only as an external validation input; do not commit it.
- Verify key `\\AmCache\\Root\\Programs\\00002b8358eeddd1e2473faba7d97870d8070000ffff` and its `Files` and `d` values.
- Rebuild the extension into `/home/asalais/dev/dfir-ogre/manual_tests/.venv`.
- Do not push or merge the branch.

---

### Task 1: Establish the failing AmCache integration test

**Files:**

- Create temporarily: `/tmp/dfir_ogre_amcache_regression.py`
- Read: `/home/asalais/dev/dfir-ogre/dfir-ogre-common/test_data/hive/hive.xml`
- Read: `/home/asalais/dev/dfir-ogre/manual_tests/debug_data/amcache.2012.data`

**Interfaces:**

- Consumes: `dfir_ogre_common.parse_hive_values(...) -> RunReport` from the installed extension.
- Produces: a repeatable command that exits nonzero with the old dependency and exits zero after rebuilding with the fixed dependency.

- [ ] **Step 1: Create the temporary verifier**

Use `apply_patch` to create `/tmp/dfir_ogre_amcache_regression.py` with this content:

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

HIVE = "/home/asalais/dev/dfir-ogre/manual_tests/debug_data/amcache.2012.data"
MAPPING = "/home/asalais/dev/dfir-ogre/dfir-ogre-common/test_data/hive/hive.xml"
KEY = r"\AmCache\Root\Programs\00002b8358eeddd1e2473faba7d97870d8070000ffff"

output_dir = Path(tempfile.mkdtemp(prefix="dfir-ogre-amcache-"))
output = OutputConfiguration(
    "amcache_regression",
    str(output_dir),
    format="jsonl",
    include_empty=True,
)
report = parse_hive_values(
    HIVE,
    RunConfiguration([output]),
    PluginConfiguration.load(MAPPING),
    Metadata("regression"),
    r"\AmCache",
    f"^{re.escape(KEY)}$",
)

rows = [
    json.loads(line)
    for line in (output_dir / "amcache_regression.hive.jsonl").read_text().splitlines()
]
values = {row["name"]: row for row in rows}
errors = {
    name: row.get("error")
    for name, row in values.items()
    if row.get("error")
}

summary = {
    "report_errors": report.num_errors,
    "rows": len(rows),
    "value_errors": errors,
}
print(json.dumps(summary, sort_keys=True))

assert len(rows) == 12, summary
assert values["Files"]["size"] == 359_720, summary
assert values["d"]["size"] == 76_178, summary
assert report.num_errors == 0, summary
assert not errors, summary
assert len(values["Files"]["data"]) == 3_747, summary
assert len(values["d"]["data"]) == 511, summary
```

- [ ] **Step 2: Run the verifier and confirm RED**

Run:

```bash
/home/asalais/dev/dfir-ogre/manual_tests/.venv/bin/python /tmp/dfir_ogre_amcache_regression.py
```

Expected: exit 1 at `assert report.num_errors == 0`, with `report_errors` equal to 2. The summary must contain an `Invalid offset` error for `Files` and the `0x0070105c` header error for `d`.

---

### Task 2: Refresh and verify the dependency lock

**Files:**

- Modify: `/home/asalais/dev/dfir-ogre/dfir-ogre-common/Cargo.lock:462`
- Verify unchanged: `/home/asalais/dev/dfir-ogre/dfir-ogre-common/Cargo.toml:38`
- Test: `/tmp/dfir_ogre_amcache_regression.py`

**Interfaces:**

- Consumes: Git dependency `https://github.com/ANSSI-FR/dfir-nt-hive.git` and exact revision `310be0f89ad1d6fb7c5ed0f15ef16a6a1137e3f0`.
- Produces: a `Cargo.lock` entry resolving `dfir-nt-hive` to the fixed revision and a rebuilt Python extension in the manual-test virtual environment.

- [ ] **Step 1: Update only the resolved dependency revision**

Run from `/home/asalais/dev/dfir-ogre/dfir-ogre-common`:

```bash
cargo update -p dfir-nt-hive --precise 310be0f89ad1d6fb7c5ed0f15ef16a6a1137e3f0
```

Expected: Cargo updates `dfir-nt-hive` from `f6906c5cfc430ee012f0dc7717e40287bcafdc33` to `310be0f89ad1d6fb7c5ed0f15ef16a6a1137e3f0` without changing other packages.

- [ ] **Step 2: Inspect the lockfile-only diff**

Run:

```bash
git diff -- Cargo.toml Cargo.lock
```

Expected: `Cargo.toml` has no diff and the only `Cargo.lock` change is the `dfir-nt-hive` source hash.

Run:

```bash
rg -n -C 2 'name = "dfir-nt-hive"|source = "git\+https://github.com/ANSSI-FR/dfir-nt-hive.git' Cargo.lock
```

Expected: the source ends in `#310be0f89ad1d6fb7c5ed0f15ef16a6a1137e3f0`.

- [ ] **Step 3: Run the complete Rust test suite**

Run:

```bash
cargo test --locked
```

Expected: exit 0 with every unit and documentation test passing.

- [ ] **Step 4: Verify formatting**

Run:

```bash
cargo fmt --check
```

Expected: exit 0 with no formatting diff.

- [ ] **Step 5: Run strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Expected: exit 0 with no warnings.

- [ ] **Step 6: Rebuild and install the Python extension**

Run from `/home/asalais/dev/dfir-ogre/dfir-ogre-common`:

```bash
env VIRTUAL_ENV=/home/asalais/dev/dfir-ogre/manual_tests/.venv uvx --cache-dir /tmp/dfir-ogre-uv-cache --from 'maturin>=1.9,<2.0' maturin develop --locked
```

Expected: maturin builds `dfir-ogre-common` with the locked `dfir-nt-hive` revision and installs it into the specified virtual environment.

- [ ] **Step 7: Rerun the same verifier and confirm GREEN**

Run:

```bash
/home/asalais/dev/dfir-ogre/manual_tests/.venv/bin/python /tmp/dfir_ogre_amcache_regression.py
```

Expected: exit 0 with `report_errors` equal to 0, 12 rows, 3,747 strings in `Files`, and 511 strings in `d`.

- [ ] **Step 8: Perform the final diff checks**

Run:

```bash
git diff --check
git status --short
git diff --stat
```

Expected: no whitespace errors; only `Cargo.lock` is modified; the stat reports one changed line and one removed line.

- [ ] **Step 9: Commit the verified lockfile update**

```bash
git add Cargo.lock
git commit -m "Update dfir-nt-hive dependency"
```

Expected: one commit containing only the `Cargo.lock` source revision update.

- [ ] **Step 10: Verify the committed branch state**

Run:

```bash
git status --short --branch
git log --oneline --decorate -3
```

Expected: a clean `fix/update-dfir-nt-hive` branch with the lockfile update commit above the design and plan commits; nothing is pushed or merged.
