---
name: bench
description: Run or compare pvf headless benchmarks and interpret performance reports.
---

# Bench

Use `benches/perf.rs` as the entry point, `benches/fixtures/` as the fixture
source, and `target/bench/` for generated fixtures and reports.

Choose the smallest run that answers the measurement question. Run benchmarks
sequentially to avoid contention; keep variables unrelated to the comparison
axis stable.

When changing benchmark behavior, report shape, or scenario metadata, consult
Performance Diagnostics in `docs/reference.md`. For the boundary between
performance diagnostics and correctness tests, use `docs/testing.md`.
A rerun needs only the relevant command, fixture, and report context.

Report the metrics relevant to the question and enough context to reproduce
them: command, fixture, scenarios, warmup, iterations, output path, and
comparison axis.
