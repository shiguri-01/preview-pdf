# Command System Specification

This document is the source of truth for command concepts, command metadata,
parsing, invocation rules, dispatch, and current command coverage.

## Scope

This document owns:

- the typed `Command` model
- command ids and command metadata
- argument shapes and parsing rules
- invocation-source and availability rules
- dispatch outcomes and event emission
- the current command set

User-visible feature behavior remains in `runtime-spec.md`. Palette-specific
interaction stays in `palette-provider.md`.

## Runtime model

Commands are first-class runtime actions with three layers:

- typed command values in `Command`
- metadata in `CommandSpec`
- source-aware validation and dispatch

Command ids are stable kebab-case strings. Parsing turns command text into
typed commands, validation checks whether the source is allowed to invoke the
command right now, and dispatch executes the command and emits runtime events.

## Command metadata contract

Each command has:

- `id`
- `title`
- `args`
- `arg` UI hints
- `exposure`
- `invocation`
- `availability`

`CommandSpec` is the registry-backed metadata type for those fields.

Rules:

- command ids are the canonical string representation
- the registry is static
- typed commands are expected to have a matching registry entry
- command palette visibility is derived from metadata rather than hand-coded
  per-command UI rules
- command argument metadata may additionally describe enum-valued arguments for
  palette completion and assistive text

## Invocation sources and visibility

Invocation sources:

- `Keymap`
- `CommandPaletteInput`
- `PaletteProvider`

Invocation policies:

- `User`
  - invocable from user-facing sources
- `KeymapOnly`
  - invocable only from key bindings
- `InternalOnly`
  - invocable only from provider-driven internal flows

Exposure rules:

- `Public` commands may be listed in user-facing command surfaces
- `Internal` commands are runtime plumbing and are not listed in the command
  palette

Availability rules are checked separately from invocation policy. At present,
the current dynamic condition is `SearchActive`.

## Parsing contract

Command parsing is whitespace-delimited:

- the first token is the command id
- the remaining text is parsed according to that command's argument contract
- unknown ids are rejected before command-specific parsing runs

General rules:

- empty input is invalid
- commands with no arguments reject trailing arguments
- integer page arguments are 1-based in command text
- page arguments must be `>= 1`
- parser errors use command-specific invalid-argument messages

Command-specific parsing rules:

- `goto-page <page>`
  - requires exactly one integer argument
- `zoom <value>`
  - requires exactly one `f32` argument
- `pan <left|right|up|down> [amount]`
  - requires a direction and accepts an optional integer amount
  - missing amount becomes `DefaultStep`
  - overflow in `amount` clamps to `i32` bounds
- `page-layout-single`
  - takes no arguments
- `page-layout-spread [ltr|rtl]`
  - accepts at most one spread direction argument
- `open-palette <kind> [seed]`
  - parses palette kind first and preserves remaining text as optional seed
- `submit-search <query> [matcher]`
  - requires a non-empty query
  - if the last token matches a known matcher id, it is parsed as matcher
- `history-goto <page>`
  - requires exactly one integer page argument
- `outline-goto <page> <title>`
  - requires a 1-based page plus non-empty title text

## Dispatch contract

Dispatch returns `CommandDispatchResult`:

- `outcome: CommandOutcome`
- `emitted_events: Vec<AppEvent>`

Outcomes:

- `Applied`
- `Noop`
- `QuitRequested`

Dispatch rules:

- source-aware validation runs before execution
- rejected commands return `Noop` and still emit `AppEvent::CommandExecuted`
- successful commands may mutate app state, queue palette requests, call into
  extensions, or request quit
- notice application is part of dispatch
- `AppEvent::CommandExecuted` is always emitted after dispatch completes

Navigation-aware commands may additionally emit:

- `AppEvent::PageChanged`
- `AppEvent::ModeChanged`

Navigation reason derivation is tied to the dispatched command:

- page movement commands emit `Step` or `Goto(...)`
- search-hit navigation emits `Search { query }`
- history commands emit `History(...)`
- outline selection emits `Outline { title }`
- layout changes may emit `LayoutNormalize`

## Current command set

Public commands:

- Navigation
  - `next-page`
  - `prev-page`
  - `first-page`
  - `last-page`
  - `goto-page <page>`

- Zoom and viewport
  - `zoom <value>`
  - `zoom-in`
  - `zoom-out`
  - `zoom-reset`
  - `pan <direction> [amount]`

- Layout
  - `page-layout-single`
  - `page-layout-spread [ltr|rtl]`

- Debug status
  - `debug-status-show`
  - `debug-status-hide`
  - `debug-status-toggle`

- Help and search
  - `help`
  - `search`
  - `next-search-hit`
  - `prev-search-hit`
  - `cancel-search`

- History and outline
  - `history-back`
  - `history-forward`
  - `history`
  - `outline`

- Process
  - `quit`

Internal commands:

- `open-palette <kind> [seed]`
- `close-palette`
- `close-help`
- `submit-search <query> [matcher]`
- `history-goto <page>`
- `outline-goto <page> <title>`

Availability-gated public commands:

- `next-search-hit`
- `prev-search-hit`

These are available only while search is active.

## Code references

- `src/command/types.rs`
- `src/command/spec.rs`
- `src/command/parse.rs`
- `src/command/dispatch.rs`
- `src/command/core.rs`
