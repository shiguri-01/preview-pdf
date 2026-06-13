---
name: testing
description: Add, update, place, or review pvf tests and validation. Use when writing regression tests, deciding test layer, adding module contract tests, changing test policy, or choosing validation commands.
---

# Testing

Use this skill for test-related work in pvf.

## Start

Read `docs/testing.md` before non-trivial test changes, moving tests, adding a
new test layer, or changing test policy.
Read `docs/reference.md` when the test protects a stable contract.
Read `docs/architecture.md` when the test depends on subsystem boundaries,
runtime flow, workers, or ownership.

## Workflow

Classify the change first:

- Bug fix: add a regression test first, or record why that is not useful.
- Stable behavior change: update the relevant contract test and `docs/reference.md`.
- Architecture boundary change: update `docs/architecture.md` and protect the
  behavior that must not regress.
- Internal refactor: preserve existing contract tests; add characterization
  tests only where coverage is weak.
- Inventory change: update the owning Rust catalog or type, then guard
  meaningful consistency rather than duplicating the inventory in tests.

Choose the narrowest useful test boundary:

- Use in-file `#[cfg(test)]` tests for private helpers, local state transitions,
  parser edge cases, matching, cache eviction, and layout calculations.
- Use `src/<module>/tests/` for public-facing module contracts that should
  survive internal refactors.
- Use repository-level `tests/` only for process-level CLI behavior, exit
  codes, config discovery, user-visible output, or headless runtime behavior.
- Keep performance diagnostics out of correctness tests except for JSON shape,
  scenario metadata, parsing, and validation rules.

## Quality Checks

Before accepting a test, check that it names one behavior, uses the narrowest
layer, asserts observable results, keeps setup small, and avoids real-time
flakiness for workers, ordering, cancellation, or background work.

Do not add broad tests that only mirror an implementation table. If a compact
overview is what the reader needs, update docs instead.

## Validation

During iteration, run the smallest useful targeted test.

Before finishing behavior changes, run:

```bash
cargo fmt
cargo test
cargo check
cargo clippy --all-targets --all-features -- -D warnings
```

For docs-only or skill-only changes, run checks that match the changed surface,
such as `git diff --check` and searches for stale paths. Record why broader
Cargo validation was not run.
