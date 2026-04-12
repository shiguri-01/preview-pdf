# Architecture

This document describes the current implementation structure.

## Scope

This document owns:

- top-level runtime structure
- subsystem ownership
- loop-level event routing
- module navigation guidance

Feature behavior lives in `runtime-spec.md` and the subsystem specs rather than
here.

## Top-level runtime model

- `App` owns the interactive runtime state and the major subsystems.
- The main event loop routes typed `DomainEvent` values.
- Commands are parsed and dispatched into typed outcomes and `AppEvent` values.
- Rendering and presenter work are performed asynchronously and fed back into
  the event loop.
- Extensions and palette providers are wired statically at construction time.

## Main subsystems

- `src/app/`
  - app construction, event loop orchestration, input handling, render
    completion handling, and view operations

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
- key handling either mutates state directly, dispatches a command, or forwards
  work to a palette or extension path
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
- render and presenter scheduling share `WorkClass` to classify priority and
  stale-generation handling

## Code references

- `src/app/core.rs`
- `src/app/event_loop.rs`
- `src/event.rs`
- `src/command/`
- `src/extension/host.rs`
- `src/palette/registry.rs`
