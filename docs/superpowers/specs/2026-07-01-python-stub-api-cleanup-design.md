# Python Stub API Cleanup Design

## Goal

Synchronize the Python type stub with the current Rust/PyO3 module surface and make one conservative Python API cleanup. This pass must not attempt aggressive pruning of PyO3-compatible Rust internals; that will be handled separately.

## Context

`src/lib_py.rs` exports Python classes and functions through the `dfir_ogre_common` module. `dfir_ogre_common.pyi` has drifted from that runtime surface:

- `parse_hive_values` is exported by Rust but missing from the stub.
- `DateInputCodec.TimestampHex` exists in Rust but is missing from the stub.
- `parse_sqlite` has a stub parameter typo: `run_donfig` instead of `run_config`.
- `OutputReport` is declared in the stub and returned by `Output.get_report()`, but it is not currently added to the Python module in `src/lib_py.rs`.

Sibling source scans of `../dfir-ogre` and `../dfir-ogre-plugin-windows`, excluding `.venv`, show active use of the core plugin API, parser wrappers, `Output`, config/report objects, registry objects, `Record`/`Value`, custom parser support, and Windows parser extensions.

## Design

### Stub Synchronization

Update `dfir_ogre_common.pyi` to match the exported runtime API:

- Add `DateInputCodec.TimestampHex()`.
- Add `parse_hive_values(input_file, run_config, plugin_config, metadata, root_name=None, regexp=None) -> RunReport`.
- Fix `parse_sqlite` to use `run_config: RunConfiguration`.
- Keep `Output` documented as a context manager; do not add an `Output.close()` Python API.

### Runtime Export Correction

Export `OutputReport` from `src/lib_py.rs`.

Rationale: `Output.get_report()` returns `OutputReport`, `RunReport.add_output_report()` accepts it, and the stub already describes it. Adding the class export aligns runtime behavior with the existing public report surface.

### Conservative Breaking Cleanup

Remove `ParserType` from the Python module exports by deleting `m.add_class::<ParserType>()` from `src/lib_py.rs`.

Detailed impact:

- Current runtime export: `ParserType`.
- Current stub coverage: no `ParserType` class is declared.
- Observed sibling source usage: no direct imports or references in `../dfir-ogre` or `../dfir-ogre-plugin-windows` source, excluding `.venv`.
- Reason to remove: `ParserType` is an internal recursive parser-tree enum. Python callers use `FieldParserTree.get_parser()` and `FieldParserTree.get_parser_subtree()` rather than constructing or matching `ParserType` directly.
- Breaking behavior: external code doing `from dfir_ogre_common import ParserType` will fail after this change.

Do not remove `ParserType`'s Rust `#[pyclass]` or `from_py_object` compatibility in this pass. That requires a separate PyO3 hot-path audit because `FieldParserTree` still stores `ParserType` internally and remains Python-visible.

## Non-Goals

- Do not aggressively prune public APIs such as `Field`, `Parser`, `FieldMapping`, `FieldParser`, `FieldParserTree`, `Record`, `Value`, `Output`, timeline builders, registry types, or Windows parser-extension functions.
- Do not remove PyO3 compatibility attributes from hot-path Rust types in this pass.
- Do not add or run Python unit tests in this repository.
- Do not change the `Output` Python lifecycle; Python code continues to use `with Output(...) as output:`.

## Validation

Use Rust-side validation only:

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`

Also run source scans for the candidate breaking removal:

- Confirm `ParserType` is not imported or referenced by sibling source outside `.venv`.
- Confirm `dfir_ogre_common.pyi` contains the new and corrected declarations.
