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

`PaletteCandidate` carries display segments for the candidate row:

- `left`: primary row content, rendered from one or more text segments
- `right`: trailing detail content, rendered from one or more text segments
- each text segment has a tone, currently `Primary` or `Secondary`
- `search_texts`: structured search inputs used by the shared matcher

`search_texts` is independent from the rendered row content. Providers should
populate it from the candidate's existing structured data so matching can use
values that are not shown directly in the UI.

The palette renderer is responsible for laying out both sides, reserving the
trailing padding space, and applying selection highlighting to the whole row.
Selection highlighting is palette-wide and does not vary by palette kind.

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
- While a palette is open, the terminal cursor is shown at the current input position.
- The input line itself does not draw a software caret; cursor visibility is delegated to the terminal.

## Built-in providers

## Command palette (`PaletteKind::Command`)

- Open shortcut in normal mode: `:`
- Enter behavior:
  1. If input parses as a valid command with args, dispatch directly.
  2. Else if selected candidate has no args, dispatch directly.
  3. Else if selected candidate requires args, reopen with `seed = "{command-id} "`.
  4. Otherwise reopen preserving input.
- `Tab` autocompletes from selected candidate and always appends one trailing space.
- Candidate rows render command `id` and `usage` on the left, with the command title on the right in secondary color.
- Candidate search also uses command metadata beyond the rendered row, so ids,
  titles, and argument-related text all participate in filtering/ranking.
- If input includes whitespace (argument phase), candidate list is hidden.
- Candidate ranking uses command-aware scoring:
  - command `id` (hyphen-separated lowercase) is the primary target.
  - `title` is a weaker secondary target.
  - acronym-style queries from id tokens are supported (for example, `nsh` -> `next-search-hit`).
- Candidate visibility is derived from command metadata.
  - internal-only commands are never listed
  - commands with availability conditions are listed only when all conditions are met
  - `next-search-hit` / `prev-search-hit` are shown only while search is active
- Hand-typed command execution also respects command metadata.
  - internal-only commands cannot be invoked from command palette input
  - commands gated by runtime conditions cannot be invoked until those conditions are met

## Search palette (`PaletteKind::Search`)

- Open shortcut in normal mode: `/`
- Also invocable by command palette command: `search`
- Input mode: `FreeText`
- Candidate list exposes matcher choices:
  - `contains-insensitive` (default)
  - `contains-sensitive`
- Enter dispatches internal search-submit behavior with query + matcher and closes.
- Empty input on Enter reopens for correction.

## History palette (`PaletteKind::History`)

- Open via command palette command: `history`
- Input mode: `FilterCandidates`
- `initial_input` returns empty text; seed is used as serialized context.
- Candidates include back stack, current page, and forward stack (newest first).
- Current page is marked and not jump-targetable.
- Candidate matching uses page numbers and navigation reason text, not only the
  rendered row label.
- Enter dispatches internal history-goto behavior with selected page and closes.

## Outline palette (`PaletteKind::Outline`)

- Open via command palette command: `outline`
- Input mode: `FilterCandidates`
- Candidate source is extension-owned cached outline data, not palette seed serialization.
- Candidates are flattened depth-first for display only.
- Hierarchy is represented with indentation in the left-side title; detail shows the page number in loading-overlay format (`p.12`).
- Candidate matching uses outline title and page metadata.
- Enter dispatches internal outline-goto behavior with the resolved page and closes.
- Empty outline state is valid and shows assistive text indicating that the document has no usable outline entries.
