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
backend or render types. Docs should name the policy, ownership boundary, and
code entry point, not copy every item.

Implementation detail belongs in code, focused tests, or local comments near
the implementation. If a doc section would become stale after an ordinary
implementation change, move the detail closer to the code or protect the
behavior with tests.

## Review Checklist

Does this change alter stable developer-facing behavior?

- Yes: add or update tests first; update `docs/reference.md` if the contract
  changes.
- No: no reference docs update is required.

Does this change affect subsystem boundaries or code-reading entry points?

- Yes: update `docs/architecture.md`.
- No: no architecture docs update is required.

Is this a bug fix?

- Yes: add a regression test first, or explain why that is not useful.

Is this only internal refactoring?

- Preserve contract tests. Add characterization tests only when important
  behavior has weak coverage.

Does this change a complete inventory?

- Update the owning code and consistency tests. Do not copy the inventory into
  docs.

Is the test focused on one behavior at the narrowest useful layer?

- If not, split it, move it, or simplify setup before relying on it.

Could this doc section become stale after an ordinary implementation change?

- If yes, move the detail to code, tests, or a local comment.
