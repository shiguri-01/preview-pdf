# pvf Architecture Specification

This document defines the current architecture of `pvf`.

## System properties

- Command and extension dispatch are statically typed.
- Palette kind selection is statically typed through `PaletteKind`.
- Main-loop message routing uses typed `DomainEvent`.
- Runtime tuning is configured by `config.toml` with default fallbacks.
- Error reporting uses typed `AppError` variants with contextual fields.

## Top-level module map

- `src/command/`
  - `types.rs`: `Command`, matcher kinds, command outcome types.
  - `spec.rs`: command catalog for parser/palette.
  - `parse.rs`: command text parser.
  - `core.rs`: command implementations (navigation/zoom/debug).
  - `dispatch.rs`: typed dispatch entry point returning `CommandDispatchResult`.
  - `ActionId`: stable typed identifier for executed commands.

- `src/event.rs`
  - Defines `DomainEvent` for loop-level routing:
    - input
    - command
    - app events
    - render completion
    - wake/timer events

- `src/app/`
  - `core.rs`: application construction and subsystem ownership.
  - `event_loop.rs`: event-driven orchestration with `tokio::select!`.
  - `actors.rs`: loop-local actor state (`InputActor`, `RenderActor`, `UiActor`).
  - `input_ops.rs`: key routing, palette flow, extension background drain, command dispatch wiring.
  - `render_ops.rs`: render completion ingestion, prefetch dispatch, current-page enqueue.
  - `view_ops.rs`: viewport/scale helpers and frame draw operations.
  - `event_bus.rs`: input stream pump and loop event sender.
  - `terminal_session.rs`: terminal lifecycle and `TerminalSurface` abstraction.

- `src/extension/`
  - `traits.rs`: static extension contract.
  - `host.rs`: concrete extension ownership and dispatch chain.
  - `events.rs`, `input.rs`: shared extension event/input types.

- `src/search/`
  - `state.rs`: search state and background result application.
  - `engine.rs`: async search worker and job lifecycle.
  - `palette.rs`: search palette provider.
  - `mod.rs`: `SearchExtension` wiring.

- `src/history/`
  - `state.rs`: history stacks and navigation logic.
  - `palette.rs`: history palette provider.
  - `mod.rs`: `HistoryExtension` wiring.

- `src/palette/`
  - `types.rs`: palette request/result/effect types.
  - `provider.rs`: `PaletteProvider` contract.
  - `registry.rs`: static provider wiring by `PaletteKind`.
  - `manager.rs`: palette UI state machine.

- `src/render/`
  - `cache.rs`: L1 rendered-page cache.
  - `scheduler.rs`: prefetch plan builder.
  - `prefetch.rs`: priority queue with dedup/stale cancellation.
  - `worker.rs`: render worker pool and result channel.

- `src/presenter/`
  - `mod.rs`: presenter API surface and factory.
  - `ratatui.rs`: ratatui-based presenter implementation.
  - `encode.rs`: encode worker.
  - `l2_cache.rs`: terminal-frame L2 cache.

- `src/backend/`
  - `traits.rs`: `PdfBackend`, `RgbaFrame`.
  - `hayro.rs`: default backend implementation and factory.

- `src/config.rs`
  - `Config`, `RenderConfig`, `CacheConfig`, `KeymapConfig`.
  - Config path resolution priority:
    1. `PVF_CONFIG_PATH`
    2. `XDG_CONFIG_HOME/pvf/config.toml`
    3. `HOME/.config/pvf/config.toml`
    4. `APPDATA/pvf/config.toml`
  - Missing config file falls back to built-in defaults.

- `src/error.rs`
  - `AppError` variants:
    - `Io { source, context }`
    - `PdfRender { page, source }`
    - `InvalidArgument(String)`
    - `Unsupported(String)`
    - `Unimplemented(String)`

- `src/input/`
  - `handler.rs`: input event routing entry points.
  - `keymap.rs`: keymap preset support (`default`, `emacs`).

- `src/ui/`
  - `layout.rs`: region geometry.
  - `chrome.rs`: status/debug bars.
  - `overlay.rs`: loading/palette overlays.

## Structural constraints

- `App` owns grouped subsystems:
  - `RenderSubsystem`
  - `InteractionSubsystem` (`ExtensionSubsystem`, `PaletteSubsystem`)
- Extension dispatch order is fixed in code.
- Palette providers are resolved via static `match` on `PaletteKind`.
- Render and search workers support backend-loader injection via typed contracts.
