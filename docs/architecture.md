# pvf Architecture Specification

This document defines the current architecture of `pvf`.

## System properties

- Command and extension dispatch are statically typed.
- Palette kind selection is statically typed through `PaletteKind`.
- Main-loop message routing uses typed `DomainEvent`.
- Runtime tuning is configured by `config.toml` with default fallbacks.
- Error reporting uses typed `AppError` variants with contextual fields.
- Viewer page layout supports typed `single` / `spread` mode with `ltr` / `rtl` spread direction.

## Top-level module map

- `src/command/`
  - `types.rs`: `Command`, invocation metadata types, matcher kinds, layout command argument kinds, command outcome types, stable `ActionId`s for emitted command events.
  - `spec.rs`: command catalog plus visibility/invocation/availability policy.
  - `parse.rs`: command text parser and direct-invocation validation.
  - `core.rs`: command implementations (navigation/zoom/layout/debug).
  - `dispatch.rs`: typed dispatch entry point returning `CommandDispatchResult`, with source-aware invocation checks.

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
  - `state.rs`: app state (current page, layout mode/direction, transient notice, cache refs).
  - `input_ops.rs`: key routing, palette flow, extension background drain, command dispatch wiring.
  - `render_ops.rs`: render completion ingestion, prefetch dispatch, visible-page enqueue.
  - `view_ops.rs`: viewport/scale helpers and frame draw operations.
  - `event_bus.rs`: input stream pump and loop event sender.
  - `terminal_session.rs`: terminal lifecycle and `TerminalSurface` abstraction.

- `src/extension/`
  - `traits.rs`: static extension contract.
  - `host.rs`: concrete extension ownership and dispatch chain.
  - `events.rs`, `input.rs`: shared extension event/input types.

- `src/outline/`
  - `state.rs`: lazy outline loading, per-document cache, outline jump behavior.
  - `palette.rs`: outline palette candidate projection and submit behavior.
  - `mod.rs`: `OutlineExtension` wiring.

- `src/search/`
  - `state.rs`: search state and background result application (search UI state lives here, not in `AppState`).
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
  - candidate display data and structured search metadata are both carried on `PaletteCandidate`.

- `src/render/`
  - `cache.rs`: L1 rendered-page cache.
  - `scheduler.rs`: prefetch plan builder.
  - `prefetch.rs`: priority queue with dedup/stale cancellation.
  - `worker.rs`: render worker pool and result channel.

- `src/presenter/`
  - `mod.rs`: presenter API surface and factory.
  - `ratatui.rs`: ratatui-based presenter implementation.
  - `encode.rs`: current/background 2-lane encode workers.
  - `l2_cache.rs`: terminal-frame L2 cache.

- `src/work.rs`
  - `WorkClass`: shared work classification for render scheduling, prefetch queueing, and presenter encode routing.

- `src/backend/`
  - `traits.rs`: `PdfBackend`, `RgbaFrame`, `OutlineNode`.
  - `hayro.rs`: default backend implementation, factory, and PDF-outline extraction over `hayro_syntax`.

- `src/config.rs`
  - `Config`, `RenderConfig`, `CacheConfig`.
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
  - `keymap.rs`: built-in global key binding definitions.
  - `sequence.rs`: runtime multi-key sequence registry/resolver with timeout-based confirmation.

- `src/ui/`
  - `layout.rs`: region geometry.
  - `chrome.rs`: status, transient notices, compact presenter/protocol diagnostics.
  - `overlay.rs`: loading/error/palette overlays.

## Structural constraints

- `App` owns grouped subsystems:
  - `RenderSubsystem`
  - `InteractionSubsystem` (`ExtensionSubsystem`, `PaletteSubsystem`)
- Extension dispatch order is fixed in code.
- Palette providers are resolved via static `match` on `PaletteKind`.
- Palette providers consume extension-owned UI snapshot data (`ExtensionUiSnapshot`) for command availability/state checks.
- Render and search workers support backend-loader injection via typed contracts.
