# Palette Provider Specification

This document defines the palette provider contract in `pvf`.

## Runtime model

- Palette instances are identified by `PaletteKind`.
- Provider resolution is static via `PaletteRegistry`.
- A palette session has a session id; transitions validate active session id.

## Provider interface

`PaletteProvider` exposes:

- `kind() -> PaletteKind`
- `title(ctx) -> String`
- `input_mode() -> PaletteInputMode`
- `list(ctx) -> Vec<PaletteCandidate>`
- `on_tab(ctx, selected) -> PaletteTabEffect` (default: `Noop`)
- `on_submit(ctx, selected) -> PaletteSubmitEffect`
- `assistive_text(ctx, selected) -> Option<String>` (default: `None`)
- `initial_input(seed) -> String` (default: `seed` passthrough)

`PaletteContext` contains:
- `ctx.app`: app state
- `ctx.kind`: active palette kind
- `ctx.input`: current input text
- `ctx.seed`: optional seed string

`selected` is the currently highlighted visible candidate, if any.

## Input modes

- `FilterCandidates`
  - Runtime filters candidates based on input text.
- `FreeText`
  - Input is provider-owned command/query text.
- `Custom`
  - Provider defines its own list/input strategy in `list()`.

## Keyboard semantics

- `Esc`: close current palette.
- `Up` / `Down` / `Ctrl+P` / `Ctrl+N`: move selection.
- `Tab`: apply provider `on_tab`.
- `Enter`: apply provider `on_submit`.

## Tab effect contract

`on_tab` returns:
- `Noop`
- `SetInput { value, move_cursor_to_end }`

`SetInput` semantics:
- replace input with `value`
- move cursor when requested
- rebuild title/candidates/assistive text

## Submit effect contract

`on_submit` returns:
- `Close`
- `Reopen { kind, seed }`
- `Dispatch { command, next }`

`Dispatch` transaction order:
1. close current palette
2. dispatch command
3. apply queued open/close requests
4. apply `next`

## Assistive text row

- Providers may return one optional assistive text line.
- Palette popup layout:
  1. input
  2. assistive text
  3. candidate list

## Built-in providers

## Command palette (`PaletteKind::Command`)

- Open shortcut in normal mode: `:`
- Enter behavior:
  1. If input parses as a valid command with args, dispatch directly.
  2. Else if selected candidate has no args, dispatch directly.
  3. Else if selected candidate requires args, reopen with `seed = "{command-id} "`.
  4. Otherwise reopen preserving input.
- `Tab` may autocomplete from selected candidate.
- If input includes whitespace (argument phase), candidate list is hidden.

## Search palette (`PaletteKind::Search`)

- Open shortcut in normal mode: `/`
- Also invocable by command palette command: `search`
- Input mode: `FreeText`
- Candidate list exposes matcher choices:
  - `contains-insensitive` (default)
  - `contains-sensitive`
- Enter dispatches `palette-search-submit` with query + matcher and closes.
- Empty input on Enter reopens for correction.

## History palette (`PaletteKind::History`)

- Open via command palette command: `history`
- Input mode: `FilterCandidates`
- `initial_input` returns empty text; seed is used as serialized context.
- Candidates include back stack, current page, and forward stack (newest first).
- Current page is marked and not jump-targetable.
- Enter dispatches `history-goto` with selected page and closes.
