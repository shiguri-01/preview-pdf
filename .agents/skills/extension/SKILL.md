---
name: extension
description: Add, edit, rename, or review pvf extension behavior. Use when changing Extension implementations, ExtensionHost composition, extension-owned state, input hooks, AppEvent handling, background drain, status bar segments, extension UI snapshots, or reload/reset behavior.
---

# Extension

Use this skill for changes centered on pvf feature extensions and their runtime state.

## Start

Read `docs/extension-system.md` before changing extension contracts or host composition.
Read `docs/palette-provider.md` when an extension exposes data to a palette or opens a palette.
Read `docs/async-workers.md` when extension work can outlive one event-loop iteration.

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
- Update `docs/extension-system.md` for extension contract, host composition, or built-in extension behavior changes.
- Update `docs/async-workers.md` when spawn points, owners, shutdown, cancellation, or stale-result handling changes.
