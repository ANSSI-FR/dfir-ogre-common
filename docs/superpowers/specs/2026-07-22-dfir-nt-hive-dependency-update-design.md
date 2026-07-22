# dfir-nt-hive Dependency Update Design

## Problem

`dfir-ogre-common` currently resolves `dfir-nt-hive` to commit `f6906c5`, which predates the REGF 1.3 large-value parsing fix. Consequently, the Python extension misinterprets the large contiguous `Files` and `d` values in the supplied AmCache hive as segmented REGF 1.5 data.

## Dependency Update

Keep the floating Git dependency declaration in `Cargo.toml` unchanged. Refresh only `Cargo.lock` so `dfir-nt-hive` resolves to a revision that contains commit `310be0f`, the version-aware large-value fix. Abort the update if the resolved revision does not contain that fix.

No `dfir-ogre-common` production code or public API changes are required.

## Regression Verification

Before updating the lockfile, run the existing Python extension against the supplied `amcache.2012.data` hive and confirm the reported key still produces errors for `Files` and `d`. After updating, rebuild and install the extension into `manual_tests/.venv`, rerun the same parse, and confirm all 12 values parse without errors.

Also run the repository-wide Rust test suite, formatting check, and strict Clippy check with the refreshed lockfile. The 3.7 MB AmCache hive remains an external manual-test input and is not committed to `dfir-ogre-common`.

## Repository Workflow

Perform the update on branch `fix/update-dfir-nt-hive` in the existing checkout, without a worktree. Commit the design, implementation plan, and lockfile update on that branch; do not push or merge it.
