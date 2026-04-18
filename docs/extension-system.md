# Extension System Specification

This document is the source of truth for extension contracts and current
extension behavior.

## Scope

This document owns the `Extension` trait contract, `ExtensionHost` composition
and dispatch order, extension input/event/background/status-bar behavior, and
built-in extension behavior.

Palette behavior is specified in `palette-provider.md`.

## Design constraints

- extensions are internal modules
- dispatch is static and typed
- extension state is stored as concrete fields on `ExtensionHost`
- extension UI data is exposed to palettes through `ExtensionUiSnapshot`

## Extension contract

Each extension defines:

- one concrete state type
- an initialization path for that state
- an input hook for extension-local interception
- an event hook for observing `AppEvent`
- a background hook for non-input progress
- an optional status-bar segment projection

Contract rules:

- extension state must be sendable across runtime boundaries
- extension hooks operate on extension-owned state plus shared app state
- input hooks return `Ignored` when they do not claim the event
- background hooks report whether they changed visible or behavioral state
- status-bar output is optional

## Host composition

`ExtensionHost` owns:

- search runtime state
- history state
- outline state

Dispatch rules:

- input hooks run search first, then history
- outline does not participate in extension input interception
- event hooks run search, history, then outline
- background drain currently polls search and history

## Runtime flows

- Input flow
  - extension input hooks are for extension-local interception
  - built-in global shortcuts stay in app keymap and command routing
  - the first non-ignored input hook result wins

- Event flow
  - command dispatch emits typed `AppEvent` values
  - the main loop re-enqueues them as `DomainEvent::App`
  - the extension host forwards each event to all current extensions that
    subscribe

- Background flow
  - search drains asynchronous search worker results
  - history may update state from loop-local background checks
  - outline currently has no background polling path

- Status-bar flow
  - the UI asks the host for extension segments
  - non-empty extension segments are appended in host order

- Palette flow
  - the host exposes `ExtensionUiSnapshot`
  - current snapshot data includes `search_active`,
    `search_palette_initial_matcher`, and cached outline entries

## Current extensions

### Search

State tracks:

- query and matcher
- generation
- progress counters
- hit list and selected hit
- latest error

Behavior:

- opens the search palette
- submits asynchronous search jobs
- cancels active search work
- moves to next or previous search hit
- consumes background search completion and progress events
- contributes compact search progress and hit status to the status bar while
  active

### History

State tracks:

- `back_stack`
- `forward_stack`
- `suppress_next_record`

Behavior:

- performs history back, forward, and direct goto
- opens the history palette
- records navigation from `AppEvent::PageChanged`
- clears forward history on non-history navigation
- records reason-bearing transitions such as goto, search, and outline

### Outline

State tracks:

- extracted outline entries cached for palette use

Behavior:

- opens the outline palette
- loads and flattens outline/bookmark entries from the backend
- ignores unsupported or non-page destinations
- jumps to the selected outline page through command flow

## Shared event types

`src/event.rs` defines:

- `NavReason`
  - `Step`
  - `Goto(GotoKind)`
  - `Search { query }`
  - `History(HistoryOp)`
  - `Outline { title }`
  - `LayoutNormalize`

- `AppEvent`
  - `CommandExecuted`
  - `PageChanged`
  - `ModeChanged`

These types belong to core runtime flow and are consumed by extensions for
recording and state updates.

## Code references

- `src/extension/traits.rs`
- `src/extension/host.rs`
- `src/event.rs`
- `src/search/`
- `src/history/`
- `src/outline/`
