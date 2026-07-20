# Hive Values Key Metadata and Complete-Key Output Design

## Goal

Make `parse_hive_values` emit a dedicated flat value record that carries its parent key's modification time and security descriptor, while ensuring that every successfully enumerated descendant key produces at least one output row.

Keys without registry values will produce one explicit placeholder row. Parsing remains best effort so recoverable corruption in one key, metadata field, value, or subtree does not stop unrelated keys from being processed.

## Current Behavior

`parse_hive_values` currently emits one row for each registry value and no row for a key without values. The `testhive` fixture demonstrates the resulting coverage gap:

- `parse_hive_keys` sees 527 descendant keys.
- Only two of those keys contain values.
- Those two keys contain 12 values in total.
- `parse_hive_values` therefore emits only 12 rows and silently omits 525 empty keys.

Value rows currently contain `path` plus value-level fields such as `name`, `data`, `type`, and `size`. They do not contain the key's `mtime` or security descriptor. The shared `test_data/hive/hive.xml` mapping also injects an irrelevant empty `values` array into flat value output because it is designed primarily for `parse_hive_keys`.

## Scope

In scope:

- Add key `mtime` and security descriptor data to every value row.
- Emit one placeholder row for each descendant key that has no values.
- Distinguish placeholders from genuine unnamed/default registry values.
- Preserve a row for a key when its value list cannot be enumerated.
- Make recoverable traversal, metadata, and value failures best effort.
- Give `parse_hive_values` a dedicated flat XML mapping.
- Preserve `parse_hive_keys` behavior through a separate key-oriented mapping fixture.
- Test JSONL, regular CSV, normalized CSV, placeholder behavior, metadata, and continued output after recoverable errors.

Out of scope:

- Emitting the hive root node. Both hive parsers continue to begin with the root's children.
- Changing public Rust or Python function signatures.
- Routing value parsing through `RegKey` or changing the registry API.
- Changing truncation rules or the representation of genuine registry value types.
- Correcting unrelated existing type labels, including the genuine `RegNone` handling.
- Adding Python unit tests; Python extension tests belong to the separate Python project.
- Adding or modifying binary hive fixtures.

## Chosen Architecture

`parse_hive_values` will keep its direct `KeyNode` traversal and produce one flat row per real value. Key metadata will be extracted once for each key into a base record. The base record will then be cloned for each value or for the single placeholder row.

This design was selected over two alternatives:

- A shared union schema would preserve one XML mapping but retain ambiguous fields and irrelevant output such as `values: []`.
- A nested `{ key, value }` record would be tidy for JSON but add avoidable complexity to regular and normalized CSV output.

The flat schema is self-contained, works naturally as one CSV row, and does not require hard-coded mapping selection inside either public parser.

## Output Contract

Every successfully parsed real value row contains:

- `path: String` — full descendant key path, including an optional `root_name` prefix.
- `mtime: Date` — modification time of the containing key.
- `security_descriptor: Object` — parsed security descriptor of the containing key.
- `name: String` — registry value name; an unnamed genuine value remains normalized to `Default`.
- `data: Value` — the existing typed value representation.
- `type: String` — the existing registry type label.
- `size: Int` — the source value data size.
- `is_placeholder: Bool` — always `false` for a real or partially parsed source value.

The existing optional fields remain available:

- `invalid_signature: Bool` when a value signature is invalid.
- `error: String` when a value or value-list operation fails.

Output infrastructure continues to add `ogre_md` or its normalized metadata identifier. The mapping data type remains `hive`.

A representative real value row is:

```json
{
  "path": "\\big-data-test",
  "mtime": "2023-01-18T11:39:48.154060+00:00",
  "security_descriptor": {
    "owner_sid": "S-1-5-32-544",
    "group_sid": "S-1-5-32-544",
    "control_flags": ["SE_DACL_PRESENT", "SE_SELF_RELATIVE"],
    "sacl_aces": [],
    "dacl_aces": [
      {
        "ace_type": "ACCESS_ALLOWED_ACE_TYPE",
        "ace_flags": ["CONTAINER_INHERIT_ACE"],
        "rights": ["KEY_ALL_ACCESS"],
        "account_sid": "S-1-5-18",
        "ace_size": 20
      }
    ]
  },
  "name": "A",
  "data": "0x41414141",
  "type": "REG_BINARY",
  "size": 4,
  "is_placeholder": false,
  "ogre_md": {
    "computer": "test",
    "data_type": "hive"
  }
}
```

## Placeholder Contract

If no real value row is emitted for a successfully enumerated key, the parser emits exactly one placeholder with these value fields:

```json
{
  "name": "Default",
  "data": "",
  "type": "REG_NONE",
  "size": 0,
  "is_placeholder": true
}
```

The placeholder also receives the key's `path`, valid `mtime`, and valid `security_descriptor` from the base record.

`is_placeholder` is required because a genuine unnamed registry value can also be called `Default`, contain empty data, or use `REG_NONE`. A genuine value always uses `is_placeholder: false`.

The placeholder does not claim that a source value existed. It is an output representation that preserves the otherwise-empty key and makes its synthetic nature machine-readable.

## Traversal and Data Flow

For each matching descendant key, `parse_subvalues` will:

1. Recover the key node and name and construct its full path.
2. Apply the existing regular-expression filter. Non-matching keys are not emitted, but their descendants are still traversed as today.
3. Read `mtime` and the security descriptor once into a base record.
4. Request the key's value iterator.
5. Clone the base record for each source value.
6. Set `is_placeholder: false` before parsing the source value so even a partial error row cannot be confused with a placeholder.
7. Parse the existing value fields and write the best available row.
8. If no source value row was emitted, clone the base record and write the placeholder.
9. Recurse into the key's subkeys.

Metadata must not be re-read for every value. `RegKey` will not be introduced into this path because it would duplicate parsing, retain a separate collection of values, and bypass the current `truncate_values_after` behavior.

## Best-Effort Error Handling

Recoverable hive corruption must be isolated to the smallest practical unit.

### Child and key errors

- A malformed child entry is recorded in `RunReport` and skipped; later sibling entries continue.
- If a key name cannot be recovered, the key cannot be assigned a trustworthy path. Record the error, skip that key, and continue with siblings.
- If a subkey collection cannot be read, record the error and abandon only that inaccessible subtree.

### Metadata errors

- If `mtime` conversion fails, add a null value for the mapped field, record the error, and continue emitting the key's rows.
- If the security descriptor cannot be retrieved or parsed, add a null value for the mapped object, record the error, and continue emitting the key's rows.
- JSON omits these null fields unless `include_empty` is enabled. Schema-driven CSV retains an empty cell in the correct column.
- Key metadata errors are kept in `RunReport` rather than duplicated into every value row for that key.

### Value errors

- `None` or an empty value iterator produces the normal placeholder.
- If the value iterator itself cannot be created, record the error in `RunReport` and emit a placeholder carrying the same error in its `error` field.
- If one iterator item or value body cannot be parsed, write the best available partial row with `is_placeholder: false` and `error`, record the error in `RunReport`, and continue with later values and keys.
- Once any real or partial source value row has been emitted, do not add an additional placeholder for that key.

### Fatal boundaries

Failures remain fatal when meaningful or consistent continuation is not possible:

- Invalid plugin or run configuration.
- Invalid regular expression.
- Unreadable hive file.
- Hive or root-node construction failure.
- Output writer or finalization failure.

Output I/O failures must not be swallowed because continuing could silently produce inconsistent output files.

## Dedicated XML Mappings

`test_data/hive/hive.xml` becomes the value-oriented mapping used by `parse_hive_values` tests. Its ordered top-level fields will be:

1. `path`
2. `mtime`
3. `security_descriptor`
4. `name`
5. `data`
6. `type`
7. `size`
8. `is_placeholder`
9. `invalid_signature`
10. `error`

The mapping will:

- Remove the nested `values` array.
- Rename the descriptor object from `descriptor` to `security_descriptor`.
- Preserve all current security descriptor child fields and ACE arrays.
- Map `is_placeholder` and `invalid_signature` with `Bool` parsers.
- Keep `data` capable of retaining its pre-parsed `String`, `Int`, or `Array` value.
- Keep `data_type="hive"`, `truncate_values_after`, the default date pattern, and the ignore-by-default policy.

A new `test_data/hive/hive_keys.xml` will contain the existing key-oriented mapping, including the nested `values` array and `descriptor` object. Existing `parse_hive_keys` tests will load that mapping and retain their current output contract.

These are caller-supplied schemas. Neither parser will hard-code a data-type name or add a new public argument to choose between mappings.

## Timeline Behavior

The value mapping keeps the existing standard timeline definition with these updates:

- The related-user path becomes `security_descriptor.owner_sid`.
- `mtime` remains the modification timestamp.
- The description includes both `path` and `name` so multiple values from one key remain distinguishable.

When timeline output is enabled, each real value row can produce a timeline event. A placeholder can also produce an event, ensuring that an empty key remains visible. Several values belonging to the same key intentionally share the key's `mtime`.

## Tests and Acceptance Criteria

Tests stay in the existing `#[cfg(test)]` module in `src/parser/hive.rs` and reuse the sanitized hive fixtures.

### Complete-key coverage

For `test_data/hive/testhive`:

- There are 527 descendant keys.
- Two keys contain 12 source values in total.
- The remaining 525 keys produce one placeholder each.
- The final value output contains exactly 537 rows.
- Every descendant key path appears at least once.
- The root node is absent.

### Row semantics

Tests will assert that:

- All 12 real rows use `is_placeholder: false`.
- All 525 placeholder rows use `is_placeholder: true`.
- An empty key has exactly one `Default`, empty-string, `REG_NONE`, size-zero placeholder.
- Real and placeholder rows carry the expected `mtime` and security descriptor fields.
- The output contains no top-level `values` field.
- A genuine unnamed default value remains distinguishable from a placeholder.
- Existing data types and `truncate_values_after` behavior remain unchanged.

### Best effort

The existing malformed value in `NTUSER.dat` will be used to verify that:

- The error is added to `RunReport`.
- The best available partial row is emitted with `is_placeholder: false` and `error`.
- Keys after the malformed value are still emitted.

Code paths for malformed child entries, key metadata, and unreadable subtrees will avoid `?` propagation at recoverable boundaries. Existing deterministic security descriptor unit tests continue to cover malformed descriptor parsing itself; no new binary fixture is required.

### Output formats

JSONL tests will verify the complete row shape and absence of `values: []`.

Regular CSV will use this exact schema order before its metadata column:

```text
path;mtime;security_descriptor;name;data;type;size;is_placeholder;invalid_signature;error;ogre_md
```

Normalized CSV will use the same mapped data columns followed by the framework-generated `ogre_id` data identifier and `ogre_md_id` metadata identifier:

```text
path;mtime;security_descriptor;name;data;type;size;is_placeholder;invalid_signature;error;ogre_id;ogre_md_id
```

Tests will assert that every header and row has the same number of columns and that placeholder cells remain aligned.

### Key parser compatibility

All `parse_hive_keys` fixture tests will switch to `hive_keys.xml`. Their existing record shape and expected counts remain unchanged.

## Compatibility

The public `parse_hive_values` signature is unchanged, but its record schema intentionally changes:

- Adds `mtime`, `security_descriptor`, and `is_placeholder`.
- Adds placeholder rows for empty keys, substantially increasing output counts.
- Removes the irrelevant `values` array when the dedicated mapping is used.
- Uses `security_descriptor` rather than the key parser's `descriptor` name.

Consumers that depend on the old flat value schema or row counts must update their mapping and expectations. `parse_hive_keys` remains compatible when used with its dedicated key mapping.

## Validation

Run targeted tests while developing, followed by repository-wide checks:

```bash
cargo test parser::hive
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

No Python unit tests will be added or run in this repository.
