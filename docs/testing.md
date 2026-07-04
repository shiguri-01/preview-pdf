# Testing

Tests are executable specifications for the behavior they cover. They should
state the expected behavior, exercise it through the narrowest useful boundary,
and fail when that behavior or consistency drifts.

Use docs for orientation, rationale, ownership, and review context. Use tests
for observable behavior, compatibility rules, edge cases, cross-module
consistency, and user-facing outcomes. Use both when a topic needs a mental
model and executable detail.

## Test Layers

Use in-file `#[cfg(test)]` tests for behavior close to the implementation:
private helpers, parser edge cases, matching, cache eviction, layout
calculations, command argument parsing, and small state transitions. These tests
may inspect private APIs and may change during refactors.

Use `src/<module>/tests/` for stable module contracts that should survive
internal refactors. The behavior should be exercised through the subsystem
boundary, not through incidental private state.

Use repository-level `tests/` only for process-level behavior: CLI arguments,
exit codes, config discovery, user-visible output, or headless runtime
behavior.

Keep performance diagnostics separate from correctness tests. Normal tests may
cover JSON shape, scenario metadata, parsing, and validation rules; benches and
diagnostics cover timing, throughput, and regression observation.

## Change Policy

Bug fixes add a regression test first, or record why that is not useful.

Stable behavior changes update the relevant unit, module contract, or
integration test first. Update [reference.md](reference.md) when the
developer-facing contract changes.

Architecture boundary changes update [architecture.md](architecture.md) and add
or update tests for behavior that must not regress.

Internal refactors preserve existing contract tests. Add characterization tests
only when important behavior has weak coverage.

Inventory changes update the owning Rust catalog, registry, or type definition.
Use tests for meaningful consistency across owners; do not duplicate a full
inventory in tests unless the assertion protects a real contract.

## Test Quality

A good test would fail before the bug fix or behavior change. Test names should
read like the behavior being specified, not like the function being called.

Assert through the boundary under test. Unit tests may inspect private helpers,
but module contract tests should assert observable results such as parser
output, command outcome, palette effect, emitted event, cache identity,
accepted or rejected worker result, rendered row, notice, or error.

Keep setup small enough that the behavior under test is easy to see. For async,
worker, ordering, cancellation, search generation, render stale results, and
presenter encode results, avoid sleeps and real-time assumptions; prefer
explicit identities, generations, queues, drain points, and deterministic
completion inputs.

Do not add broad tests that only mirror an implementation table or provide an
overview. If there is a real cross-owner invariant, test that invariant; if the
value is orientation, document it instead.

## Validation Commands

Run the smallest useful targeted test during iteration. Before finishing a
behavior or docs migration change, run:

```bash
cargo fmt
cargo test
cargo check
cargo clippy --all-targets --all-features -- -D warnings
```

For docs-only changes, run checks that match the changed surface, such as
`git diff --check`. Record why broader validation was not run when it would
normally be expected.
