---
name: pvf-palette
description: Add, edit, rename, or review pvf palette behavior. Use when changing PaletteKind, PaletteProvider implementations, palette payloads, palette candidate rows, input modes, selection behavior, tab or submit behavior, or palette rendering contracts.
---

# PVF Palette

Use this skill for changes centered on pvf palette UI and interaction behavior.

## Start

Read only the relevant `docs/reference.md` Palette section before changing
palette contracts or built-in palette behavior.
Read only the relevant `docs/reference.md` Commands section when palette
submission dispatches commands or changes command palette behavior.
Read only the relevant `docs/reference.md` Extensions section when palette data
comes from extension-owned state.
Read the relevant part of `docs/architecture.md` when palette ownership,
snapshots, or subsystem boundaries change.
Read the relevant section of `docs/testing.md` before placing new palette
tests.

## User-Facing Design First

Before editing implementation code, write a short working contract:

- What is the user trying to accomplish?
- What would the user intuitively do to open this palette, search within it, choose an item, and recover from a mistake?
- What exact title, input seed, selected row, row text, assistive text, notice, or empty-state text will the user see?
- What is the shortest successful keyboard path?
- What happens for empty input, no matches, unavailable state, repeated use, tab, enter, escape, and history navigation?
- Which existing palette should this feel consistent with?

Do not start implementation until the interaction is clear enough to test.

## Implementation Checks

- Keep palette kind parsing, open payloads, registry resolution, and provider behavior aligned.
- Choose the input mode based on who owns input meaning: generic filtering, free text, or custom provider logic.
- Keep rendered row text separate from searchable text.
- Make row labels and assistive text scannable for users; avoid exposing internal ids unless the user intentionally types them.
- Preserve session and selection behavior when changing open, reopen, tab, submit, or input-change flows.
- If a palette needs app data, add read-only snapshot data instead of passing mutable app state into a provider.
- If a palette reads extension data, expose only the needed UI snapshot fields.
- If submit dispatches a command, ensure the command source, invocation policy, and history recording are intentional.

## Tests And Docs

- Add or update provider tests for candidates, matching, selected item, assistive text, initial input, tab, and submit behavior.
- Add manager or rendering tests when palette session, input history, cursor, or row layout behavior changes.
- Update `docs/reference.md` for palette contract or built-in palette behavior
  changes.
- Update command or extension sections only when those contracts change too.
- Update `docs/architecture.md` only when ownership or boundary rationale
  changes.
