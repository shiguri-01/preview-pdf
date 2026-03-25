# pvf Rendering Pipeline Specification

This document defines how `pvf` transforms PDF page content into terminal pixels.

## End-to-end contract

Pipeline:

1. Open PDF backend document.
2. Rasterize visible page(s) into `RgbaFrame`.
   - On cold start, the visible page or spread may also be rasterized at a lower preview scale so the viewer can present a coarse image before the full-resolution content is ready.
3. Cache rasterized frame in L1 cache.
4. Compose spread frame (when enabled), then crop to viewport/pan window when required.
5. Prepare terminal frame entry in L2 cache.
6. Encode image for terminal protocol in encode worker lanes.
7. Render ready protocol frame through ratatui draw path.

## Stage contracts

## 1. PDF rasterization (`backend/traits.rs`, `backend/hayro.rs`)

Primary API:

```rust
fn render_page(&mut self, page: u32, scale: f64) -> AppResult<RgbaFrame>
```

Requirements:
- Output pixel format is RGBA (4 bytes/pixel).
- `RgbaFrame` pixel storage is shareable across caches and presenter handoff, but encode/downscale paths should consume the owned buffer without cloning when the frame is uniquely owned.
- Cache identity includes `doc_id` (stable hash of file path), page index, scale, and layout tag.
- Render-worker page tasks use layout tag `0` (source page identity).

Text extraction (`extract_text`) provides line-oriented text data for search.

## 2. L1 rendered-page cache (`render/cache.rs`)

- Cache key: `RenderedPageKey { doc_id, page, scale_milli, layout_tag }`.
- Cache value: `RgbaFrame`.
- Cache policy: LRU + memory budget enforcement on insert.
- Cache counters are tracked for hit/miss/eviction reporting.

`scale_milli` stores scale as integer milli-units for exact key equality.
`layout_tag` separates single-page and spread presenter identities to prevent L2 key collisions.

## 3. Viewport crop/pan (`app/frame_ops.rs`)

APIs:

```rust
fn crop_frame_for_viewport(frame, viewport, pan, cell_px) -> RgbaFrame
fn compose_spread_frame(left, right, gap_px) -> RgbaFrame
```

Requirements:
- In spread mode, two page frames are horizontally composed with a fixed gap.
- Missing partner page (odd tail page) is represented as a blank slot.
- When zoomed content exceeds viewport, only the visible region is forwarded.
- Crop is cell-aligned to avoid sub-cell artifacts.
- If full frame fits viewport, original frame is forwarded.

## 4. Render workers (`render/worker.rs`)

- A fixed-size worker pool executes `RenderTask`.
- Completed work is emitted as `RenderResultEvent` through worker result channel.
- Capacity limits bound concurrent in-flight render tasks.
- `RenderTask.class` uses shared `WorkClass`.

Preemption rule for current-page critical tasks:
- On saturation, stale lower-priority tasks can be canceled to admit critical work.
- Priority order:
  - `CriticalCurrent`
  - `GuardReverse`
  - `DirectionalLead`
  - `Background`

Cold-start preview rule:
- While no page image has been displayed yet, the runtime may enqueue both:
  - lower-resolution preview render(s) for the currently visible page(s), and
  - the normal full-resolution current-page render(s).
- In spread layout, the preview path renders each visible source page at preview scale, then composes them into a temporary spread image with the same layout tag as the eventual full-resolution spread.
- The preview is only a temporary display path; pending redraws continue until the full-resolution current page is cached and presented.

## 5. Scheduling and prefetch (`render/scheduler.rs`, `render/prefetch.rs`)

`NavTracker` produces `NavIntent { direction, streak, generation }`.

Scheduler requirements:
- Always include current visible page(s) (`CriticalCurrent` for each).
- Include one reverse guard page (`GuardReverse`).
- Include directional lead pages (`DirectionalLead`) with depth based on streak.
- Fill remaining budget with `Background` tasks.

Queue requirements (`PrefetchQueue`):
- Priority ordering.
- Key deduplication with priority replacement.
- Stale-generation cancellation.
- Max depth enforcement.
- Queue class metadata uses shared `WorkClass`.

## 6. Encode worker lanes (`presenter/encode.rs`)

Input:
- `EncodeWorkerRequest::Encode { key, picker, frame, area, class, generation }`

Per-task behavior:
1. Resize/crop frame for target terminal area.
2. Encode to `StatefulProtocol` with negotiated protocol picker.

Requirements:
- `CriticalCurrent` tasks use the current lane.
- `GuardReverse`, `DirectionalLead`, and `Background` tasks use the background lane.
- Current-lane queued work drops older generations so fast page flips do not build a stale current backlog.
- Background-lane stale-generation tasks are discarded before encode completion path, except for `GuardReverse` and any other work class preserved by `WorkClass::kept_on_background_stale_generation()`.
- Render-complete handoff keeps one explicit exception: completed `CriticalCurrent` render work is downgraded to `DirectionalLead` before presenter-side prefetch encode routing.
- Resize/downscale reads source RGBA bytes through an immutable image view, so shared `RgbaFrame` storage does not trigger copy-on-write cloning before the destination buffer is produced.
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

`ImagePresenter::render(...) -> AppResult<PresenterRenderOutcome>` contract:
- `drew_image=true` means a terminal frame was drawn (current or stale fallback).
- `feedback` conveys overlay intent:
  - `None`: current image is ready.
  - `Pending`: current image is still preparing/encoding.
  - `Failed`: current image failed to prepare/encode.
- `used_stale_fallback=true` means the presenter drew a previous ready frame.

Protocol handling:
- Presenter resolves terminal image protocol through picker/capability flow.
- Draw path renders protocol image into ratatui frame region.

UI contract:
- A frame must never end with "Clear only".
- Viewer output for each frame must include one of:
  - image content,
  - loading overlay,
  - error overlay.
- When the current page is pending and an older image is still visible, the loading overlay is drawn on top of that existing image instead of suppressing it.

## Pending redraw timer behavior

- `RedrawTick` is not a permanently active idle timer.
- The loop enables pending redraw ticks only while the current view is not fully cached and either:
  - render workers still have in-flight work, or
  - the presenter still has encode/presenter-side pending work.
- Prefetch work stays wake-sensitive even after the current view is cached:
  - queued prefetch tasks keep the loop on the busy wake timeout.
- Once the current view is cached and presenter work is drained, redraws must be event-driven (`input`, `command`, `app_event`, `render_complete`, `state_changed`) rather than timer-driven.

## Observable performance signals (`perf.rs`)

The runtime tracks:
- render time (`render_ms`)
- encode time (`encode_ms`, surfaced as `convert_ms` in perf reports)
- terminal draw time (`blit_ms`)
- render/encode queue wait time (`render_queue_wait_ms`, `encode_queue_wait_ms`)
- L1/L2 cache hit rates
- render/encode queue depth and in-flight samples
- presenter encode queue metrics are aggregated across the current and background lanes
- render/encode canceled task counts
- redraw request counts broken down by reason (`input`, `command`, `app_event`, `render_complete`, `pending_work`, `timer`, `input_error`, `state_changed`)

These values are used by perf JSON reports and other offline/debugging analysis. The interactive diagnostics bar is intentionally narrower and only shows the active presenter/protocol path.
