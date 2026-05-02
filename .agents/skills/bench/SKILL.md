---
name: bench
description: Run and interpret pvf headless performance diagnostics. Use for generating benchmark PDF fixtures, running `cargo bench --bench perf`, writing JSON reports, comparing results across PDFs or code changes, and explaining scenario metrics or regressions.
---

# Bench

Use this skill to run, compare, or explain repo-local performance diagnostics.

Before running or changing benchmarks, read `docs/performance-diagnostics.md`.
Use `benches/perf.rs` as the bench entry point, `benches/fixtures/` as the fixture source, and `target/bench/` for generated fixtures and reports.

## Agent Workflow

Infer the measurement goal from the conversation, then choose the smallest run that answers it.
Run benchmark commands sequentially, not in parallel.
For comparisons, keep variables unrelated to the comparison axis stable.
Report only the metrics that explain the user's question, and include enough context to reproduce the result: command, fixture, scenarios, warmup, iterations, output path, and comparison axis.
