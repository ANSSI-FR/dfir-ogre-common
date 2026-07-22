# Oversized DWORD Raw Fallback Design

## Goal

Parse registry values declared as `REG_DWORD_LE` or `REG_DWORD_BE` without an error when their payload is larger than eight bytes. Preserve the declared registry type and emit the payload as a raw hexadecimal string.

## Scope

Apply the behavior consistently to both `dfir-ogre-common` hive interfaces:

- the flat hive parser in `src/parser/hive.rs`;
- the `RegValue` API in `src/registry_api.rs`.

Do not add SAM-specific path matching or interpret oversized data as alias membership. Do not change `dfir-nt-hive` or its scalar `dword_data` API.

## Behavior

For both DWORD endianness variants, inspect the declared payload size before calling `dword_data`:

- Sizes of 0, 1, 2, 4, or 8 bytes continue through the existing scalar conversion. Existing error behavior for unsupported sizes of 3, 5, 6, or 7 bytes is unchanged.
- Sizes greater than eight bytes use the existing raw-data extraction and hexadecimal serialization path.

The flat parser retains `type: "REG_DWORD_LE"` or `type: "REG_DWORD_BE"` and writes a string such as `0xe9030000ee030000ed030000ef030000` to `data`. Its existing `truncate_values_after` setting also applies to this raw hexadecimal fallback.

The `RegValue` API retains its DWORD `DataType`, leaves `int_data` unset, stores the bytes in `bin_data`, and serializes them through its existing hexadecimal representation.

Because the oversized branch does not call `dword_data`, it does not create an `InvalidWordSize` error. The flat parser therefore adds neither a value-level `error` field nor a run-report error, and the `RegValue` API leaves its `error` field unset.

## Testing

Add regression coverage for both interfaces using a sanitized hive value declared as a DWORD with a payload larger than eight bytes. Tests will assert:

- the declared DWORD type is preserved;
- the complete raw payload is emitted as hexadecimal;
- no value-level error is present;
- the flat parser reports no parsing error;
- ordinary DWORD values still serialize as integers.

Run focused parser and registry API tests, followed by formatting, the complete Rust test suite, and strict Clippy checks.
