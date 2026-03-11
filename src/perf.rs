use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::app::App;
use crate::backend::open_default_backend;
use crate::error::{AppError, AppResult};
use crate::presenter::PresenterKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedrawReason {
    Input,
    Command,
    AppEvent,
    RenderComplete,
    PendingWork,
    Timer,
    InputError,
    StateChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct RedrawReasonCounts {
    pub input: u64,
    pub command: u64,
    pub app_event: u64,
    pub render_complete: u64,
    pub pending_work: u64,
    pub timer: u64,
    pub input_error: u64,
    pub state_changed: u64,
}

impl RedrawReasonCounts {
    fn record(&mut self, reason: RedrawReason) {
        match reason {
            RedrawReason::Input => self.input += 1,
            RedrawReason::Command => self.command += 1,
            RedrawReason::AppEvent => self.app_event += 1,
            RedrawReason::RenderComplete => self.render_complete += 1,
            RedrawReason::PendingWork => self.pending_work += 1,
            RedrawReason::Timer => self.timer += 1,
            RedrawReason::InputError => self.input_error += 1,
            RedrawReason::StateChanged => self.state_changed += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerfStats {
    pub render_ms: f64,
    pub convert_ms: f64,
    pub blit_ms: f64,
    pub cache_hit_rate_l1: f64,
    pub cache_hit_rate_l2: f64,
    pub queue_depth: usize,
    pub canceled_tasks: usize,
    pub render_samples: u64,
    pub convert_samples: u64,
    pub blit_samples: u64,
    pub render_in_flight: usize,
    pub encode_queue_depth: usize,
    pub encode_in_flight: usize,
    pub render_canceled_tasks: usize,
    pub encode_canceled_tasks: usize,
    pub redraw_requests_total: u64,
    pub redraw_by_reason: RedrawReasonCounts,
    collect_samples: bool,
    render_samples_ms: Vec<f64>,
    encode_samples_ms: Vec<f64>,
    blit_samples_ms: Vec<f64>,
    render_queue_wait_samples_ms: Vec<f64>,
    encode_queue_wait_samples_ms: Vec<f64>,
    render_queue_depth_samples: Vec<usize>,
    render_in_flight_samples: Vec<usize>,
    encode_queue_depth_samples: Vec<usize>,
    encode_in_flight_samples: Vec<usize>,
}

impl PerfStats {
    pub fn enable_sample_collection(&mut self) {
        self.collect_samples = true;
    }

    pub fn record_render(&mut self, elapsed: Duration) {
        self.render_ms = elapsed.as_secs_f64() * 1000.0;
        self.render_samples += 1;
        if self.collect_samples {
            self.render_samples_ms.push(self.render_ms);
        }
    }

    pub fn record_convert(&mut self, elapsed: Duration) {
        self.convert_ms = elapsed.as_secs_f64() * 1000.0;
        self.convert_samples += 1;
        if self.collect_samples {
            self.encode_samples_ms.push(self.convert_ms);
        }
    }

    pub fn record_blit(&mut self, elapsed: Duration) {
        self.blit_ms = elapsed.as_secs_f64() * 1000.0;
        self.blit_samples += 1;
        if self.collect_samples {
            self.blit_samples_ms.push(self.blit_ms);
        }
    }

    pub fn record_render_queue_wait(&mut self, elapsed: Duration) {
        if self.collect_samples {
            self.render_queue_wait_samples_ms
                .push(elapsed.as_secs_f64() * 1000.0);
        }
    }

    pub fn record_encode_queue_wait(&mut self, elapsed: Duration) {
        if self.collect_samples {
            self.encode_queue_wait_samples_ms
                .push(elapsed.as_secs_f64() * 1000.0);
        }
    }

    pub fn set_l1_hit_rate(&mut self, rate: f64) {
        self.cache_hit_rate_l1 = rate.clamp(0.0, 1.0);
    }

    pub fn set_l2_hit_rate(&mut self, rate: f64) {
        self.cache_hit_rate_l2 = rate.clamp(0.0, 1.0);
    }

    pub fn set_queue_depth(&mut self, depth: usize) {
        self.queue_depth = depth;
        if self.collect_samples {
            self.render_queue_depth_samples.push(depth);
        }
    }

    pub fn set_render_in_flight(&mut self, inflight: usize) {
        self.render_in_flight = inflight;
        if self.collect_samples {
            self.render_in_flight_samples.push(inflight);
        }
    }

    pub fn set_encode_queue_depth(&mut self, depth: usize) {
        self.encode_queue_depth = depth;
        if self.collect_samples {
            self.encode_queue_depth_samples.push(depth);
        }
    }

    pub fn set_encode_in_flight(&mut self, inflight: usize) {
        self.encode_in_flight = inflight;
        if self.collect_samples {
            self.encode_in_flight_samples.push(inflight);
        }
    }

    pub fn add_canceled_tasks(&mut self, canceled: usize) {
        self.canceled_tasks += canceled;
        self.render_canceled_tasks += canceled;
    }

    pub fn add_encode_canceled_tasks(&mut self, canceled: usize) {
        self.encode_canceled_tasks += canceled;
    }

    pub fn record_redraw(&mut self, reason: RedrawReason) {
        self.redraw_requests_total += 1;
        self.redraw_by_reason.record(reason);
    }

    pub fn absorb_presenter_metrics(&mut self, presenter: &PerfStats) {
        self.convert_ms = presenter.convert_ms;
        self.blit_ms = presenter.blit_ms;
        self.cache_hit_rate_l2 = presenter.cache_hit_rate_l2;
        self.convert_samples = presenter.convert_samples;
        self.blit_samples = presenter.blit_samples;
        self.encode_queue_depth = presenter.encode_queue_depth;
        self.encode_in_flight = presenter.encode_in_flight;
        self.encode_canceled_tasks = presenter.encode_canceled_tasks;
    }

    pub fn clear_blit_metrics(&mut self) {
        self.blit_ms = 0.0;
        self.blit_samples = 0;
        self.blit_samples_ms.clear();
    }

    fn render_samples_ms(&self) -> &[f64] {
        &self.render_samples_ms
    }

    fn encode_samples_ms(&self) -> &[f64] {
        &self.encode_samples_ms
    }

    fn blit_samples_ms(&self) -> &[f64] {
        &self.blit_samples_ms
    }

    fn render_queue_wait_samples_ms(&self) -> &[f64] {
        &self.render_queue_wait_samples_ms
    }

    fn encode_queue_wait_samples_ms(&self) -> &[f64] {
        &self.encode_queue_wait_samples_ms
    }

    fn render_queue_depth_samples(&self) -> &[usize] {
        &self.render_queue_depth_samples
    }

    fn render_in_flight_samples(&self) -> &[usize] {
        &self.render_in_flight_samples
    }

    fn encode_queue_depth_samples(&self) -> &[usize] {
        &self.encode_queue_depth_samples
    }

    fn encode_in_flight_samples(&self) -> &[usize] {
        &self.encode_in_flight_samples
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricSummary {
    pub count: usize,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScalarSummary {
    pub count: usize,
    pub avg: f64,
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseMetricsSummary {
    pub render_ms: MetricSummary,
    pub encode_ms: MetricSummary,
    pub blit_ms: MetricSummary,
    pub render_queue_wait_ms: MetricSummary,
    pub encode_queue_wait_ms: MetricSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RedrawSummary {
    pub total: u64,
    pub by_reason: RedrawReasonCounts,
    pub pending_work_redraw_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueueSummary {
    pub render_depth: ScalarSummary,
    pub render_in_flight: ScalarSummary,
    pub render_canceled_total: usize,
    pub encode_depth: ScalarSummary,
    pub encode_in_flight: ScalarSummary,
    pub encode_canceled_total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CacheSummary {
    pub l1_hit_rate: f64,
    pub l2_hit_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfIterationReport {
    pub iteration_index: usize,
    pub phase_metrics: PhaseMetricsSummary,
    pub redraw: RedrawSummary,
    pub queues: QueueSummary,
    pub cache: CacheSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfSummaryReport {
    pub phase_metrics: PhaseMetricsSummary,
    pub redraw: RedrawSummary,
    pub queues: QueueSummary,
    pub cache: CacheSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfScenarioParameters {
    pub page_flip_limit: usize,
    pub idle_duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PerfScenarioId {
    PageFlipForward,
    PageFlipBackward,
    IdlePendingRedraw,
}

impl PerfScenarioId {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "page-flip-forward" => Some(Self::PageFlipForward),
            "page-flip-backward" => Some(Self::PageFlipBackward),
            "idle-pending-redraw" => Some(Self::IdlePendingRedraw),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::PageFlipForward => "page-flip-forward",
            Self::PageFlipBackward => "page-flip-backward",
            Self::IdlePendingRedraw => "idle-pending-redraw",
        }
    }

    pub fn parameters(self) -> PerfScenarioParameters {
        match self {
            Self::PageFlipForward | Self::PageFlipBackward => PerfScenarioParameters {
                page_flip_limit: 8,
                idle_duration_ms: 0,
            },
            Self::IdlePendingRedraw => PerfScenarioParameters {
                page_flip_limit: 0,
                idle_duration_ms: 250,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfRunConfig {
    pub scenario: PerfScenarioId,
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
}

impl Default for PerfRunConfig {
    fn default() -> Self {
        Self {
            scenario: PerfScenarioId::PageFlipForward,
            warmup_iterations: 0,
            measured_iterations: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerfIterationSnapshot {
    pub runtime: PerfStats,
    pub presenter: PerfStats,
}

impl PerfIterationSnapshot {
    pub fn into_report(self, iteration_index: usize) -> PerfIterationReport {
        build_iteration_report(iteration_index, &self.runtime, &self.presenter)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfPdfInfo {
    pub path: String,
    pub doc_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfScenarioInfo {
    pub id: PerfScenarioId,
    pub parameters: PerfScenarioParameters,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerfRunInfo {
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfReport {
    pub version: u32,
    pub generated_at_unix_ms: u128,
    pub pdf: PerfPdfInfo,
    pub scenario: PerfScenarioInfo,
    pub run: PerfRunInfo,
    pub summary: PerfSummaryReport,
    pub iterations: Vec<PerfIterationReport>,
}

impl PerfReport {
    pub fn from_iterations(
        pdf_path: &Path,
        doc_id: u64,
        run: &PerfRunConfig,
        measured: Vec<PerfIterationSnapshot>,
    ) -> Self {
        let iterations = measured
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, snapshot)| snapshot.into_report(idx))
            .collect::<Vec<_>>();
        let summary_runtime = merge_stats(measured.iter().map(|snapshot| &snapshot.runtime));
        let summary_presenter = merge_stats(measured.iter().map(|snapshot| &snapshot.presenter));

        Self {
            version: 1,
            generated_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0),
            pdf: PerfPdfInfo {
                path: pdf_path.display().to_string(),
                doc_id,
            },
            scenario: PerfScenarioInfo {
                id: run.scenario,
                parameters: run.scenario.parameters(),
            },
            run: PerfRunInfo {
                warmup_iterations: run.warmup_iterations,
                measured_iterations: run.measured_iterations,
            },
            summary: build_summary_report(&summary_runtime, &summary_presenter),
            iterations,
        }
    }
}

pub async fn run_report(pdf_path: &Path, run: PerfRunConfig) -> AppResult<PerfReport> {
    validate_run_config(&run)?;
    let total_iterations = run.warmup_iterations + run.measured_iterations;
    let mut measured = Vec::with_capacity(run.measured_iterations);
    let mut doc_id = None;

    for iteration in 0..total_iterations {
        let mut pdf = open_default_backend(pdf_path)?;
        doc_id.get_or_insert(pdf.doc_id());
        let mut app = App::new(PresenterKind::RatatuiImage)?;
        let snapshot = app.run_perf(pdf.as_mut(), run.scenario).await?;
        if iteration >= run.warmup_iterations {
            measured.push(snapshot);
        }
    }

    let doc_id = doc_id.ok_or_else(|| AppError::unsupported("perf run did not open the PDF"))?;

    Ok(PerfReport::from_iterations(
        &PathBuf::from(pdf_path),
        doc_id,
        &run,
        measured,
    ))
}

fn validate_run_config(run: &PerfRunConfig) -> AppResult<()> {
    if run.measured_iterations == 0 {
        return Err(AppError::invalid_argument(
            "perf run requires at least one measured iteration",
        ));
    }
    Ok(())
}

pub fn write_report(report: &PerfReport, out: Option<&Path>) -> AppResult<()> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| AppError::unsupported(format!("failed to serialize perf report: {err}")))?;

    match out {
        Some(path) => fs::write(path, format!("{json}\n"))?,
        None => println!("{json}"),
    }
    Ok(())
}

fn merge_stats<'a>(stats: impl Iterator<Item = &'a PerfStats>) -> PerfStats {
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

fn build_iteration_report(
    iteration_index: usize,
    runtime: &PerfStats,
    presenter: &PerfStats,
) -> PerfIterationReport {
    let summary = build_summary_report(runtime, presenter);
    PerfIterationReport {
        iteration_index,
        phase_metrics: summary.phase_metrics,
        redraw: summary.redraw,
        queues: summary.queues,
        cache: summary.cache,
    }
}

fn build_summary_report(runtime: &PerfStats, presenter: &PerfStats) -> PerfSummaryReport {
    PerfSummaryReport {
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

fn summarize_metric(samples: &[f64]) -> MetricSummary {
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

fn summarize_scalar(samples: &[usize]) -> ScalarSummary {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        PerfRunConfig, PerfScenarioId, PerfStats, RedrawReason, merge_stats, summarize_metric,
        summarize_scalar, validate_run_config,
    };

    #[test]
    fn records_milliseconds_and_clamped_rates() {
        let mut stats = PerfStats::default();
        stats.enable_sample_collection();
        stats.record_render(Duration::from_millis(12));
        stats.record_convert(Duration::from_millis(3));
        stats.record_blit(Duration::from_millis(1));
        stats.record_render_queue_wait(Duration::from_millis(4));
        stats.record_encode_queue_wait(Duration::from_millis(2));
        stats.set_l1_hit_rate(1.5);
        stats.set_l2_hit_rate(-0.5);
        stats.set_queue_depth(7);
        stats.set_render_in_flight(2);
        stats.set_encode_queue_depth(3);
        stats.set_encode_in_flight(1);
        stats.add_canceled_tasks(2);
        stats.add_encode_canceled_tasks(1);
        stats.record_redraw(RedrawReason::PendingWork);
        stats.record_redraw(RedrawReason::Timer);

        assert_eq!(stats.render_ms, 12.0);
        assert_eq!(stats.convert_ms, 3.0);
        assert_eq!(stats.blit_ms, 1.0);
        assert_eq!(stats.cache_hit_rate_l1, 1.0);
        assert_eq!(stats.cache_hit_rate_l2, 0.0);
        assert_eq!(stats.queue_depth, 7);
        assert_eq!(stats.render_in_flight, 2);
        assert_eq!(stats.encode_queue_depth, 3);
        assert_eq!(stats.encode_in_flight, 1);
        assert_eq!(stats.canceled_tasks, 2);
        assert_eq!(stats.render_canceled_tasks, 2);
        assert_eq!(stats.encode_canceled_tasks, 1);
        assert_eq!(stats.redraw_requests_total, 2);
        assert_eq!(stats.redraw_by_reason.pending_work, 1);
        assert_eq!(stats.redraw_by_reason.timer, 1);
    }

    #[test]
    fn absorbs_presenter_metrics_without_overwriting_render_path() {
        let mut runtime = PerfStats::default();
        runtime.record_render(Duration::from_millis(11));

        let mut presenter = PerfStats::default();
        presenter.record_convert(Duration::from_millis(5));
        presenter.record_blit(Duration::from_millis(2));
        presenter.set_l2_hit_rate(0.8);
        presenter.set_encode_queue_depth(4);
        presenter.set_encode_in_flight(1);
        presenter.add_encode_canceled_tasks(3);

        runtime.absorb_presenter_metrics(&presenter);

        assert_eq!(runtime.render_ms, 11.0);
        assert_eq!(runtime.convert_ms, 5.0);
        assert_eq!(runtime.blit_ms, 2.0);
        assert_eq!(runtime.cache_hit_rate_l2, 0.8);
        assert_eq!(runtime.encode_queue_depth, 4);
        assert_eq!(runtime.encode_in_flight, 1);
        assert_eq!(runtime.encode_canceled_tasks, 3);
    }

    #[test]
    fn summarizes_metrics_and_scalars() {
        let metric = summarize_metric(&[1.0, 2.0, 3.0, 4.0]);
        let scalar = summarize_scalar(&[1, 2, 3, 4]);

        assert_eq!(metric.count, 4);
        assert_eq!(metric.min_ms, 1.0);
        assert_eq!(metric.max_ms, 4.0);
        assert_eq!(scalar.count, 4);
        assert_eq!(scalar.min, 1.0);
        assert_eq!(scalar.max, 4.0);
    }

    #[test]
    fn does_not_retain_samples_unless_enabled() {
        let mut stats = PerfStats::default();
        stats.record_render(Duration::from_millis(12));
        stats.record_convert(Duration::from_millis(3));
        stats.record_blit(Duration::from_millis(1));
        stats.record_render_queue_wait(Duration::from_millis(4));
        stats.record_encode_queue_wait(Duration::from_millis(2));
        stats.set_queue_depth(7);
        stats.set_render_in_flight(2);
        stats.set_encode_queue_depth(3);
        stats.set_encode_in_flight(1);

        assert_eq!(summarize_metric(stats.render_samples_ms()).count, 0);
        assert_eq!(summarize_metric(stats.encode_samples_ms()).count, 0);
        assert_eq!(summarize_metric(stats.blit_samples_ms()).count, 0);
        assert_eq!(
            summarize_scalar(stats.render_queue_depth_samples()).count,
            0
        );
        assert_eq!(
            summarize_scalar(stats.encode_queue_depth_samples()).count,
            0
        );
    }

    #[test]
    fn parses_perf_scenarios() {
        assert_eq!(
            PerfScenarioId::parse("page-flip-forward"),
            Some(PerfScenarioId::PageFlipForward)
        );
        assert_eq!(PerfScenarioId::parse("unknown"), None);
    }

    #[test]
    fn merge_stats_averages_cache_hit_rates() {
        let mut first = PerfStats::default();
        first.set_l1_hit_rate(0.25);
        first.set_l2_hit_rate(0.5);

        let mut second = PerfStats::default();
        second.set_l1_hit_rate(0.75);
        second.set_l2_hit_rate(0.25);
        second.record_redraw(RedrawReason::Timer);

        let merged = merge_stats([&first, &second].into_iter());
        assert_eq!(merged.cache_hit_rate_l1, 0.5);
        assert_eq!(merged.cache_hit_rate_l2, 0.375);
        assert_eq!(merged.redraw_by_reason.timer, 1);
    }

    #[test]
    fn clear_blit_metrics_drops_samples_and_current_value() {
        let mut stats = PerfStats::default();
        stats.enable_sample_collection();
        stats.record_blit(Duration::from_millis(2));

        stats.clear_blit_metrics();

        assert_eq!(stats.blit_ms, 0.0);
        assert_eq!(stats.blit_samples, 0);
        assert_eq!(summarize_metric(stats.blit_samples_ms()).count, 0);
    }

    #[test]
    fn rejects_zero_measured_iterations() {
        let run = PerfRunConfig {
            measured_iterations: 0,
            ..PerfRunConfig::default()
        };

        let err = validate_run_config(&run).expect_err("zero measured iterations should fail");
        assert!(err.to_string().contains("measured iteration"));
    }
}
