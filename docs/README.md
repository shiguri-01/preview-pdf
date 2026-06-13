# Developer Docs

This directory contains the durable developer-facing docs for `pvf`.

## Reading Path

1. `architecture.md` for the system map, runtime flow, subsystem boundaries,
   and code-reading entry points.
2. `reference.md` for stable contracts that implementation and review should
   protect.
3. `testing.md` for test placement, test-first guidance, quality checklists,
   and validation commands.

## Where Material Belongs

- Put architecture material in `architecture.md` when a change affects runtime
  flow, subsystem boundaries, ownership, event routing, or code-reading entry
  points.
- Put stable developer-facing behavior in `reference.md` when a change
  intentionally changes CLI behavior, config compatibility, command policy,
  key binding resolution, palette semantics, extension host behavior, render
  stale-result behavior, cache behavior visible outside the implementation, or
  performance diagnostics contracts.
- Put testing policy in `testing.md` when a change affects how behavior should
  be protected.

Complete inventories belong in the Rust code that defines them: command
catalogs, keymaps, palette registries, extension hosts, config types, and
backend or render types. Docs may include a short orientation map or a few
representative examples when that helps readers understand the whole system,
but they should not become the source of truth for every item.

Implementation detail belongs in code, focused tests, or local comments near
the implementation. If a doc section would become stale after an ordinary
implementation change, move the detail closer to the code or protect the
behavior with tests.

Use judgment before replacing docs with tests. Tests are good at preventing
drift in stable behavior and cross-module consistency. Docs are better for
explaining the shape of the system, why a boundary exists, and where to start
reading. Prefer both when a topic needs orientation and correctness protection.

## Change Triage

Use this as the first pass before editing docs or tests:

- Stable behavior changed: update focused tests first and update
  `reference.md` if the contract changes.
- Subsystem boundary changed: update `architecture.md`.
- Bug fixed: add a regression test first, or record why that is not useful.
- Inventory changed: update owning code and meaningful consistency tests; keep
  docs to orientation and compatibility notes.
- Internal refactor only: preserve contract tests; add characterization tests
  only when coverage is weak.
- Doc detail would stale quickly: move it to code, tests, or a local comment.
