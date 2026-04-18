# Rendering Pipeline Specification

This document is the source of truth for how the runtime transforms PDF page
content into terminal image output.

## Scope

This document owns:

- rasterization contracts
- L1 and L2 cache semantics
- viewport crop and spread composition behavior
- render scheduling and worker priority rules
- presenter encode behavior
- redraw timing rules
- exported performance signals

## End-to-end flow

1. Open the PDF backend document.
2. Rasterize the visible page or pages into `RgbaFrame`.
3. Store raster output in the L1 rendered-page cache.
4. Apply current-view highlight overlays without mutating cached raster output.
5. Compose spread output and crop to the current viewport when needed.
6. Prepare terminal-frame entries in the L2 cache.
7. Encode the image for the active terminal protocol.
8. Draw a ready terminal frame through the presenter.

Cold start may additionally render a lower-resolution preview for the current
view before the full-resolution image is ready.

## Rasterization contract

Primary backend API:

```rust
fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame>
```

Search and overlay extraction use a separate text-page path that exposes glyph
rectangles in page coordinates.

Rules:

- raster output uses RGBA pixel storage
- cache identity includes document identity, page identity, scale, and layout
  identity
- render-worker page tasks use layout tag `0` for source-page identity
- text extraction is a separate backend path used by search
- raw render cache entries do not include highlight overlays

## L1 rendered-page cache

- key: `RenderedPageKey { doc_id, page, scale_milli, layout_tag }`
- value: `RgbaFrame`
- policy: LRU with memory-budget enforcement on insert, except that the current
  critical render may temporarily occupy a single oversize entry
- counters track hit, miss, and eviction behavior

`scale_milli` stores scale in integer milli-units for exact key equality.
`layout_tag` keeps single-page and spread presenter identities distinct.

## Viewport and spread composition

Rules:

- spread mode horizontally composes the left and right page with a fixed gap
- a missing partner page is represented as a blank slot
- if zoomed content exceeds the viewport, only the visible region is forwarded
- crop is cell-aligned
- if the full frame already fits, the uncropped frame is forwarded

## Render workers and scheduling

- render work runs in a fixed-size worker pool
- completed render work returns as `RenderWorkerResult`
- in-flight work is capacity-limited
- task priority uses shared `WorkClass`

Priority order:

1. `CriticalCurrent`
2. `GuardReverse`
3. `DirectionalLead`
4. `Background`

Scheduling rules:

- always include the currently visible page or pages
- include one reverse-guard page
- include directional lead pages based on navigation streak
- use remaining budget for background prefetch work
- stale lower-priority work may be canceled to admit critical current work

## Cold-start preview behavior

- while no page image has been displayed yet, the runtime may enqueue both
  preview-scale work and full-resolution current-page work
- in spread layout, preview rendering rasterizes each visible source page and
  then composes a temporary spread image
- the preview remains visible until the full-resolution current view is ready

## Presenter encode and L2 cache

Overlay rules:

- highlight overlays are source-agnostic decorations applied after raw raster
  retrieval and before presenter preparation
- search is currently one overlay producer, but the pipeline does not depend on
  search-specific types
- search-hit rectangle merging uses glyph geometry in page coordinates; the
  inferred merge axis is a screen-space layout heuristic, not a writing-mode classification
- overlay differences do not affect L1 rendered-page cache identity

Encode input:

```rust
EncodeWorkerRequest::Encode { key, picker, frame, area, class, generation }
```

Encode rules:

- `CriticalCurrent` tasks use the current encode lane
- lower-priority work uses the background lane
- stale generations are dropped aggressively for current work
- background stale-generation work is discarded unless the work class is
  explicitly preserved
- completed `CriticalCurrent` render work is downgraded to `DirectionalLead`
  before presenter-side prefetch encode routing

L2 cache rules:

- key: `TerminalFrameKey { rendered_page, viewport, pan, overlay_stamp }`
- states:
  - `PendingFrame`
  - `Encoding`
  - `Ready(StatefulProtocol)`
  - `Failed`
- policy: LRU with memory-budget enforcement, except that an oversize current
  entry may temporarily coexist with the currently visible ready frame to avoid
  regressing to a blank viewer

`viewport`, `pan`, and `overlay_stamp` are part of the L2 key because terminal
output depends on all three.

## Presenter draw contract

`ImagePresenter::render(...) -> AppResult<PresenterRenderOutcome>` must follow
these rules:

- `drew_image = true` means a terminal image was drawn
- `feedback` indicates whether the current image is ready, pending, or failed
- `used_stale_fallback = true` means an older ready frame was drawn

Frame-level UI rules:

- a frame must not end in a clear-only state
- each frame must show either image content, a loading overlay, or an error
  overlay
- if the current page is pending and an older image is visible, the loading
  overlay is drawn over that image

## Redraw timing

- `RedrawTick` is enabled only while the current view is not fully cached and
  render or presenter work is still pending
- queued prefetch work keeps the loop on the busy wake timeout even after the
  current view is cached
- once the current view is cached and presenter work is drained, redraws are
  event-driven rather than timer-driven

## Observable performance signals

The runtime tracks:

- `render_ms`
- `encode_ms` and the reported `convert_ms`
- `blit_ms`
- `render_queue_wait_ms`
- `encode_queue_wait_ms`
- L1 and L2 cache hit rates
- render and encode queue depth and in-flight samples
- canceled render and encode task counts
- redraw request counts by reason

These values feed perf JSON reports and offline diagnostics.

## Code references

- `src/backend/traits.rs`
- `src/render/cache.rs`
- `src/render/scheduler.rs`
- `src/render/prefetch.rs`
- `src/render/worker.rs`
- `src/presenter/encode.rs`
- `src/presenter/l2_cache.rs`
- `src/presenter/ratatui.rs`
