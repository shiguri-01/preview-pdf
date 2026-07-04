# Developer Docs

This directory contains durable developer-facing docs for `pvf`. The docs do
not replace code or tests. They explain project-level decisions, contracts, and
testing policy whose ownership belongs outside individual implementation files.

## Code, Tests, And Docs

Code owns the implementation and complete inventories: command catalogs,
keymaps, palette registries, extension composition, config fields, backend
behavior, render cache rules, and similar lists.

Tests are the detailed executable specification for the behavior they cover.
They should describe observable behavior, compatibility rules, edge cases, and
cross-module consistency at the narrowest useful boundary.

Docs explain the context that tests and code do not express well: intent,
ownership, boundaries, compatibility concerns, and review criteria. Keep docs
at the level that should remain useful across routine implementation changes.
Move volatile detail closer to the code, or specify behavior with a focused
test when executable detail matters.

## Change Ownership

Changes should update the artifact that owns the affected information:

- Architecture changes update [architecture.md](architecture.md) when runtime
  shape, ownership, subsystem boundaries, or event routing changes.
- Stable behavior changes update focused tests first and update
  [reference.md](reference.md) when the developer-facing contract changes.
- Test policy changes update [testing.md](testing.md) when the expected
  specification, placement, protection, or validation path changes.
- Bug fixes add a regression test first, or record why that is not useful.
- Inventory changes update the owning code and consistency tests; do not copy
  the full inventory into docs.
- Internal refactors preserve contract tests; add characterization tests
  only when coverage is weak.
