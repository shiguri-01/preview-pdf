---
name: pvf-command
description: Change or review pvf runtime command contracts, dispatch, or key bindings.
---

# PVF Command

For user-visible changes, establish the intended invocation, arguments,
feedback, and relevant failure behavior from the request and existing commands.
Clarify unresolved product choices when they matter; a separate design document
is not required.

## Contracts

- The command catalog owns typed commands, ids, metadata, parsing, and execution
  routing. Keep parser behavior, metadata arguments, and dispatch aligned.
- Role, exposure, invocation source policy, target resolution, and runtime
  `enabled_when` are separate concerns. Commands and key bindings share the
  runtime condition system.
- Keep public names and arguments stable during internal redesigns. Choose
  new interfaces for user understanding.
- Feature handlers own execution. Surface-local interactions use scoped key
  bindings or interaction command requests and resolve targets through dispatch.
  Normal, palette, and help keys share the input sequence registry.
- Handlers return `CommandExecution`: `Applied` or `Noop` plus
  `CommandEffects`. Dispatch applies effects after validation; handlers do not
  write pending palette queues, input history, or loop lifecycle state directly.
- Use effects for notices, app events, palette requests, history records,
  follow-up commands, and lifecycle requests. Follow-up commands carry an
  intentional invocation source and go through validation. Termination is a
  lifecycle effect, not an outcome.
- Keep navigation events and reasons aligned with the command's behavior.
  Update keymap and help together when changing key access.

## Relevant context and checks

Use Commands and Keymap in `docs/reference.md` for those contracts;
use its Palette section when changing listing, hints, completion, or submission.
Use `docs/architecture.md` for routing or ownership changes and
`docs/testing.md` for test-layer decisions.

Protect changed argument shapes, validation, and source restrictions with
parser or dispatch tests. Palette metadata and completion belong in provider
tests. Update docs when stable contracts or ownership change.
