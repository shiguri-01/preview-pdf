# Palette Provider Specification

This document is the source of truth for palette contracts and current palette
behavior.

## Scope

This document owns `PaletteKind`, `PaletteProvider`, palette session and
selection behavior, palette-local keyboard semantics, and built-in palette
behavior.

## Runtime model

- palette instances are identified by `PaletteKind`
- provider resolution is static via `PaletteRegistry`
- a palette session has a session id, and transitions validate the active
  session id

## Provider contract

`PaletteProvider` exposes:

- `kind() -> PaletteKind`
- `title(ctx) -> String`
- `input_mode() -> PaletteInputMode`
- `list(ctx) -> AppResult<Vec<PaletteCandidate>>`
- `on_tab(ctx, selected) -> AppResult<PaletteTabEffect>`
- `on_submit(ctx, selected) -> AppResult<PaletteSubmitEffect>`
- `assistive_text(ctx, selected) -> Option<String>`
- `reset_selection_on_input_change() -> bool`
- `initial_selected_candidate(ctx, candidates) -> Option<usize>`
- `initial_input(open_payload) -> String`

`PaletteContext` includes current app state, palette kind, input text,
optional `open_payload`, and extension UI snapshot data.

## Candidate and rendering contract

`PaletteCandidate` separates:

- `left` text parts
- `right` text parts
- `search_texts` for matching

Rules:

- candidate search input is independent from rendered row text
- the renderer lays out both sides and applies row-wide selection highlighting
- when width is constrained, the renderer preserves row meaning rather than a
  rigid left/right split

## Input modes

- `FilterCandidates`
  - runtime filtering is driven by the input text
- `FreeText`
  - provider owns the meaning of the input text
- `Custom`
  - provider owns how input and candidate generation interact

Selection rules:

- providers opt into selection reset by returning `true` from
  `reset_selection_on_input_change()`
- providers can override the initial highlight with
  `initial_selected_candidate(...)`
- `initial_input(open_payload)` defaults to the payload's visible input text and
  may be overridden when open payload data should not appear verbatim in the
  input field

## Keyboard semantics

- `<esc>` closes the current palette
- `<c-p>` / `<c-n>` move the current selection
- `<up>` / `<down>` recall input history in command and search palettes
- `<up>` / `<down>` move selection in history and outline palettes
- `<tab>` applies the provider `on_tab` effect
- `<enter>` applies the provider `on_submit` effect

## Tab and submit effects

`on_tab` returns:

- `Noop`
- `SetInput { value, move_cursor_to_end }`

`on_submit` returns:

- `Close`
- `Reopen { kind, payload }`
- `Dispatch { command, history_record, next }`

Dispatch order:

1. close the current palette
2. record optional input history
3. queue or return the command request for later dispatch
4. apply the queued next action

## Assistive text and input line

- providers may expose one assistive text row
- palette layout is input line, then assistive text, then candidate list
- while a palette is open, the terminal cursor is shown at the current input
  position
- the input line does not draw a software caret

## Current palette behavior

### Command palette

- kind: `PaletteKind::Command`
- open shortcut: `:`
- input mode: `Custom`
- `<up>` / `<down>` recall recent command inputs
- `<c-p>` / `<c-n>` move candidate selection
- `Tab` autocompletes from the selected candidate and appends one trailing
  space
- enum-valued arguments keep the candidate list visible during argument entry
- non-enum arguments still hide the candidate list during argument entry
- candidate ranking and argument-phase handling are provider-defined rather than
  generic filter-mode behavior
- internal-only commands are never listed
- runtime-gated commands are listed only when their availability conditions are
  met
- direct typed invocation still enforces the same exposure and availability
  checks
- assistive text uses English type labels for free-form arguments and literal
  value lists for enum arguments, such as `integer`, `number`, `text`, or
  `ltr / rtl`

Enter behavior:

1. if an enum candidate is selected, accept that value and dispatch
   immediately when the resulting command is complete
2. otherwise, if an enum candidate is selected, reopen with the accepted value
   so later arguments can be entered
3. otherwise dispatch typed input directly when it parses as a valid command
4. otherwise dispatch the selected command when it needs no arguments
5. otherwise reopen with the selected command id plus trailing space
6. otherwise reopen preserving input

During argument entry, `Enter` follows the same submit rules as any other
palette state: selected enum candidates take precedence over parsing the
current input.

### Search palette

- kind: `PaletteKind::Search`
- open shortcut: `/`
- command entry point: `search`
- input mode: `FreeText`
- active search reopens with the current query prefilled and current matcher selected
- `<up>` / `<down>` recall recent search queries
- `<c-p>` / `<c-n>` select the matcher candidate
- matcher candidates are `contains-insensitive` and `contains-sensitive`
- search history stores only the query text
- pressing Enter with empty input reopens for correction
- successful submit dispatches internal search-submit behavior and closes

### History palette

- kind: `PaletteKind::History`
- command entry point: `history`
- input mode: `Custom`
- `PaletteOpenPayload::HistorySeed` is used as serialized context, while
  visible input starts empty
- candidates are shown in navigation order around the current page
- the current page is selected when the palette opens
- matching uses signed index, then formatted reason text, then page label
- Enter dispatches internal history-goto behavior and closes

### Outline palette

- kind: `PaletteKind::Outline`
- command entry point: `outline`
- input mode: `Custom`
- candidates come from extension-owned cached outline data
- visible rows flatten the hierarchy depth-first and show indentation for depth
- detail text shows page labels as `p.N`
- matching uses outline title first, then page label
- an empty outline list is valid and shows assistive text
- Enter dispatches internal outline-goto behavior and closes

## Code references

- `src/palette/kind.rs`
- `src/palette/types.rs`
- `src/palette/manager.rs`
- `src/palette/registry.rs`
- `src/palette/providers/command.rs`
- `src/search/palette.rs`
- `src/history/palette.rs`
- `src/outline/palette.rs`
