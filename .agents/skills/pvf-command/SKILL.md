---
name: pvf-command
description: Add, edit, rename, or review pvf command behavior. Use when changing command ids, arguments, parsing, roles, exposure, invocation policy, target requirements, enabled_when runtime conditions, command palette visibility, key bindings, help text, or command dispatch.
---

# PVF Command

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
- What would the user intuitively try first: a key, command palette search,
  typed command, focused interaction, or existing workflow?
- What exact command id, arguments, key binding, title, help text, notice, or error will the user see?
- What is the shortest successful path?
- What happens for empty input, invalid input, unavailable state, repeated use, cancellation, and recovery?
- Which existing command should this feel consistent with?

Do not start implementation until the external behavior is clear enough to test.

## Implementation Checks

- Treat the command catalog as the source for the typed command, command id,
  role, exposure, invocation policy, target requirement, `enabled_when`,
  metadata, parser routing, and execution routing.
- Keep parser behavior, metadata args, and dispatch behavior aligned.
- Choose role, exposure, invocation policy, target requirement, and
  `enabled_when` deliberately. Visibility, source policy, target resolution,
  and runtime enabled-state are separate concerns.
- Use the shared runtime condition system for command `enabled_when` and key
  binding `enabled_when`; do not reintroduce command-only or keymap-only
  condition enums.
- Choose public command names and arguments for user understanding, not internal convenience.
- Preserve existing user-facing command names and argument contracts unless an
  explicit migration is being designed. Do not add fallback aliases or parallel
  old/new dispatch paths for internal redesign work.
- Put command execution in the feature handler that owns the behavior.
- Focused operations such as palette submit, palette selection, text editing,
  text history recall, help close, and help scroll should be represented as
  scoped key bindings or interaction command requests, then resolve their active
  target through dispatch.
- Keep key binding resolution in the input sequence registry. Normal, palette,
  and help keys should share the scoped key binding path instead of adding
  surface-local key matching.
- When a command completes another command, return a follow-up command request
  with an intentional invocation source instead of directly bypassing command
  validation.
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
