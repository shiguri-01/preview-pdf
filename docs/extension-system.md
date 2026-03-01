# pvf Extension System Specification

This document defines extension contracts and runtime behavior in `pvf`.

## Design constraints

- Extensions are internal modules.
- Extension dispatch is static and typed.
- Extension state is stored as concrete fields in `ExtensionHost`.
- Extension hooks are explicit: input, event, background.

## Extension contract

`src/extension/traits.rs`:

```rust
pub trait Extension {
    type State: Send;

    fn init_state() -> Self::State;

    fn handle_input(
        state: &mut Self::State,
        event: AppInputEvent,
        app: &mut AppState,
    ) -> InputHookResult;

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState);

    fn on_background(state: &mut Self::State, app: &mut AppState) -> bool;
}
```

## Host composition

`src/extension/host.rs` owns:

```rust
pub struct ExtensionHost {
    search: SearchState,
    history: HistoryState,
    search_engine: SearchEngine,
}
```

Dispatch order is fixed:
1. `SearchExtension`
2. `HistoryExtension`

## Input/event/background flow

- Input flow:
  - `ExtensionHost::handle_input()` invokes search, then history.
  - First non-`Ignored` hook result is applied.

- Event flow:
  - `command::dispatch()` emits typed `AppEvent` values.
  - Main loop re-enqueues them as `DomainEvent::App`.
  - `ExtensionHost::handle_event()` forwards each event to both extensions.

- Background flow:
  - Main loop calls `command::drain_background_events()`.
  - Host polls extension background hooks.
  - Search results are drained through host-owned `SearchEngine`.

## Built-in extensions

### Search (`src/search/`)

State (`SearchState`) contains:
- query and matcher
- generation
- progress counters
- hit list and selected hit
- latest error

Behavior:
- opens search palette
- submits async search jobs
- moves to next/previous hit
- consumes background search events (`Snapshot`, `Completed`, `Failed`)

### History (`src/history/`)

State (`HistoryState`) contains:
- `back_stack` and `forward_stack` (capacity: 64)
- `suppress_next_record`

Behavior:
- history back/forward/goto
- opens history palette
- records navigation from `AppEvent::PageChanged`

## Shared extension event types

`src/extension/events.rs`:

```rust
enum NavReason {
    Step,
    Jump,
    Search(String),
    History,
}

enum AppEvent {
    CommandExecuted { id: ActionId, outcome: CommandOutcome },
    PageChanged { from: usize, to: usize, reason: NavReason },
    ModeChanged { from: Mode, to: Mode },
}
```

## Adding an extension

1. Define a concrete state type and `Extension` implementation.
2. Add the state field to `ExtensionHost`.
3. Wire dispatch in host methods:
   - `handle_input`
   - `handle_event`
   - background drain path
4. Add command variants/dispatch behavior when required.
5. Add palette provider and `PaletteKind` support when UI exposure is needed.
