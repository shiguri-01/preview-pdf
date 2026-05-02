# Performance Diagnostics

Use the performance diagnostics bench to measure headless viewer startup,
navigation, zoom, redraw, render, encode, blit, queue, and cache behavior. The
public CLI remains `pvf <file.pdf>`; diagnostics run through Cargo's bench entry
point.

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

## Fixtures

Standard benchmark PDFs are generated from fixture definitions under `benches/fixtures/`.
Generated PDFs, image assets, and JSON reports go under `target/bench/`.

Generate the standard fixtures from the repository root:

```bash
mkdir -p target/bench/fixtures target/bench/assets
typst compile --input pages=10 benches/fixtures/text.typ target/bench/fixtures/text-10-pages.pdf
typst compile --input pages=1000 benches/fixtures/text.typ target/bench/fixtures/text-1000-pages.pdf
python3 benches/fixtures/generate-high-res-image.py target/bench/assets/high-res-bench.png
typst compile --root . --input pages=10 --input image=/target/bench/assets/high-res-bench.png benches/fixtures/high-res-image.typ target/bench/fixtures/high-res-image.pdf
```

Run the standard benchmark set and write JSON reports:

```bash
mkdir -p target/bench/reports
cargo bench --bench perf -- --pdf target/bench/fixtures/text-10-pages.pdf --warmup 1 --iterations 5 --out target/bench/reports/text-10-pages.json
cargo bench --bench perf -- --pdf target/bench/fixtures/text-1000-pages.pdf --warmup 1 --iterations 5 --out target/bench/reports/text-1000-pages.json
cargo bench --bench perf -- --pdf target/bench/fixtures/high-res-image.pdf --warmup 1 --iterations 5 --out target/bench/reports/high-res-image.json
```

Run benchmarks sequentially when comparing results. Parallel runs are acceptable
for smoke checks, but CPU contention can distort render and wall-clock timings.

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
- `benches/fixtures/`
- `src/perf.rs`
- `src/app/perf_runner.rs`
- `src/app/event_loop.rs`
