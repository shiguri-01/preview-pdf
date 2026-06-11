# Rendering Pipeline Specification

This document is the source of truth for how the runtime transforms PDF page
content into terminal image output.

## Scope

This document owns:

- rasterization contracts
- L1 and L2 cache semantics
- viewport crop and spread slot behavior
- render scheduling and worker priority rules
- presenter encode behavior
- redraw timing rules
- exported performance signals

## End-to-end flow

1. Open the PDF backend document.
2. Rasterize the visible page or pages into `RgbaFrame`.
3. Store raster output in the L1 rendered-page cache.
4. Apply current-view highlight overlays without mutating cached raster output.
5. Crop each visible page to its display slot when needed.
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

Render workers may create a backend-specific render context before processing
tasks. The hayro backend uses this context to keep hayro's document-scoped
`RenderCache` local to each worker thread while preserving the shared
`PdfBackend` contract.

Search and overlay extraction use a separate text-page path that exposes glyph
rectangles in page coordinates.

Rules:

- raster output uses RGBA pixel storage
- cache identity includes document identity, page identity, scale, and an
  optional layout identity
- document identity changes when the opened path's PDF bytes change, even if
  the replacement file has the same byte length
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
`layout_tag` is reserved for layout-sensitive identities. Presenter drawing uses
source-page identities for both single-page and spread slots.

## Viewport and spread slots

Rules:

- spread mode splits the viewer into left and right page slots with a fixed gap
- when spread cover policy is `cover`, page 1 is presented through the
  single-page path; later spreads use the normal left/right slot path
- each slot draws the same source-page frame path used by single-page mode
- a missing partner page is represented by clearing that slot
- each page is independently fit within its slot, so differently sized pages do
  not force each other to the same displayed size
- if zoomed content exceeds the viewport, only the visible region is forwarded
- crop is cell-aligned
- if the full frame already fits, the uncropped frame is forwarded
- viewer state stores the requested pan from user commands; crop preparation
  derives a separate effective pan clamped to the currently available image and
  viewport bounds
- zoomed spread-canvas crops still preserve left/right slot identity; a page
  outside the viewport is represented as an inactive slot rather than removing
  that slot from the presenter input

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
  prepares each page in its own presenter slot
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

`viewport`, effective `pan`, and `overlay_stamp` are part of the L2 key because
terminal output depends on all three. The requested pan stored in viewer state
is not rewritten by cache preparation.

## Presenter draw contract

`ImagePresenter::render(...) -> AppResult<PresenterRenderOutcome>` draws a
single slot. `ImagePresenter::render_slots(...)` draws one or more slots and
returns both aggregate outcome fields and per-slot `PresenterSlotOutcome`
entries. Both must follow these rules:

- `drew_image = true` means terminal image content is visible for the frame
- `feedback` indicates whether the current image is ready, pending, or failed
- `used_stale_fallback = true` means an older ready frame was drawn
- `slots` contains the display area and the same state for each rendered slot;
  inactive slots are returned with `active = false` and do not participate in
  aggregate feedback

Frame-level UI rules:

- a frame must not end in a clear-only state
- each frame must show either image content, a loading overlay, or an error
  overlay
- in single-page mode, if the current page is pending, the loading overlay is
  drawn over the viewer, including when a stale fallback or preview image is
  visible
- in spread mode, viewer-level loading is not used; each active pending page
  slot draws loading inside its own image area from the first pending frame,
  labeled with that slot's page using the same `p.N` notation as the rest of
  the app
- the presenter tracks the last terminal key and render area for each slot;
  when a later draw targets the same key and area, it preserves the existing
  terminal image with skipped buffer cells instead of re-emitting image
  protocol data
- when an app overlay has covered the image and then closes, the next frame
  forces an image redraw so the covered image cells are restored

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

These values feed the developer-only performance diagnostics JSON emitted by
`cargo bench --bench perf`; the public viewer CLI does not expose a performance
subcommand.

## Code references

- `src/backend/traits.rs`
- `src/render/cache.rs`
- `src/render/scheduler.rs`
- `src/render/prefetch.rs`
- `src/render/worker.rs`
- `src/presenter/encode.rs`
- `src/presenter/l2_cache.rs`
- `src/presenter/ratatui/`
