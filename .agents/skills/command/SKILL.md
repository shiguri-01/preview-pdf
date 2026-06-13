---
name: command
description: Add, edit, rename, or review pvf command behavior. Use when changing command ids, arguments, parsing, invocation policy, command palette visibility, key bindings, help text, or command dispatch.
---

# Command

Use this skill for changes centered on pvf runtime commands.

## Start

Read `docs/command-system.md` before changing command internals.
Read `docs/runtime-spec.md` when the command is user-visible.
Read `docs/palette-provider.md` when command palette behavior, completion, or visibility changes.

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
- Update `docs/command-system.md` for command model or command-set changes.
- Update `docs/runtime-spec.md` only for user-visible behavior, CLI, key binding, or runtime contract changes.
