use std::time::Duration;

use super::{
    CacheSummary, MetricSummary, PerfAggregateReport, PerfIterationReport, PerfStats,
    PhaseMetricsSummary, QueueSummary, RedrawSummary, ScalarSummary,
};

pub(super) fn merge_stats<'a>(stats: impl Iterator<Item = &'a PerfStats>) -> PerfStats {
    let mut merged = PerfStats::default();
    let mut stat_count = 0usize;
    for stat in stats {
        stat_count += 1;
        merged.render_ms = stat.render_ms;
        merged.convert_ms = stat.convert_ms;
        merged.blit_ms = stat.blit_ms;
        merged.cache_hit_rate_l1 += stat.cache_hit_rate_l1;
        merged.cache_hit_rate_l2 += stat.cache_hit_rate_l2;
        merged.queue_depth = stat.queue_depth;
        merged.render_in_flight = stat.render_in_flight;
        merged.encode_queue_depth = stat.encode_queue_depth;
        merged.encode_in_flight = stat.encode_in_flight;
        merged.canceled_tasks += stat.canceled_tasks;
        merged.render_canceled_tasks += stat.render_canceled_tasks;
        merged.encode_canceled_tasks += stat.encode_canceled_tasks;
        merged.render_samples += stat.render_samples;
        merged.convert_samples += stat.convert_samples;
        merged.blit_samples += stat.blit_samples;
        merged.redraw_requests_total += stat.redraw_requests_total;
        merged.redraw_by_reason.input += stat.redraw_by_reason.input;
        merged.redraw_by_reason.command += stat.redraw_by_reason.command;
        merged.redraw_by_reason.app_event += stat.redraw_by_reason.app_event;
        merged.redraw_by_reason.render_complete += stat.redraw_by_reason.render_complete;
        merged.redraw_by_reason.pending_work += stat.redraw_by_reason.pending_work;
        merged.redraw_by_reason.timer += stat.redraw_by_reason.timer;
        merged.redraw_by_reason.input_error += stat.redraw_by_reason.input_error;
        merged.redraw_by_reason.state_changed += stat.redraw_by_reason.state_changed;
        merged
            .render_samples_ms
            .extend_from_slice(stat.render_samples_ms());
        merged
            .encode_samples_ms
            .extend_from_slice(stat.encode_samples_ms());
        merged
            .blit_samples_ms
            .extend_from_slice(stat.blit_samples_ms());
        merged
            .render_queue_wait_samples_ms
            .extend_from_slice(stat.render_queue_wait_samples_ms());
        merged
            .encode_queue_wait_samples_ms
            .extend_from_slice(stat.encode_queue_wait_samples_ms());
        merged
            .render_queue_depth_samples
            .extend_from_slice(stat.render_queue_depth_samples());
        merged
            .render_in_flight_samples
            .extend_from_slice(stat.render_in_flight_samples());
        merged
            .encode_queue_depth_samples
            .extend_from_slice(stat.encode_queue_depth_samples());
        merged
            .encode_in_flight_samples
            .extend_from_slice(stat.encode_in_flight_samples());
    }
    if stat_count > 0 {
        merged.cache_hit_rate_l1 /= stat_count as f64;
        merged.cache_hit_rate_l2 /= stat_count as f64;
    }
    merged
}

pub(super) fn build_iteration_report(
    iteration_index: usize,
    wall_time: Duration,
    runtime: &PerfStats,
    presenter: &PerfStats,
    final_page: usize,
    visited_steps: usize,
) -> PerfIterationReport {
    let summary = build_metrics_report(runtime, presenter);
    PerfIterationReport {
        iteration_index,
        wall_time_ms: wall_time.as_secs_f64() * 1000.0,
        phase_metrics: summary.phase_metrics,
        redraw: summary.redraw,
        queues: summary.queues,
        cache: summary.cache,
        final_page,
        visited_steps,
    }
}

pub(super) fn build_aggregate_report(
    runtime: &PerfStats,
    presenter: &PerfStats,
    wall_samples_ms: &[f64],
) -> PerfAggregateReport {
    let summary = build_metrics_report(runtime, presenter);
    PerfAggregateReport {
        wall_time_ms: summarize_metric(wall_samples_ms),
        phase_metrics: summary.phase_metrics,
        redraw: summary.redraw,
        queues: summary.queues,
        cache: summary.cache,
    }
}

struct PerfMetricsReport {
    phase_metrics: PhaseMetricsSummary,
    redraw: RedrawSummary,
    queues: QueueSummary,
    cache: CacheSummary,
}

fn build_metrics_report(runtime: &PerfStats, presenter: &PerfStats) -> PerfMetricsReport {
    PerfMetricsReport {
        phase_metrics: PhaseMetricsSummary {
            render_ms: summarize_metric(runtime.render_samples_ms()),
            encode_ms: summarize_metric(presenter.encode_samples_ms()),
            blit_ms: summarize_metric(presenter.blit_samples_ms()),
            render_queue_wait_ms: summarize_metric(runtime.render_queue_wait_samples_ms()),
            encode_queue_wait_ms: summarize_metric(presenter.encode_queue_wait_samples_ms()),
        },
        redraw: RedrawSummary {
            total: runtime.redraw_requests_total,
            by_reason: runtime.redraw_by_reason.clone(),
            pending_work_redraw_ratio: ratio(
                runtime.redraw_by_reason.pending_work,
                runtime.redraw_requests_total,
            ),
        },
        queues: QueueSummary {
            render_depth: summarize_scalar(runtime.render_queue_depth_samples()),
            render_in_flight: summarize_scalar(runtime.render_in_flight_samples()),
            render_canceled_total: runtime.render_canceled_tasks,
            encode_depth: summarize_scalar(presenter.encode_queue_depth_samples()),
            encode_in_flight: summarize_scalar(presenter.encode_in_flight_samples()),
            encode_canceled_total: presenter.encode_canceled_tasks,
        },
        cache: CacheSummary {
            l1_hit_rate: runtime.cache_hit_rate_l1,
            l2_hit_rate: presenter.cache_hit_rate_l2,
        },
    }
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub(super) fn summarize_metric(samples: &[f64]) -> MetricSummary {
    let values = summarize_f64(samples);
    MetricSummary {
        count: values.count,
        avg_ms: values.avg,
        min_ms: values.min,
        p50_ms: values.p50,
        p95_ms: values.p95,
        p99_ms: values.p99,
        max_ms: values.max,
    }
}

pub(super) fn summarize_scalar(samples: &[usize]) -> ScalarSummary {
    let converted = samples
        .iter()
        .map(|sample| *sample as f64)
        .collect::<Vec<_>>();
    let values = summarize_f64(&converted);
    ScalarSummary {
        count: values.count,
        avg: values.avg,
        min: values.min,
        p50: values.p50,
        p95: values.p95,
        p99: values.p99,
        max: values.max,
    }
}

struct SummaryValues {
    count: usize,
    avg: f64,
    min: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

fn summarize_f64(samples: &[f64]) -> SummaryValues {
    if samples.is_empty() {
        return SummaryValues {
            count: 0,
            avg: 0.0,
            min: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
        };
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let count = sorted.len();
    let sum = sorted.iter().sum::<f64>();

    SummaryValues {
        count,
        avg: sum / count as f64,
        min: sorted[0],
        p50: percentile(&sorted, 0.50),
        p95: percentile(&sorted, 0.95),
        p99: percentile(&sorted, 0.99),
        max: sorted[count - 1],
    }
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}
