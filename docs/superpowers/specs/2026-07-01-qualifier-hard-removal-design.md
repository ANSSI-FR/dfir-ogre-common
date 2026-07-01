# Qualifier Hard Removal Design

## Goal

Remove the qualifier runtime feature from `dfir-ogre-common`.

Qualifiers are currently inactive in production and add branching, duplicate output construction, public API surface, and validation state. This change removes the feature rather than preserving it behind no-op compatibility layers. XML plugin definitions may still contain `qualifier="..."`, but that attribute becomes descriptive only and accepts any string.

This design intentionally does not optimize or refactor unrelated output code. Broader cleanup can happen in a later change.

## Scope

In scope:

- Remove the qualifier registry and authorized qualifier list.
- Remove validation of XML `qualifier="..."` values.
- Keep XML `qualifier="..."` accepted on field-like mapping nodes, but ignore it at runtime.
- Remove qualified output names such as `field:qualifier`.
- Remove `with_qualifiers` from output configuration and file reports.
- Remove the exported Python `Qualifiers` class.
- Remove qualifier storage from `FieldName`.
- Change `FieldName.name(with_qualifier)` to `FieldName.name()`.
- Update Rust tests and fixtures expectations to assert plain output names only.

Out of scope:

- Broader output pipeline refactors.
- Output schema changes unrelated to qualifier removal.
- Renaming existing plain output fields.
- Python unit tests. Python extension testing is handled outside this repository.

## XML Configuration

Existing XML plugin files remain loadable when they contain `qualifier` attributes. The parser will no longer create a `Qualifiers` registry, look up qualifier names, or reject unknown qualifier strings.

The `qualifier` attribute is accepted as free-form descriptive XML metadata. It is not converted to a canonical value, stored in `FieldName`, exposed through Python, or used when output records are built.

Examples that must keep loading:

```xml
<field input="AppId" output="app_id" parser="String" qualifier="APP_ID" />
<field input="custom" output="custom" parser="String" qualifier="ANY_ARBITRARY_VALUE" />
```

Both examples produce plain output keys: `app_id` and `custom`.

## Rust Data Model

`FieldName` will keep only the metadata needed for parsing and output:

- `in_name`
- `out_name`
- `primary_key`
- `display_name`
- `description`

The following fields are removed:

- `qualifier`
- `qualified_name`

`FieldName::new` no longer accepts a qualifier argument. `FieldName::name()` always returns the plain output name. `Field::name()` follows the same shape and no longer accepts a qualifier flag.

Generated fields from parser extensions, such as Windows FRN and signed hash parsers, will construct plain `FieldName` values directly.

## Output Pipeline

`OutputConfiguration` no longer carries `with_qualifiers`, so `Output` no longer needs to detect whether any configured output requires qualified field names.

`Output` will own a single `LineBuilder`. All configured writers receive the same plain output record. The dual builder path is removed:

- no `line_builder_with_qualifiers`
- no `line_builder_without_qualifiers`
- no cloning input records solely to support mixed qualified/unqualified outputs
- no output writer routing by qualifier mode

`LineBuilder` no longer has `require_qualifiers`. Nested object and array traversal uses the same plain key behavior everywhere.

## Python API Changes

These breaking Python API changes are approved:

- `OutputConfiguration.__init__` removes `with_qualifiers`.

Current shape:

```python
OutputConfiguration(
    base_file_name,
    output_folder,
    output_type="file",
    format="jsonl",
    date_format="iso_utc",
    with_timeline=False,
    with_qualifiers=False,
    include_empty=True,
    params={},
)
```

New shape:

```python
OutputConfiguration(
    base_file_name,
    output_folder,
    output_type="file",
    format="jsonl",
    date_format="iso_utc",
    with_timeline=False,
    include_empty=True,
    params={},
)
```

- `OutputConfiguration.with_qualifiers` is removed.
- `FileReport.with_qualifiers` is removed.
- `Qualifiers` is no longer exported from `dfir_ogre_common`.
- `FieldName.__init__` removes `qualifier`.

Current shape:

```python
FieldName(
    input_name,
    primary_key=False,
    output_name=None,
    qualifier=None,
    display_name=None,
    description=None,
)
```

New shape:

```python
FieldName(
    input_name,
    primary_key=False,
    output_name=None,
    display_name=None,
    description=None,
)
```

- `FieldName.name(with_qualifier: bool)` becomes `FieldName.name()`.
- XML qualifier strings are not surfaced in Python.

The public type hints in `dfir_ogre_common.pyi` must match these changes.

## Error Handling

`Error::UnknownQualifier` is removed when no longer referenced. Unknown XML qualifier strings do not produce errors.

Other configuration errors, parser errors, and output errors remain unchanged.

## Tests

Rust tests will be updated to verify the new behavior:

- XML with a known qualifier still loads.
- XML with an unknown/free-form qualifier loads.
- Outputs use plain field names even when XML contains `qualifier`.
- Multiple output configurations receive the same plain record.
- Parser fixture tests keep existing XML fixtures with qualifier attributes to prove compatibility.

Qualifier-specific tests that only validate `field:qualifier` output names are removed or rewritten around plain names.

Validation commands:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Migration Notes

Python callers must remove:

- `with_qualifiers` constructor arguments and attribute reads
- imports or construction of `Qualifiers`
- `qualifier=` when constructing `FieldName`
- arguments passed to `FieldName.name(...)`

XML plugin authors do not need to remove `qualifier="..."`. Existing attributes continue to parse, but they no longer affect output.
