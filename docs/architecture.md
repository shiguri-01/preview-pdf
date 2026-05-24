# Architecture

This document describes the current implementation structure.

## Scope

This document owns:

- top-level runtime structure
- subsystem ownership
- loop-level event routing
- module navigation guidance

Feature behavior lives in `runtime-spec.md` and the subsystem specs rather than
here. Worker ownership and cancellation details live in `async-workers.md`.

## Top-level runtime model

- `App` owns the interactive runtime state and the major subsystems.
- The main event loop waits for typed `DomainEvent` values and delegates
  routing to the loop router.
- Commands are parsed and dispatched into typed outcomes and `AppEvent` values.
- Rendering and presenter work are performed asynchronously and fed back into
  the event loop.
- Extensions and palette providers are wired statically at construction time.

## Main subsystems

- `src/app/`
  - app construction, event loop orchestration, loop event routing, input
    handling, render completion handling, and view operations
  - `event_loop.rs` owns loop setup and wait/select orchestration
  - `loop_runtime.rs` owns loop-local runtime state shared by the loop shell
    and router
  - `loop_router.rs` routes `DomainEvent` values and applies loop-level
    effects such as command enqueueing, redraw requests, and quit requests
  - `actors.rs` owns loop-adjacent input, render, and UI actors

- `src/command/`
  - command catalog, parser, dispatch, invocation checks, and command outcomes

- `src/event.rs`
  - loop-level event types, including `DomainEvent`, `AppEvent`, and
    navigation reasons

- `src/render/`
  - L1 rendered-page cache, render scheduling, prefetch queue, and render
    workers

- `src/presenter/`
  - protocol picker, terminal-frame L2 cache, encode workers, and draw path

- `src/extension/`
  - extension contract, extension host, and shared extension-facing types

- `src/palette/`
  - palette session management, provider contract, shared palette types, and
    provider registry

- `src/search/`, `src/history/`, `src/outline/`
  - current extension and palette-provider implementations

- `src/backend/`
  - PDF backend abstraction and the default backend implementation

- `src/input/`
  - keymap definitions, sequence resolution, and palette input history

- `src/ui/`
  - layout, chrome, overlays, and help rendering

## Event flow

- terminal input enters the runtime as `DomainEvent::Input`
- `loop_router.rs` handles `DomainEvent` dispatch and keeps the main loop
  focused on orchestration
- key handling either mutates state directly, dispatches a command, or forwards
  work to a palette or extension path
- input, render, and UI actors own the loop-level decisions for their areas:
  sequence timeouts and input effects, render/navigation synchronization and
  work enqueueing, and redraw/frame rendering respectively
- command dispatch emits typed `AppEvent` values when state changes need to be
  observed by the rest of the runtime
- render workers report finished raster work through `DomainEvent::RenderComplete`
- presenter encode workers report background completion through
  `DomainEvent::EncodeComplete`
- timer- and wake-based events drive redraw and prefetch progress when needed

## Structural constraints

- command dispatch is statically typed
- palette provider resolution is a static match on `PaletteKind`
- current extension ownership is explicit in `ExtensionHost`
- extension UI data is exposed to palettes through `ExtensionUiSnapshot`
- app data exposed to palettes is limited to a read-only palette snapshot
- render and presenter scheduling share `WorkClass` to classify priority and
  stale-generation handling

## Code references

- `src/app/core.rs`
- `src/app/event_loop.rs`
- `src/app/loop_router.rs`
- `src/app/loop_runtime.rs`
- `src/app/actors.rs`
- `src/event.rs`
- `src/command/`
- `src/extension/host.rs`
- `src/palette/registry.rs`
- `docs/async-workers.md`
