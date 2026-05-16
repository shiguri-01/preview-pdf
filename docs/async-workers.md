# Async Worker Lifecycle

This document lists the runtime workers that can outlive a single event-loop
iteration. Keep it updated when adding a new spawn point or changing cancellation
semantics.

## Workers

| Worker | Spawn point | Owner | Stop path | Cancellation model | Stale result handling |
| --- | --- | --- | --- | --- | --- |
| Terminal input task | `EventBusRuntime::start_input` in `src/app/event_bus.rs` | `LoopRuntime.loop_event_runtime` | `EventBusRuntime::shutdown` aborts task handles | No per-input cancellation; the task stops when the loop event channel closes or the runtime aborts it | Input events are delivered as `DomainEvent::Input`; input errors become `DomainEvent::InputError` |
| Render workers | `RenderWorker::spawn` in `src/render/worker.rs` | `LoopRuntime.render_worker` | `RenderWorker::drop` sends `Shutdown` to each worker and aborts handles | Current work can mark prefetch work as canceled; active PDF rendering is not interrupted mid-render | `RenderWorker::accept_result_event` drops canceled or superseded results using task id, key, class, and generation |
| Search worker | `SearchEngine::new` in `src/search/engine.rs` | `SearchRuntime` inside `ExtensionHost` | `SearchEngine::drop` sends `Shutdown` and aborts the worker handle | `SearchEngine::cancel` submits an empty query with a new generation; it does not interrupt an already running scan | Search state applies events by generation and ignores events that do not match the active query generation |
| Presenter encode workers | `spawn_encode_worker` in `src/presenter/encode.rs` | `RatatuiImagePresenter` current/background lanes | `RatatuiImagePresenter::shutdown_worker` sends `Shutdown` to each lane and aborts handles | Queued encode tasks are pruned by generation and work class; active encode work is not interrupted mid-encode | Encode events include lane, key, queue state, and canceled-stale notifications; presenter cache/generation checks decide whether the result is still useful |

## Event Propagation

- Terminal input uses the loop event channel directly.
- Render completion is polled by the event loop and wrapped as
  `DomainEvent::RenderComplete`.
- Presenter encode completion is polled by the event loop and wrapped as
  `DomainEvent::EncodeComplete`.
- Search events are drained by the search extension during background handling,
  not through `DomainEvent`.

## Conventions

- New workers should have one explicit owner and one shutdown path.
- Prefer generation-based stale-result checks when work cannot be interrupted.
- If cancellation only marks queued or in-flight metadata, document that active
  work may still run to completion.
- Event payloads should carry enough identity to decide whether they still apply
  at the receiver boundary.
