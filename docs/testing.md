# Testing

Tests should protect behavior at the narrowest useful boundary. Prefer tests
over detailed docs when a stable behavior was removed from old documentation,
but do not turn every piece of orientation into a test. Use docs for the shape
of the system and tests for behavior or consistency that can regress.

## Test Layers

In-file unit tests live next to the implementation:

```text
src/<module>.rs #[cfg(test)] mod tests
```

Use them for parser edge cases, matcher scoring, cache eviction, layout
calculations, command argument parsing, small state transitions, and private
helpers. They may inspect private APIs and may change during refactors.

Module contract tests live under a module's `tests` directory:

```text
src/<module>/tests/
```

Use them for behavior that should survive internal refactors: command registry
invariants, public command metadata completeness, public/internal invocation
rules, keymap and command registry consistency, palette submit/tab/cancel and
selection behavior, extension host event propagation, render worker
stale-result handling, and observable app runtime behavior.

Integration tests live under repository-level `tests/` only when behavior needs
a process-level boundary: CLI arguments, exit codes, config file discovery,
user-visible error output, or headless runtime behavior.

Performance diagnostics are separate from correctness tests. Use tests for JSON
shape, diagnostic output parsing, and scenario metadata compatibility. Use
benches or diagnostics for timing, throughput, and regression observation.

## Docs Versus Tests

Use docs when the reader needs orientation, rationale, ownership, or a compact
map of related concepts. Use tests when the project needs executable
protection: parser behavior, compatibility rules, cross-module consistency,
stale-result handling, and observable user-facing outcomes.

Use both when a topic needs a mental model and a guardrail. For example,
[reference.md](reference.md) explains that key bindings are owned by the input registry and
must resolve to invocable commands; command tests verify that registry
consistency.

Avoid both extremes:

- Do not keep a complete hand-written inventory in docs when ordinary code
  changes can make it stale.
- Do not add broad tests that only duplicate an implementation table without
  protecting a meaningful contract.

## Placement Rules

- Keep tests that depend on private helpers, private data layout, or local
  implementation branches as in-file unit tests.
- Use `src/<module>/tests/` only when the behavior can be exercised through the
  subsystem boundary: public module exports, crate-visible facade APIs, or
  explicit test-support fixtures.
- Do not move a test to a module contract directory only because the behavior
  is important.
- Add or update module contract tests when stable behavior in [reference.md](reference.md)
  changes.
- Keep process-level integration tests minimal because they are slower and more
  brittle than unit or module contract tests.

Existing module contract examples:

- [src/app/tests/](../src/app/tests/)
- [src/presenter/tests/](../src/presenter/tests/)

Good future locations include:

- [src/command/tests/](../src/command/tests/)
- `src/palette/tests/`
- `src/extension/tests/`
- `src/render/tests/`

Before adding one, check whether the same behavior can be tested cleanly
through the subsystem boundary. If not, add an in-file unit test near the code.

## Test-First Bias

Prefer writing or updating the test before changing behavior.

Use test-first work for bug fixes, command parsing or dispatch behavior, key
binding resolution, palette interaction semantics, extension event propagation,
render stale-result handling, config compatibility, and CLI-visible behavior.

For pure refactors, first check whether existing tests already cover the
behavior. Add characterization tests only when important behavior has weak
coverage.

Do not force test-first work for mechanical edits, local cleanup, renames,
comments, docs-only changes, clippy fixes, or refactors already covered by
existing tests.

## Change Policy

Bug fix:

1. Add a regression test first, or explain why not.
2. Fix the bug.
3. Keep or improve the test.

Stable behavior change:

1. Add or update the relevant unit, module contract, or integration test first.
2. Update [reference.md](reference.md) if the documented contract changes.
3. Implement the change.

Architecture boundary change:

1. Update [architecture.md](architecture.md).
2. Add or update tests for behavior that must not regress.
3. Keep local implementation details out of docs.

Internal refactor:

1. Preserve existing contract tests.
2. Add characterization tests only when important behavior has weak coverage.
3. Usually do not update docs.

Inventory change:

1. Update the owning Rust catalog, registry, or type definition.
2. Update tests that guard meaningful cross-module consistency.
3. Update docs only when the inventory change alters a category, ownership
   boundary, compatibility policy, or reader-facing orientation.

## Test Quality Checklist

- Does the test describe one behavior?
- Does the test name explain the expected behavior?
- Would the test fail before the bug fix or behavior change?
- Is this the narrowest test layer that can protect the behavior?
- Does the assertion check an observable result rather than incidental state?
- Is setup small enough that the behavior under test is easy to see?
- If this is a module contract test, does it use the subsystem boundary?
- If the test involves workers, time, ordering, or cancellation, does it avoid
  flaky real-time assumptions?

Treat test code as maintained code. Prefer small fixtures and clear helpers
over large all-knowing setup.

## Validation Commands

Run these before finishing a behavior or docs migration change:

```bash
cargo fmt
cargo test
cargo check
cargo clippy --all-targets --all-features -- -D warnings
```

For fast iteration, `cargo check` is useful before the full validation set.
Record the reason if a required validation command cannot be run.
