---
name: testing
description: Add, update, place, or review pvf tests and validation. Use when writing regression tests, protecting behavior changes, choosing unit/module/integration test boundaries, reviewing test quality, changing test policy, or deciding validation commands.
---

# Testing

Tests are executable specifications for the behavior they cover. Code owns
implementation detail; docs own durable orientation and policy. Write tests to
protect observable behavior, compatibility rules, edge cases, and meaningful
cross-owner consistency.

## Before editing

Read the code boundary under test first. Read `docs/testing.md` when adding or
moving tests, changing validation policy, or choosing a test layer. For a tiny
local edit that follows an obvious existing pattern, the nearby tests may be
enough.

Use `docs/reference.md` and `docs/architecture.md` selectively. Check the
relevant section when the code or task suggests a stable developer-facing
contract, runtime ownership boundary, worker interaction, or subsystem
boundary may be involved. Otherwise, rely on the local code and existing tests.

## Change Triage

Before writing tests, classify what must be protected: a regression, a stable
contract, an architecture boundary, refactor characterization, or cross-owner
consistency. Use `docs/testing.md` for project policy when that choice affects
placement, validation, or documentation updates.

## Quality Checks

Prefer tests that would fail for the intended bug or contract drift. Name the
specified behavior, not the function being called.

When a test is the detailed specification, make the asserted rule visible in
the test body: cover the important success path, the boundary or rejection case,
and the observable effect that a maintainer must preserve. Prefer one focused
scenario with clear inputs and assertions over a broad case that only proves the
code was exercised.

Assert behavior through the boundary under test. Unit tests may inspect private
helpers, but module contract tests should avoid incidental internal state and
assert parser output, command outcome, palette effect, emitted event, cache
identity, accepted or rejected worker result, rendered row, notice, error, or
other observable results.

Keep setup small enough that the behavior under test is easy to see.

For async, worker, ordering, cancellation, search generation, render stale
results, and presenter encode results, avoid sleeps and real-time assumptions.
Prefer explicit identities, generations, queues, drain points, and deterministic
completion inputs.

Do not let task history or obsolete implementation context shape the test. The
test should specify the current behavior directly; it should not encode "new
versus old" reasoning unless compatibility across versions is the behavior.

Use consistency tests for repo-owned registries and catalogs when drift is the
risk: command metadata, parser and dispatch routing, keymaps, palette provider
registration, extension host composition, config parsing, and diagnostic report
shape. Do not mirror a full inventory unless the assertion protects a real
cross-owner invariant. If the value is orientation, update docs instead.

## Validation

Run the smallest useful targeted check during iteration. Before finishing, use
the validation path from `docs/testing.md` or the repo instructions that matches
the changed surface, and record when broader validation is intentionally skipped.
`cargo test` accepts one test-name filter; multiple test-name filters in one
command fail as unexpected arguments.
