# Reduce Crate Clippy Allowances Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce the crate-wide Clippy allowances in `src/lib.rs` by fixing mechanical warnings and moving justified exceptions to the narrowest practical item.

**Architecture:** Treat strict Clippy diagnostics as the regression test for this refactor. Remove crate-level allowances incrementally, run Clippy to expose the exact failures, then either fix the underlying code or localize the allowance when the lint is caused by PyO3 binding shape, public API compatibility, or intentionally large enum/error types.

**Tech Stack:** Rust 2024, Cargo, Clippy, PyO3, existing inline Rust unit tests.

---

### File Structure

- Modify: `src/lib.rs`
  - Remove or shrink the production crate-level `#![allow(...)]`.
  - Remove or shrink the `#![cfg_attr(test, allow(...))]` test-only block after diagnostics are handled.
- Modify: `src/date_util.rs`
  - Replace the inherent `DateInputCodec::from_str` constructor with trait-friendly conversion while preserving existing parsing behavior.
- Modify: call sites reported by `rg "DateInputCodec::from_str" src -g'*.rs'`
  - Update internal Rust code and inline tests to the replacement conversion API.
- Modify: files reported by strict Clippy diagnostics
  - Fix mechanical test lints where the code is simple and local.
  - Localize unavoidable lint exceptions with `#[allow(...)]` on the specific enum, function, impl block, or test module.
- Do not modify: `docs/to_do.md`
- Do not add or run Python tests in this repository.

### Task 1: Baseline And Production RED Check

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Confirm branch and clean tree**

Run:

```bash
git branch --show-current
git status --short
```

Expected:

```text
reduce-crate-clippy-allowances-design
```

`git status --short` should not show source-code changes before the RED edit.

- [ ] **Step 2: Remove the production crate-level allowance block**

Edit the top of `src/lib.rs` from:

```rust
#![allow(
    clippy::inherent_to_string_shadow_display,
    clippy::large_enum_variant,
    clippy::result_large_err,
    clippy::should_implement_trait,
    clippy::too_many_arguments
)]
#![cfg_attr(
    test,
    allow(
        clippy::approx_constant,
        clippy::bool_assert_comparison,
        clippy::clone_on_copy,
        clippy::field_reassign_with_default,
        clippy::for_kv_map,
        clippy::legacy_numeric_constants,
        clippy::needless_borrows_for_generic_args,
        clippy::redundant_pattern_matching,
        clippy::single_char_add_str,
        clippy::unnecessary_to_owned,
        clippy::useless_format
    )
)]
```

to:

```rust
#![cfg_attr(
    test,
    allow(
        clippy::approx_constant,
        clippy::bool_assert_comparison,
        clippy::clone_on_copy,
        clippy::field_reassign_with_default,
        clippy::for_kv_map,
        clippy::legacy_numeric_constants,
        clippy::needless_borrows_for_generic_args,
        clippy::redundant_pattern_matching,
        clippy::single_char_add_str,
        clippy::unnecessary_to_owned,
        clippy::useless_format
    )
)]
```

- [ ] **Step 3: Run strict Clippy and verify RED**

Run:

```bash
cargo clippy --all-targets --all-features --message-format short -- -D warnings
```

Expected: FAIL with Clippy diagnostics for at least one of these lints:

```text
clippy::inherent_to_string_shadow_display
clippy::large_enum_variant
clippy::result_large_err
clippy::should_implement_trait
clippy::too_many_arguments
```

The failure is the RED check for the production allowance cleanup.

### Task 2: Fix The Trait-Naming Warning

**Files:**
- Modify: `src/date_util.rs`
- Modify: `src/configuration.rs`
- Modify: `src/line_builder.rs`
- Modify: inline tests in `src/date_util.rs`

- [ ] **Step 1: Locate current call sites**

Run:

```bash
rg "DateInputCodec::from_str" src -g'*.rs'
```

Expected current sites include:

```text
src/configuration.rs
src/date_util.rs
src/line_builder.rs
```

- [ ] **Step 2: Replace the inherent constructor**

In `src/date_util.rs`, replace:

```rust
impl DateInputCodec {
    pub fn from_str(name: &str) -> Self {
        match name {
            "iso" => DateInputCodec::Iso(),
            "file_time" => DateInputCodec::FileTime(),
            "timestamp" => DateInputCodec::Timestamp(),
            "timestamp_hex" => DateInputCodec::TimestampHex(),
            "timestamp_ms" => DateInputCodec::TimestampMs(),
            "timestamp_ns" => DateInputCodec::TimestampNs(),
            _ => DateInputCodec::Pattern(name.to_owned()),
        }
    }
}
```

with:

```rust
impl From<&str> for DateInputCodec {
    fn from(name: &str) -> Self {
        match name {
            "iso" => DateInputCodec::Iso(),
            "file_time" => DateInputCodec::FileTime(),
            "timestamp" => DateInputCodec::Timestamp(),
            "timestamp_hex" => DateInputCodec::TimestampHex(),
            "timestamp_ms" => DateInputCodec::TimestampMs(),
            "timestamp_ns" => DateInputCodec::TimestampNs(),
            _ => DateInputCodec::Pattern(name.to_owned()),
        }
    }
}
```

- [ ] **Step 3: Update call sites**

Use these concrete replacements:

```rust
DateInputCodec::from_str(&value)
```

becomes:

```rust
DateInputCodec::from(value.as_str())
```

```rust
DateInputCodec::from_str(date_codec)
```

becomes:

```rust
DateInputCodec::from(date_codec)
```

```rust
DateInputCodec::from_str("iso")
```

becomes:

```rust
DateInputCodec::from("iso")
```

Apply the same literal-string replacement to all inline tests in `src/date_util.rs` and `src/line_builder.rs`.

- [ ] **Step 4: Verify trait-naming GREEN**

Run:

```bash
cargo clippy --all-targets --all-features --message-format short -- -D warnings
```

Expected: no remaining `clippy::should_implement_trait` diagnostic for `DateInputCodec`.

### Task 3: Localize PyO3 And Public-Shape Production Exceptions

**Files:**
- Modify: files reported by Task 1/2 Clippy output.

- [ ] **Step 1: For each `clippy::inherent_to_string_shadow_display` diagnostic, localize**

For `Record::to_string` in `src/record.rs`, keep the Python-visible method name and add the narrow allowance directly on the method:

```rust
    #[allow(clippy::inherent_to_string_shadow_display)]
    pub fn to_string(&self) -> String {
```

Do not rename the Python method in this pass.

- [ ] **Step 2: For each `clippy::too_many_arguments` diagnostic, decide fix or localize**

If the diagnostic is on a `#[new]` PyO3 constructor or a parser helper whose parameter list mirrors parser state, keep the signature stable and put the existing local pattern on that exact item:

```rust
#[allow(clippy::too_many_arguments)]
```

If the diagnostic is from PyO3 `#[pyclass(from_py_object)]` generated conversion code, put the allowance on the affected public enum or struct declaration rather than restoring the crate-level allowance:

```rust
#[allow(clippy::too_many_arguments)]
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub enum Parser {
```

Use this generated-code localization only for the exact items reported by Clippy.

- [ ] **Step 3: For each `clippy::large_enum_variant` diagnostic, avoid hot-path redesign**

If the diagnostic is on public PyO3-facing enums such as `Value`, `Parser`, `Field`, `ParserType`, or the crate `Error`, localize the allowance to the enum:

```rust
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
#[pyclass(from_py_object)]
pub enum Value {
```

Do not box variants or change enum layout in this pass unless Clippy reports a small, private enum where boxing is local and behavior-neutral.

- [ ] **Step 4: For each `clippy::result_large_err` diagnostic, prefer enum-level root cause**

If diagnostics are caused by the crate `Error` enum size, first localize `clippy::large_enum_variant` to `src/errors.rs`:

```rust
#[allow(clippy::large_enum_variant)]
#[derive(Error, Debug)]
pub enum Error {
```

If `result_large_err` remains widespread after that, keep a crate-level `clippy::result_large_err` allowance in `src/lib.rs` with only that lint and no unrelated production lints:

```rust
#![allow(clippy::result_large_err)]
```

Only keep this crate-level exception when item-level localization would require annotating broad parser/output APIs across the crate.

- [ ] **Step 5: Verify production GREEN**

Run:

```bash
cargo clippy --all-targets --all-features --message-format short -- -D warnings
```

Expected: PASS or only test-only lint failures currently hidden by the `cfg_attr(test, allow(...))` block.

### Task 4: Test-Only RED Check

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Remove the test-only crate-level allowance block**

Edit the top of `src/lib.rs` so no `#![cfg_attr(test, allow(...))]` block remains.

- [ ] **Step 2: Run strict Clippy and verify RED**

Run:

```bash
cargo clippy --all-targets --all-features --message-format short -- -D warnings
```

Expected: FAIL if any test-only lint is still present. The diagnostic lint names should come from this removed list:

```text
clippy::approx_constant
clippy::bool_assert_comparison
clippy::clone_on_copy
clippy::field_reassign_with_default
clippy::for_kv_map
clippy::legacy_numeric_constants
clippy::needless_borrows_for_generic_args
clippy::redundant_pattern_matching
clippy::single_char_add_str
clippy::unnecessary_to_owned
clippy::useless_format
```

If strict Clippy passes, keep the block removed.

### Task 5: Fix Or Localize Test-Only Diagnostics

**Files:**
- Modify: inline `#[cfg(test)]` modules reported by Clippy.

- [ ] **Step 1: Apply mechanical test fixes**

Use these direct replacements when Clippy reports them:

```rust
assert_eq!(value, true);
```

becomes:

```rust
assert!(value);
```

```rust
assert_eq!(value, false);
```

becomes:

```rust
assert!(!value);
```

```rust
let mut config = Type::default();
config.field = value;
```

becomes a struct literal when all fields are local and readable:

```rust
let config = Type {
    field: value,
    ..Type::default()
};
```

```rust
for (key, _) in &map {
```

becomes:

```rust
for key in map.keys() {
```

```rust
buffer.push_str("\n");
```

becomes:

```rust
buffer.push('\n');
```

- [ ] **Step 2: Localize readability-driven test exceptions**

If a diagnostic would make a fixture-style test less clear, add the allowance to that exact test function or local `mod tests` block:

```rust
#[allow(clippy::field_reassign_with_default)]
#[test]
fn test_name() {
```

Use local test allowances only after checking the mechanical replacement would obscure the fixture setup.

- [ ] **Step 3: Verify test GREEN**

Run:

```bash
cargo clippy --all-targets --all-features --message-format short -- -D warnings
```

Expected: PASS with no crate-wide test-only allowance block in `src/lib.rs`, unless the final diff documents a deliberately retained narrow crate-level exception.

### Task 6: Full Verification And Commit

**Files:**
- Modify: all changed Rust files.

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

If it fails, run:

```bash
cargo fmt
cargo fmt --check
```

Expected after formatting: PASS.

- [ ] **Step 2: Unit tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 3: Strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Confirm `src/lib.rs` has fewer crate-level allowances**

Run:

```bash
sed -n '1,40p' src/lib.rs
```

Expected: the original five-lint production block is gone or reduced to a documented minimal exception, and the test-only block is gone or reduced.

- [ ] **Step 5: Confirm `docs/to_do.md` is untouched**

Run:

```bash
git diff -- docs/to_do.md
```

Expected: no output.

- [ ] **Step 6: Commit implementation**

Run:

```bash
git status --short
git add src/lib.rs src/date_util.rs src/configuration.rs src/line_builder.rs
git add <any additional Rust files changed by localized Clippy fixes>
git commit -m "Reduce crate-wide Clippy allowances"
```

Expected: commit succeeds. Do not push.

### Self-Review Checklist

- [ ] The plan removes or shrinks both production and test-only crate-level allowances.
- [ ] The plan preserves PyO3 method names and constructor signatures.
- [ ] The plan avoids hot-path enum/error redesign in this pass.
- [ ] The plan uses strict Clippy as the RED/GREEN verification for lint cleanup.
- [ ] The plan includes full final verification with `cargo fmt --check`, `cargo test`, and strict Clippy.
- [ ] The plan does not touch `docs/to_do.md`.
