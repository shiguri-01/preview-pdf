# Architecture

`pvf` is a Rust CLI/TUI PDF viewer. The runtime is organized around one
interactive `App`, a typed event loop with driver-controlled execution,
static command and palette registries, internal extensions, asynchronous
render and encode workers, and a backend abstraction for PDF data.

## Runtime Flow

Startup begins in [src/main.rs](../src/main.rs).

1. CLI arguments are parsed into `AppOptions` patches.
2. Configuration is loaded and merged with built-in defaults and CLI options.
3. The default backend opens the requested PDF.
4. `App` is built with resolved view, input, render, cache, watch, and
   extension policies.
5. The terminal session starts the event loop.
6. The loop receives typed `DomainEvent` values, routes input and worker
   completions, dispatches commands, drains extension background work, schedules
   render and presenter work, and asks the UI to draw frames.

Rationale: startup keeps option resolution outside the event loop so the loop
can operate on resolved policies rather than re-reading config or CLI state.

## Subsystems

- [src/app/](../src/app/) owns interactive runtime state, event-loop orchestration, input
  handling, render completion handling, view operations, and terminal session
  coordination.
- [src/command/](../src/command/) owns command ids, metadata, parsing, source-aware validation,
  dispatch, typed command outcomes, and command effects.
- [src/input/](../src/input/) owns key sequence normalization, numeric prefixes,
  and input history used by palette inputs.
- [src/config/](../src/config/) owns config loading and option resolution.
- [src/palette/](../src/palette/) owns palette sessions, provider lookup, candidate matching,
  selection state, completion, submit, cancel, palette input state, and rendered
  palette views. It does not own raw terminal key routing.
- [src/extension/](../src/extension/) owns the extension host contract and the composition of
  built-in extension state.
- [src/search/](../src/search/), [src/history/](../src/history/), and
  [src/outline/](../src/outline/) provide current extension and palette-provider behavior.
- [src/render/](../src/render/) owns L1 rendered-page caching, scheduling, prefetch, render
  worker messages, stale-result acceptance, and cancellation metadata.
- [src/presenter/](../src/presenter/) owns terminal image protocol selection, L2 terminal-frame
  caching, encode workers, slot drawing, and presenter feedback.
- [src/backend/](../src/backend/) owns the PDF backend trait and default backend implementation.
- [src/ui/](../src/ui/) owns layout, chrome, overlays, help, theme, and frame composition.
- [src/perf/](../src/perf/) owns headless performance diagnostics, scenario
  drivers, and JSON report construction.
- [src/metrics.rs](../src/metrics.rs) owns low-level runtime and presenter
  metric primitives shared by diagnostics and runtime instrumentation.

## Dependency Direction

`app` coordinates other subsystems and is allowed to depend on command, input,
palette, extension, render, presenter, backend, and UI types.

Commands do not own feature state. Command dispatch receives an execution
context from `app`, mutates app-owned state through that context, and applies
typed command effects for notices, app events, palette requests, input history,
follow-up commands, and lifecycle requests.

Palette providers receive read-only app and extension snapshots. They should
request behavior by returning typed effects or commands instead of taking
`AppState` directly. Surface-local operations such as palette submit, palette
selection, palette input editing, palette input history recall, and help
scrolling enter the same command dispatch path as normal-mode keymap entries.
Palette open requests carry only common initialization. Provider-owned UI data
crosses through snapshots instead of being embedded in open requests.

Extensions own extension-local state and observe `AppEvent` values.
`ExtensionHost` owns concrete extension state, hook routing, and shared
snapshots. Feature behavior stays with the feature modules. Shared UI data
crosses from extensions to palettes through `ExtensionUiSnapshot`.

Render workers and presenter encode workers communicate with the loop through
typed request and completion values. They do not mutate app state directly.

Performance diagnostics drive the same event loop through the app-owned loop
driver contract. The `perf` subsystem owns headless scenarios and reports;
`app` owns terminal/session coordination and does not depend on performance
diagnostic scenario types.

Rationale: the central `App` boundary keeps mutable runtime state in one place,
while commands, palettes, extensions, and workers remain testable through typed
contracts.

## Mutable State Ownership

`AppState` is the primary owner of viewer state: current page, mode, zoom, pan,
notices, debug status, and view policy. `App` owns `AppState` plus the runtime
objects that act on it: command execution context, palette session controller,
extension host, render runtime, presenter, input sequence resolver, and watch
state.

Subsystem-local mutable state stays with the subsystem that owns its invariants:

- search, history, and outline state live in `ExtensionHost`
- active palette session state lives in the palette session controller
- L1 rendered-page cache state lives in `render`
- L2 terminal-frame cache and encode generation state live in `presenter`
- input sequence buffers live in `input`

Rationale: state is separated by the boundary that can validate it. Cross-boundary
communication uses snapshots, command requests, events, or worker completions.

## Event Flow

Terminal input enters as `DomainEvent::Input`.

1. The loop router delegates input to app input handling.
2. App input handling builds a keymap context from the active surface and
   shared runtime condition state, such as palette kind, palette input history
   availability, and active search.
3. Extension input hooks may intercept extension-local inputs in normal mode
   when no pending key sequence owns the input.
4. The input sequence resolver evaluates every keymap entry against the current
   shared runtime conditions and maps matching key sequences to typed commands.
   All resolved keymap entries dispatch as binding input.
5. Command dispatch validates invocation source, resolves the required target
   such as app, active palette, or active help, checks the
   command `enabled_when` runtime condition, applies behavior, and applies
   typed command effects.
6. The loop re-routes emitted app events, follow-up command requests, palette
   requests, and lifecycle requests to extensions and other loop effects.
   Binding commands and internal follow-up commands use distinct invocation
   sources.
7. Render workers return `DomainEvent::RenderComplete`; presenter encode
   workers return `DomainEvent::EncodeComplete`.
8. UI redraws happen when input, command effects, extension background work, or
   worker completions make visible state change.

Search worker events are drained by the search extension during background
handling rather than entering the loop as `DomainEvent` values.

Rationale: the loop uses typed event values instead of ad hoc callbacks so
worker results, command outcomes, extension reactions, and UI redraw decisions
can be reasoned about separately.

## Boundary Rationale

Command catalog:
The command catalog is the owning inventory for command ids, metadata, parser
routing, and dispatch routing. Docs describe stability and invocation policy;
tests guard registry consistency.

Palette providers:
Providers own candidate generation, completion semantics, and submit semantics
for their palette kind, while the palette session controller owns common
session state and operation methods. Key routing remains outside providers and
produces commands; the active palette is the command target for palette
operations.

Extensions:
Built-in features that need background state, event observation, status-bar
segments, palette-facing snapshots, or render projections live behind
`ExtensionHost`. The host owns hook order and snapshot composition. They are
internal modules, not a dynamic plugin system.

Render and presenter:
Raw page rasterization and terminal protocol encoding are separated because
their cache identities, stale-result rules, and performance costs differ.

Performance diagnostics:
Headless diagnostics are modeled as loop drivers so they exercise the same
runtime path as the interactive viewer. Low-level metrics live outside `perf`
because render, presenter, and app instrumentation can record them without
depending on diagnostic report ownership.

Backend:
The backend trait isolates PDF opening, rasterization, text extraction, and
outline extraction from the interactive runtime.
