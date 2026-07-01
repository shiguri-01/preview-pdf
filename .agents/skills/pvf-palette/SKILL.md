---
name: pvf-palette
description: Add, edit, rename, or review pvf palette behavior. Use when changing PaletteKind, PaletteProvider implementations, palette open options, provider snapshots, candidate rows, command-palette visibility, input modes, selection behavior, tab or submit behavior, palette-scoped key behavior, or palette rendering contracts.
---

# PVF Palette

Use this skill for pvf palette behavior and palette subsystem boundaries.

## First Decision

Before touching code, decide whether the task changes the user's interaction.

- If yes, write the user-facing interaction contract first. Do not start
  implementation until the user goal, operation, display, and feedback are clear
  enough to test.
- If no, keep the work internal: preserve behavior, avoid UX speculation, and
  use existing tests as the contract.

User-facing interaction contract means the concrete workflow: what the user is
trying to accomplish, the intuitive operation to get there, the shortest
keyboard path, what is shown or not shown, where it appears, title, initial
input, selected row, row text, assistive text, empty state, notices, Tab, Enter,
Escape, selection movement, text editing, history navigation, no matches,
unavailable state, repeated use, close, reopen, and recovery from mistakes.

## Context

- Palette contract: `docs/reference.md` Palette section.
- Ownership or snapshot boundary: relevant `docs/architecture.md` section.
- Command-palette dispatch or visibility: `docs/reference.md` Commands section.
- Test placement question: `docs/testing.md`.

For implementation, start from the owning code:

- Common active-session behavior: `src/palette/session_controller.rs`.
- Provider contract and context: `src/palette/provider.rs`.
- Candidate ids, text, row construction, and matching text:
  `src/palette/candidate.rs`, `src/palette/text.rs`, `src/palette/row.rs`.
- Registry wiring only: `src/palette/registry.rs`.
- Built-in providers: `src/palette/providers/command.rs`,
  `src/search/palette.rs`, `src/history/palette.rs`,
  `src/outline/palette.rs`.

## Rules

- Providers own candidate generation, input mode, completion, submit,
  assistive text, and provider-specific initial selection.
- `PaletteSessionController` owns open/close, session id validation, input
  editing, filtering, selection, completion routing, submit routing, and input
  history navigation.
- Open requests carry only common initialization (`PaletteOpenOptions`).
  Provider-owned UI data crosses through app/extension snapshots.
- Build candidates through `PaletteRow`; displayed text and match text should
  share the same formatting path.
- Use typed effects and commands. Providers must not mutate app state, key
  routing, follow-up queues, or input history directly.

## Tests And Docs

- Put provider-specific behavior tests next to that provider.
- Keep `session_controller` tests about provider-neutral session behavior.
- Use command-dispatch, input, or rendering tests only for those boundaries.
- Update docs only for stable contracts, ownership, or compatibility policy.
  Put executable behavior in tests and local details in code.
