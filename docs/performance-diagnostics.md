# Performance Diagnostics

This document owns developer-facing performance measurement. The public CLI is
only `pvf <file.pdf>`; diagnostics run through Cargo's bench entry point.

## Command

```bash
cargo bench --bench perf -- --pdf sample.pdf
```

Options:

- `--pdf <path>`: required input PDF.
- `--scenario <id>`: optional and repeatable. Defaults to all scenarios. `all`
  selects all scenarios explicitly.
- `--warmup <n>`: warmup iterations, default `1`.
- `--iterations <n>`: measured iterations, default `5`.
- `--page-steps <n>`: page navigation steps for page scenarios, default `8`.
- `--idle-ms <n>`: idle observation window for `idle-settled-redraw`, default
  `250`.
- `--out <path>`: optional JSON output path. When omitted, JSON is written to
  stdout.

The bench binary uses `harness = false` and does not use Criterion. It runs the
same headless app event loop, render worker, presenter encode path, cache path,
blit path, and redraw scheduling used by the viewer.

## Scenarios

- `cold-first-page`: PDF open through first settled idle display.
- `steady-next-page`: after each settled idle state, send the next-page command
  until `--page-steps` or the last page is reached.
- `steady-prev-page`: move to the last page side, then after each settled idle
  state send previous-page until `--page-steps` or page zero is reached.
- `rapid-next-page`: after first settled idle, enqueue multiple next-page
  commands without waiting between them and measure until settled.
- `zoom-step`: after first settled idle, measure zoom-in and zoom-out redraw.
- `idle-settled-redraw`: after first settled idle, observe the idle window and
  report redraw activity.

Search performance is intentionally excluded from v1 diagnostics.

## JSON Report

The report is a single JSON document with no separate human summary.

Top-level fields:

- `version`
- `generated_at_unix_ms`
- `pdf`: includes `path` and `doc_id`
- `run`: includes warmup, measured iteration, page-step, and idle settings
- `scenarios`

Each scenario includes:

- `id`
- `parameters`
- `aggregate`
- `iterations`

Aggregates use count, average, min, p50, p95, p99, and max summaries. Each
iteration includes wall-clock duration, render/encode/blit phase metrics, queue
metrics, cache hit rates, redraw counts, final page, and visited step count.

## Code References

- `benches/perf.rs`
- `src/perf.rs`
- `src/app/perf_runner.rs`
- `src/app/event_loop.rs`
