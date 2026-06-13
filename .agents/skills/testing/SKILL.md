---
name: testing
description: Add, update, place, or review pvf tests and validation. Use when writing regression tests, deciding test layer, adding module contract tests, changing test policy, or choosing validation commands.
---

# Testing

Use this skill for test-related work in pvf.

## Start

Read the relevant section of `docs/testing.md` before non-trivial test changes,
moving tests, adding a new test layer, or changing test policy.
Read only the relevant section of `docs/reference.md` when the test protects a
stable contract.
Read only the relevant section of `docs/architecture.md` when the test depends
on subsystem boundaries, runtime flow, workers, or ownership.

## Change Triage

Classify the change first:

- Bug fix: add a regression test first, or record why that is not useful.
- Stable behavior change: update the relevant contract test and `docs/reference.md`.
- Architecture boundary change: update `docs/architecture.md` and protect the
  behavior that must not regress.
- Internal refactor: preserve existing contract tests; add characterization
  tests only where coverage is weak.
- Inventory change: update the owning Rust catalog or type, then guard
  meaningful consistency rather than duplicating the inventory in tests.

## Test Placement

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

Prefer tests that would fail for the intended bug or contract drift. Test names
should read like the behavior being protected, not like the function being
called.

Assert behavior through the boundary under test. Unit tests may inspect private
helpers, but module contract tests should avoid incidental internal state and
assert parser output, command outcome, palette effect, emitted event, cache
identity, accepted or rejected worker result, rendered row, notice, error, or
other observable results.

Keep setup small enough that the behavior under test is easy to see.

Use consistency tests for repo-owned registries and catalogs when drift is the
risk: command metadata, command parser and dispatch routing, built-in keymaps,
palette provider registration, extension host composition, config parsing, and
performance diagnostic report shape. Do not duplicate a full inventory unless
the assertion protects a meaningful cross-module invariant.

For async, worker, ordering, cancellation, search generation, render stale
results, and presenter encode results, avoid sleeps and real-time assumptions.
Prefer explicit identities, generations, queues, drain points, and deterministic
completion inputs.

If a proposed test mostly mirrors an implementation table or provides overview,
prefer a consistency invariant or a docs update instead.

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
