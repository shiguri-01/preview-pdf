# pvf Rendering Pipeline Specification

This document defines how `pvf` transforms a PDF page into terminal pixels.

## End-to-end contract

Pipeline:

1. Open PDF backend document.
2. Rasterize a page into `RgbaFrame`.
3. Cache rasterized frame in L1 cache.
4. Crop to viewport/pan window when required.
5. Prepare terminal frame entry in L2 cache.
6. Encode image for terminal protocol in encode worker.
7. Render ready protocol frame through ratatui draw path.

## Stage contracts

## 1. PDF rasterization (`backend/traits.rs`, `backend/hayro.rs`)

Primary API:

```rust
fn render_page(&mut self, page: u32, scale: f64) -> AppResult<RgbaFrame>
```

Requirements:
- Output pixel format is RGBA (4 bytes/pixel).
- Cache identity includes `doc_id` (stable hash of file path), page index, and scale.

Text extraction (`extract_text`) provides line-oriented text data for search.

## 2. L1 rendered-page cache (`render/cache.rs`)

- Cache key: `RenderedPageKey { doc_id, page, scale_milli }`.
- Cache value: `RgbaFrame`.
- Cache policy: LRU + memory budget enforcement on insert.
- Cache counters are tracked for hit/miss/eviction reporting.

`scale_milli` stores scale as integer milli-units for exact key equality.

## 3. Viewport crop/pan (`app/frame_ops.rs`)

API:

```rust
fn crop_frame_for_viewport(frame, viewport, pan, cell_px) -> RgbaFrame
```

Requirements:
- When zoomed content exceeds viewport, only the visible region is forwarded.
- Crop is cell-aligned to avoid sub-cell artifacts.
- If full frame fits viewport, original frame is forwarded.

## 4. Render workers (`render/worker.rs`)

- A fixed-size worker pool executes `RenderTask`.
- Completed work is emitted as `RenderResultEvent` through worker result channel.
- Capacity limits bound concurrent in-flight render tasks.

Preemption rule for current-page critical tasks:
- On saturation, stale lower-priority tasks can be canceled to admit critical work.
- Priority order:
  - `CriticalCurrent`
  - `GuardReverse`
  - `DirectionalLead`
  - `Background`

## 5. Scheduling and prefetch (`render/scheduler.rs`, `render/prefetch.rs`)

`NavTracker` produces `NavIntent { direction, streak, generation }`.

Scheduler requirements:
- Always include current page (`CriticalCurrent`).
- Include one reverse guard page (`GuardReverse`).
- Include directional lead pages (`DirectionalLead`) with depth based on streak.
- Fill remaining budget with `Background` tasks.

Queue requirements (`PrefetchQueue`):
- Priority ordering.
- Key deduplication with priority replacement.
- Stale-generation cancellation.
- Max depth enforcement.

## 6. Encode worker (`presenter/encode.rs`)

Input:
- `EncodeWorkerRequest::Encode { key, picker, frame, area, class, generation }`

Per-task behavior:
1. Resize/crop frame for target terminal area.
2. Encode to `StatefulProtocol` with negotiated protocol picker.

Requirements:
- Stale-generation tasks are discarded before encode completion path.
- Results are returned as `EncodeWorkerResult` and drained by presenter.

## 7. L2 terminal-frame cache (`presenter/l2_cache.rs`)

- Cache key: `TerminalFrameKey { rendered_page, viewport, pan }`.
- Value states:
  - `PendingFrame`
  - `Encoding`
  - `Ready(StatefulProtocol)`
  - `Failed`
- Cache policy: LRU + memory budget enforcement.

State transitions:

```text
insert -> PendingFrame -> Encoding -> Ready | Failed
```

`viewport` and `pan` are part of the key because terminal output depends on both.

## 8. Presenter draw path (`presenter/ratatui.rs`)

`ImagePresenter::render(...) -> AppResult<bool>` contract:
- Returns `true` when a ready terminal frame was drawn.
- Returns `false` when frame is still pending/encoding (caller may show loading UI).

Protocol handling:
- Presenter resolves terminal image protocol through picker/capability flow.
- Draw path renders protocol image into ratatui frame region.

## Observable performance signals (`perf.rs`)

The runtime tracks:
- render time (`render_ms`)
- encode time (`convert_ms`)
- terminal draw time (`blit_ms`)
- L1/L2 cache hit rates
- prefetch queue depth
- canceled task count

These values are used by status/debug surfaces.
