# Architecture

`pvf` is a Rust CLI/TUI PDF viewer built around a single interactive runtime.
This document defines the project-level architecture: the runtime shape,
ownership boundaries, and design constraints that should guide code changes.
It is not the source of truth for exact behavior; the Rust modules and tests
own implementation details, inventories, and edge cases.

## Runtime Shape

The interactive runtime is centered on one `App` and a typed event loop.
Startup resolves inputs such as CLI arguments, configuration, backend state,
view policy, and runtime services before the terminal loop begins. The loop
then operates on resolved policy instead of reinterpreting startup state.

Runtime work enters the loop as typed data: input, worker completions, and
internal effects. The loop routes that data through commands, extensions,
rendering, presentation, and redraw decisions. The important architectural rule
is that state changes happen through owned runtime objects, not through hidden
callbacks or cross-subsystem mutation.

Headless performance diagnostics use the same loop-driver shape as the
interactive viewer. They should measure the real runtime path instead of
maintaining a parallel simulation of viewer behavior.

## Ownership Boundaries

`App` is the coordination boundary. It owns viewer state and the runtime
objects that act on that state. Other subsystems communicate with the app
through typed operations, snapshots, requests, effects, or completions. They do
not reach across the boundary to mutate viewer state directly.

Commands are the main behavior boundary for user-initiated actions. The command
catalog owns command identity, metadata, parsing, validation, dispatch routing,
and typed effects. Feature state belongs with the feature owner, not in the
catalog. Command execution receives app-provided context and returns effects
for the loop to apply.

Palettes are a shared interaction surface, not independent mini-apps. Common
session state and surface operations belong to the palette controller. Providers
own candidate generation and provider-specific submit or completion semantics.
Provider data crosses into the UI as snapshots or typed results, while key
routing and command dispatch stay outside providers.

Extensions are internal feature hosts, not a dynamic plugin system. Features
that need background work, event observation, status-bar output, palette-facing
snapshots, or render projections live behind the extension host. The host owns
hook routing and snapshot composition; individual extensions own their local
invariants. Extension-owned worker output returns through the app loop before
it mutates extension-owned state.

Rendering and presentation are separated because they have different costs and
cache identities. Rendering produces page images from PDF data. Presentation
turns rendered pages into terminal-specific frames. Worker results return to
the loop as typed completions, so stale results, cancellation, and redraw
decisions stay under app-owned coordination.

The backend boundary isolates PDF operations from the interactive runtime.
Opening documents, rasterizing pages, extracting text, and reading outlines are
backend responsibilities; deciding how those results affect viewer state is app
runtime responsibility.

## Design Principles

Prefer typed boundaries over callbacks or shared mutable access. Commands,
events, worker requests, worker completions, snapshots, and extension hooks make
cross-subsystem behavior visible and testable.

Keep mutable state where its invariants can be checked. Viewer state belongs to
the app boundary, feature-local state belongs to the feature owner, and cache
state belongs to the subsystem that defines the cache key and stale-result
rules.

Keep inventories in code. Command lists, keymaps, config fields, palette
kinds, extension composition, cache details, and backend-specific behavior
should be read from the Rust definitions and protected by tests where
consistency matters. Architecture docs explain why a boundary exists; they do
not duplicate complete lists.

When behavior changes, update the owning code and focused tests first. Update
this document only when the system shape, ownership model, or boundary rationale
changes.
