---
name: command
description: Add, edit, rename, or review pvf command behavior. Use when changing command ids, arguments, parsing, invocation policy, command palette visibility, key bindings, help text, or command dispatch.
---

# Command

Use this skill for changes centered on pvf runtime commands.

## Start

Read only the relevant `docs/reference.md` sections for Commands and Key
Bindings before changing command internals, command ids, invocation policy, key
bindings, or help surfaces.
Read only the relevant `docs/reference.md` Palette section when command palette
behavior, completion, visibility, or submission changes.
Read the relevant part of `docs/architecture.md` when command routing or
subsystem boundaries change.
Read the relevant section of `docs/testing.md` before placing new command or
keymap tests.

## User-Facing Design First

Before editing implementation code for a user-visible command, write a short working contract:

- What is the user trying to accomplish?
- What would the user intuitively try first: a key, command palette search, typed command, or existing workflow?
- What exact command id, arguments, key binding, title, help text, notice, or error will the user see?
- What is the shortest successful path?
- What happens for empty input, invalid input, unavailable state, repeated use, cancellation, and recovery?
- Which existing command should this feel consistent with?

Do not start implementation until the external behavior is clear enough to test.

## Implementation Checks

- Treat the command catalog as the source for the typed command, command id, metadata, parser routing, and execution routing.
- Keep parser behavior, metadata args, and dispatch behavior aligned.
- Choose exposure, invocation policy, and availability deliberately.
- Choose public command names and arguments for user understanding, not internal convenience.
- Put command execution in the feature handler that owns the behavior.
- If a command affects navigation, ensure emitted app events and navigation reasons still match the feature behavior.
- If a public command should be reachable by a key, update the keymap and help surface together.
- If command palette behavior changes, verify listing, argument hints, completion, and runtime gating.

## Tests And Docs

- Add or update focused parser and dispatch tests for new argument shapes, validation, and source restrictions.
- Add palette-provider tests when metadata, hints, visibility, or completion behavior changes.
- Update `docs/reference.md` when command, key binding, palette visibility, or
  user-visible runtime contracts change.
- Update `docs/architecture.md` only when command routing or ownership
  boundaries change.
