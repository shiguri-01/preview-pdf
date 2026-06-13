---
name: extension
description: Add, edit, rename, or review pvf extension behavior. Use when changing Extension implementations, ExtensionHost composition, extension-owned state, input hooks, AppEvent handling, background drain, status bar segments, extension UI snapshots, or reload/reset behavior.
---

# Extension

Use this skill for changes centered on pvf feature extensions and their runtime state.

## Start

Read `docs/reference.md` sections for Extensions before changing extension
contracts or host composition.
Read `docs/reference.md` sections for Palette when an extension exposes data to
a palette or opens a palette.
Read `docs/reference.md` sections for Rendering And Workers when extension work
can outlive one event-loop iteration.
Read `docs/architecture.md` when extension ownership, event flow, worker flow,
or subsystem boundaries change.
Read `docs/testing.md` before placing new extension tests.

## Implementation Checks

- Keep extension state owned by `ExtensionHost` unless the architecture changes deliberately.
- Preserve hook ordering and first-claim input behavior when adding input interception.
- Keep event observation driven by typed `AppEvent` values rather than ad hoc cross-module calls.
- Make background drain report whether visible or behavioral state changed.
- Expose palette-facing data through a small UI snapshot instead of leaking extension internals.
- Keep status-bar output optional and compact.
- If document reload affects the extension, define the reset, preserve, or rehydrate behavior explicitly.
- If the extension starts asynchronous work, define owner, shutdown path, cancellation model, stale-result identity, and event propagation.

## Tests And Docs

- Add or update state tests for event handling, input hooks, background drain, status output, and reload/reset behavior.
- Add host tests when composition, ordering, snapshot data, or cross-extension interactions change.
- Update `docs/reference.md` for extension contract, host composition, built-in
  extension behavior, worker, cancellation, or stale-result contract changes.
- Update `docs/architecture.md` only when ownership, event flow, worker flow, or
  boundary rationale changes.
