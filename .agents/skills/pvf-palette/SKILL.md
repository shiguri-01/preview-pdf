---
name: pvf-palette
description: Change or review pvf palette providers, interactions, or session behavior.
---

# PVF Palette

For interaction changes, establish the intended keyboard path, displayed
content, selection, and feedback, including relevant empty or unavailable states.
Use the request and existing behavior to resolve these details; internal
refactors preserve the existing interaction contract.

## Ownership

- Providers own candidates, input mode, completion, submit, assistive text, and
  provider-specific initial selection.
- `PaletteSessionController` owns session lifecycle and id validation, editing,
  filtering, selection, completion and submit routing, and history navigation.
- Open requests carry common initialization through `PaletteOpenOptions`.
  Provider-owned UI data crosses app/extension snapshots.
- Build candidates through `PaletteRow` so display and match text share a
  formatting path.
- Providers return typed effects and commands; they do not mutate app state,
  key routing, follow-up queues, or input history directly.

## Relevant context and checks

Use Palette in `docs/reference.md` for palette contracts and Commands for
command visibility or dispatch. Consult `docs/architecture.md` for snapshot
or ownership changes and `docs/testing.md` for test-layer decisions.

Start from `src/palette/session_controller.rs` for common session behavior,
`src/palette/provider.rs` for provider contracts, and candidate/text/row modules
for row construction. `src/palette/registry.rs` owns wiring only.
Built-in providers live in `src/palette/providers/command.rs`,
`src/search/palette.rs`, `src/history/palette.rs`, and `src/outline/palette.rs`.

Keep provider tests beside the provider and session tests provider-neutral.
Use dispatch, input, or rendering tests for behavior crossing those boundaries.
Update docs for stable contracts or ownership changes.
