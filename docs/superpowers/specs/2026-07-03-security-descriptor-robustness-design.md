# Security Descriptor Robustness Design

## Goal

Make Windows security descriptor parsing panic-free, deterministic, and easier to reason about while preserving best-effort output for malformed forensic data.

The immediate bug is a panic in `security_descriptor_from_bytes` for short inputs: it checks for fewer than 12 bytes, then later reads `b[12..16]`. The broader issue is that the parser uses direct slices for untrusted binary offsets, collapses multiple ACEs into one parsed ACE, ignores object ACE GUID fields, and relies on unordered flag collections.

## Scope

In scope:

- Make `security_descriptor_from_bytes`, ACL parsing, ACE parsing, and SID conversion avoid panics on short or malformed input.
- Keep the public parser behavior best-effort: return the most complete descriptor possible instead of failing on truncation.
- Replace single `sacl_ace` and `dacl_ace` fields with plural ACE arrays.
- Preserve unknown or partially decoded ACEs when enough bytes exist to identify them.
- Decode object ACE GUID fields when present.
- Make flag output order deterministic.
- Update Rust tests, Python type hints, and the hive XML mapping for the schema change.

Out of scope:

- Strict validation of all Windows security descriptor invariants.
- Public exploit details or crash proof-of-concept documentation.
- Python unit tests. Python extension testing is handled outside this repository.
- Registry or hive parser behavior unrelated to security descriptor output.

## Parser Model

Keep the existing public entry point:

```rust
pub fn security_descriptor_from_bytes(b: &[u8]) -> Result<SecurityDescriptor, Error>
```

The parser continues to treat the first four bytes as the size prefix returned by the registry library. Security descriptor offsets remain relative to the descriptor body, so the current `sd_start = 4` model stays intact.

Direct indexing and slicing will be replaced by small bounded helpers:

- `read_u16_le(bytes, offset) -> Option<u16>`
- `read_u32_le(bytes, offset) -> Option<u32>`
- `slice_at(bytes, offset, len) -> Option<&[u8]>`
- `sid_at(bytes, offset) -> Option<String>`
- `acl_at(bytes, offset) -> Vec<SecurityDescriptorAce>`

Helpers should return `None` for truncation or out-of-range offsets. The caller then keeps already decoded fields and stops only the affected substructure.

## Data Model

`SecurityDescriptor` changes from one optional ACE per ACL to full ACE arrays:

```rust
pub struct SecurityDescriptor {
    control_flags: Vec<String>,
    owner_sid: String,
    group_sid: String,
    sacl_aces: Vec<SecurityDescriptorAce>,
    dacl_aces: Vec<SecurityDescriptorAce>,
}
```

`SecurityDescriptorAce` keeps existing fields and adds the pieces needed to represent unknown, object, and truncated ACEs:

```rust
pub struct SecurityDescriptorAce {
    ace_type: Option<String>,
    ace_flags: Vec<String>,
    rights: Option<Vec<String>>,
    account_sid: Option<String>,
    ace_size: Option<u16>,
    object_type_guid: Option<String>,
    inherited_object_type_guid: Option<String>,
    raw_hex: Option<String>,
}
```

`account_sid` becomes optional because some unsupported or truncated ACEs may not contain a recoverable SID. `raw_hex` should be emitted for unknown or partial ACEs where retaining source bytes helps downstream inspection. It does not need to be emitted for every normal ACE.

## ACL and ACE Parsing

ACL parsing should read each ACE independently:

1. Read the ACL header only when enough bytes exist.
2. Read `ace_count` from the ACL header.
3. For each ACE, read the ACE header and declared size.
4. If the full ACE is available, parse it normally.
5. If only a partial ACE is available, emit a partial `SecurityDescriptorAce` with known metadata and `raw_hex`.
6. Stop parsing that ACL when the next ACE header cannot be read or the declared ACE size is invalid.

This fixes the current behavior where all ACE bytes are appended into one buffer and only the first ACE is parsed.

Known ACE parsing should decode:

- ACE type
- ACE flags
- access mask rights
- account SID when recoverable
- object ACE GUIDs when the ACE type and object flags indicate they are present

Unknown ACE types should still produce an ACE object with any known metadata:

- `ace_type` as a stable unknown representation
- `ace_flags`
- `ace_size`
- `raw_hex`
- `account_sid` only if a SID can be recovered without guessing

## Deterministic Flags

Control flags, ACE flags, rights, and object ACE flags should be decoded from ordered static tables. Avoid `HashMap` or `HashSet` in the final output path because they make field order nondeterministic.

The output should be stable across runs, which allows exact unit tests and predictable Python-facing values.

## Record and Python Output

`SecurityDescriptor::to_record` should emit:

- `owner_sid`
- `group_sid`
- `control_flags`
- `sacl_aces`
- `dacl_aces`

`sacl_aces` and `dacl_aces` are arrays of ACE objects. Empty arrays are acceptable when the ACL is absent, truncated before any ACE, or present with zero ACEs.

`SecurityDescriptorAce::to_record` should omit optional fields that are not known, following the current style for optional ACE fields. The Python type hints in `dfir_ogre_common.pyi` must match the plural names and optional ACE fields.

The hive XML mapping in `test_data/hive/hive.xml` must change from single nested objects:

```xml
<object input="dacl_ace">
```

to array mappings:

```xml
<array input="dacl_aces">
```

with the same nested ACE field mappings adjusted for optional `account_sid` and the new diagnostic fields where appropriate.

## Error Handling

Malformed or truncated descriptors should not produce parser errors when a partial result can be returned. Best-effort behavior means:

- Too short for control flags: return a default descriptor.
- Too short for owner/group offsets: return fields decoded so far.
- Offset points outside the buffer: leave that subfield empty and continue where possible.
- ACL is truncated: return any ACEs decoded before the truncation.
- ACE is truncated: emit a partial ACE when the ACE header is available.
- SID is truncated: omit `account_sid` for that ACE or leave the descriptor SID empty.

`Error` remains available for truly unexpected conversion failures, but normal malformed binary data should be handled through `Option`-based helpers instead of `try_into` errors from slices.

## Tests

Rust tests will be deterministic and local to `src/windows_utils.rs`.

Required coverage:

- Short descriptor inputs around the old panic boundary, including total lengths 12 through 15.
- Existing valid descriptor still decodes owner SID, group SID, control flags, and DACL ACE fields.
- Valid descriptor with multiple DACL ACEs returns multiple `dacl_aces`.
- Truncated owner, group, SACL, and DACL offsets return partial descriptors without panics.
- Truncated ACL header returns an empty ACE list.
- Truncated ACE header or ACE body returns a partial ACE when enough metadata exists.
- Unknown ACE type is preserved with `ace_type`, `ace_size`, and `raw_hex`.
- Object ACEs decode `object_type_guid` and `inherited_object_type_guid` when flags say those GUIDs are present.
- Flag vectors have deterministic order.
- `convert_sid` keeps direct tests for valid, empty, and truncated SID bytes.

The current randomized fuzz-style tests should be replaced or reduced to deterministic table tests. Random tests did not reliably catch the known length-12-to-15 panic and make failures harder to reproduce.

Validation commands:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

If `cargo test` cannot locate the local Python shared library, run it with the same `LD_LIBRARY_PATH` used previously:

```bash
LD_LIBRARY_PATH=/home/asalais/.local/share/uv/python/cpython-3.13.5-linux-x86_64-gnu/lib cargo test
```
