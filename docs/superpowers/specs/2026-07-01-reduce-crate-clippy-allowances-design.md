# Reduce Crate Clippy Allowances Design

## Goal

Reduce crate-wide Clippy allowances in `src/lib.rs` by fixing mechanical warnings and localizing justified warnings. This pass covers both production allowances and the `cfg_attr(test, allow(...))` test-only allowance block.

## Context

`src/lib.rs` currently allows five Clippy lints crate-wide in production:

- `clippy::inherent_to_string_shadow_display`
- `clippy::large_enum_variant`
- `clippy::result_large_err`
- `clippy::should_implement_trait`
- `clippy::too_many_arguments`

It also allows several test-only lints crate-wide through `#![cfg_attr(test, allow(...))]`. Some nearby item-local allowances already exist in parser/formatter code for `too_many_arguments`, which suggests at least part of the cleanup can be localized without changing behavior.

## Design

### Scope

Use approach 2: reduce both production and test-only crate-level allowances.

The target outcome is fewer crate-wide allowances in `src/lib.rs`. Prefer removing crate-level allowances entirely when practical. If a lint is unavoidable because of PyO3 constructor signatures, public API compatibility, generated-style test fixtures, or intentionally large error/value shapes, move the allowance to the narrowest practical item and add a short rationale only when the reason is not obvious from the code.

### Workflow

1. On a non-`main` branch, temporarily remove the crate-wide production allowances from `src/lib.rs`.
2. Run `cargo clippy --all-targets --all-features -- -D warnings`.
3. Address each production warning:
   - Fix local mechanical warnings directly.
   - Localize justified allowances to specific functions, impl blocks, enums, or test modules.
   - Keep a crate-wide allowance only if warnings are widespread and intentionally tied to crate-wide public API shape.
4. Repeat for the test-only `cfg_attr(test, allow(...))` block.
5. Keep unrelated refactors out of scope.

### Decision Rules

- Fix: redundant clones, needless borrows, avoidable formatting, simple trait naming issues where the public API does not change.
- Localize: PyO3 constructors or parser functions with stable positional signatures, public enums whose size is constrained by Python/Rust API compatibility, tests that intentionally use explicit values for readability.
- Do not change: Python API behavior, parser output, record/value serialization, public constructor parameter order, or extension trait behavior.

## Validation

Required validation:

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`

Intermediate discovery commands should include strict Clippy runs after removing each crate-level allowance group, so every remaining allowance has a concrete reason.

## Non-Goals

- Do not aggressively redesign hot-path enums or error types in this pass.
- Do not remove PyO3 compatibility attributes just to satisfy Clippy.
- Do not add or run Python tests in this repository.
- Do not change unrelated docs such as `docs/to_do.md`.
