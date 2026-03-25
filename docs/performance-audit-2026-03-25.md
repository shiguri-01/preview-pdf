# Performance Audit Notes (2026-03-25)

This note captures concrete performance improvement candidates found during a code read of the current `pvf` runtime.

## Priority findings

### 1. Encode completion is surfaced by polling instead of an immediate loop event

- The main loop waits on input, render completion, prefetch tick, redraw tick, and wake timeout.
- Encode completion is not part of the loop's wait set.
- Presenter-side encode results are only drained when the presenter is touched again from the draw path or background-drain path.

Relevant code:

- `src/app/event_loop.rs`
- `src/app/actors.rs`
- `src/presenter/ratatui.rs`

Impact:

- When L1 already has the current frame but L2 encode is still running, the encode completion may not redraw immediately.
- This can show up as a visible delay between "render finished" and "image actually appears".

Suggested direction:

- Promote encode completion into a `DomainEvent`.
- Request redraw immediately when the current presenter key finishes encoding.
- Reduce dependence on `wake_timeout` and timer-based redraws for L2 completion.

### 2. Encode work is single-threaded and can delay current-page responsiveness

- Only one encode worker is spawned.
- The encode queue is priority-aware, but an already running prefetch encode cannot be interrupted by a new current-page encode.

Relevant code:

- `src/presenter/encode.rs`

Impact:

- Fast page flips can still stall on an in-flight prefetch encode.
- User-visible responsiveness is limited by one encode lane even when render workers are parallel.

Suggested direction:

- Split current-page encode from background/prefetch encode, or
- Allow more than one encode worker and preserve strict priority for current work.

### 3. Shared RGBA frames can be fully cloned before resize/encode

- Non-crop prepare paths keep `RgbaFrame` shared by cloning the `Arc`.
- L2 cache also keeps a shared `PendingFrame`.
- The resize path calls `PixelBuffer::with_mut_bytes(self, ...)`.
- If the pixel buffer is shared, `with_mut_bytes` clones the full pixel buffer before resize.

Relevant code:

- `src/app/frame_ops.rs`
- `src/presenter/ratatui.rs`
- `src/presenter/image_ops.rs`
- `src/backend/traits.rs`

Impact:

- Large page images can pay for an extra full-buffer copy before downscale.
- This directly affects "time to first visible image" and page-flip latency.

Suggested direction:

- Add a resize path that accepts immutable source bytes.
- Avoid copy-on-write cloning for read-only resize operations.
- Revisit L2 ownership so encode can consume unique buffers more often.

### 4. Backend render output copies the full pixmap into a new `Vec<u8>`

- Hayro render output is converted with `data_as_u8_slice().to_vec()`.

Relevant code:

- `src/backend/hayro.rs`

Impact:

- Every rasterized page pays for one full-frame copy before entering caches.
- This is especially noticeable on large pages and cold start.

Suggested direction:

- If the backend API allows it, move or borrow the render buffer directly into `RgbaFrame`.
- Otherwise, investigate a backend-local pooled buffer strategy.

### 5. Startup blocks on synchronous full-file PDF read

- `PdfDoc::load_shared_bytes` uses `std::fs::read(path)?` to load the entire document into memory.
- For large PDFs (e.g., 500MB+), this causes a multi-second block of the main thread during initialization.

Relevant code:

- `src/backend/hayro.rs`

Impact:

- Poor "time to first frame" and "app responsiveness" during cold start with large documents.

Suggested direction:

- Use `memmap2` for memory-mapped I/O to allow the OS to handle demand-paging of the PDF data.
- Alternatively, move the document load to a background task or use asynchronous I/O if the parser supports it.

### 6. RenderWorker threads block on a shared Mutex during task reception

- The `RenderWorker` spawns multiple threads, but they share a single `tokio::sync::mpsc::UnboundedReceiver` wrapped in an `Arc<Mutex<...>>`.
- When a worker calls `request_rx.lock()?.blocking_recv()`, it holds the Mutex *while* waiting (sleeping) for a new task.

Status:

- Fixed on 2026-03-25.
- `src/render/worker.rs` now uses a true MPMC request channel (`flume`) so each render worker blocks on its own cloned receiver instead of serializing task reception behind a shared Mutex.

Relevant code:

- `src/render/worker.rs`

Impact:

- Severe lock contention. If one thread is waiting for work, the Mutex is locked. Other threads finishing their work cannot even check the queue, defeating the purpose of multi-threading.
- Causes unpredictable stutters and poor CPU utilization during heavy rendering or page flips.

Suggested direction:

- Replace the `Arc<Mutex<mpsc::Receiver>>` with a true MPMC channel (e.g., `crossbeam-channel` or `flume`).
- Alternatively, assign a dedicated `mpsc` receiver to each worker thread and route tasks from a central dispatcher.

### 7. PrefetchQueue uses an O(N log N) algorithm for task cancellation

- `PrefetchQueue::retain` pops every single item from the `BinaryHeap`, filters them, and then pushes the retained items back one by one.
- Since `push` is `O(log N)`, the total operation is `O(N log N)`.

Status:

- Fixed on 2026-03-25.
- `src/render/prefetch.rs` now filters the heap via `BinaryHeap::into_vec()` + `Vec::retain()` and rebuilds it with `BinaryHeap::from()`, avoiding repeated `push()` calls during cancellation-heavy paths.

Relevant code:

- `src/render/prefetch.rs`

Impact:

- Cancellation is triggered frequently (e.g., on every page flip and encode enqueue).
- As the prefetch queue grows, this blocks the main thread, directly degrading the responsiveness of user input (like fast scrolling).

Suggested direction:

- Convert the `BinaryHeap` into a `Vec` using `into_vec()`, filter it in-place using `Vec::retain` (which is `O(N)`), and then rebuild the heap using `BinaryHeap::from()` (which is also `O(N)`).

## Secondary observations

### Busy polling remains active while work is pending

- Default busy wake timeout is `8ms`.
- Default prefetch tick is `8ms`.
- Default pending redraw interval is `33ms`.

Relevant code:

- `src/config.rs`

Impact:

- This may increase idle CPU usage while work is pending.
- It is likely not the primary bottleneck compared with render/encode copies and event delivery.

Suggested direction:

- Revisit these defaults after encode completion becomes event-driven.
- Prefer event delivery over periodic wakeups where possible.

## Recommended implementation order

1. Fix RenderWorker Mutex lock contention. (Critical bug)
2. Fix PrefetchQueue O(N log N) cancellation. (Quick win for responsiveness)
3. Make encode completion event-driven.
4. Reduce or isolate encode contention for current-page work.
5. Remove avoidable full-frame clones in resize/encode paths.
6. Investigate zero-copy or lower-copy backend frame handoff.
7. Optimize startup document loading with memory mapping or async I/O.

## Verification status

- Codebase inspection completed.
- `cargo check` completed successfully on 2026-03-25.
