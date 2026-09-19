---
name: pvf-extension
description: Change or review pvf extension lifecycle, host composition, or state ownership.
---

# PVF Extension

- `ExtensionHost` owns extension state. Preserve hook ordering and first-claim
  input behavior when changing interception.
- Observe typed `AppEvent` values instead of ad hoc cross-module calls.
- Background drain reports whether visible or behavioral state changed.
- Expose palette-facing data through small UI snapshots; keep status output
  optional and compact.
- For document reload, define which state resets, persists, or is rehydrated.
- For asynchronous work, establish its owner, shutdown and cancellation paths,
  stale-result identity, and event propagation.

Use Extensions in `docs/reference.md` for lifecycle or host contracts, Palette
for palette integration, and Rendering And Workers for background work.
Consult `docs/architecture.md` for ownership or event-flow changes and
`docs/testing.md` for test-layer decisions.

Test changed state behavior beside the extension. Host tests cover composition,
ordering, snapshots, and cross-extension interactions. Update docs when stable
contracts or ownership change.
