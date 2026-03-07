use std::time::{Duration, Instant};

const MAX_LATENCY_SAMPLES: usize = 1024;
const MAX_TIMESERIES_SAMPLES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedrawReason {
    Input,
    Completion,
    Timer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseSummary {
    pub count: u64,
    pub latest_ms: f64,
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeSeriesSample {
    pub at_ms: f64,
    pub value: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedrawSummary {
    pub input: u64,
    pub completion: u64,
    pub timer: u64,
    pub frames_drawn: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct LatencySeries {
    values_ms: Vec<f64>,
    total_ms: f64,
}

impl LatencySeries {
    fn record(&mut self, value_ms: f64) {
        self.values_ms.push(value_ms);
        if self.values_ms.len() > MAX_LATENCY_SAMPLES {
            let removed = self.values_ms.remove(0);
            self.total_ms -= removed;
        }
        self.total_ms += value_ms;
    }

    fn summary(&self, latest_ms: f64, total_count: u64) -> PhaseSummary {
        let window_count = self.values_ms.len() as u64;
        if window_count == 0 {
            return PhaseSummary {
                count: 0,
                latest_ms,
                avg_ms: 0.0,
                p50_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
            };
        }

        let mut sorted = self.values_ms.clone();
        sorted.sort_by(|left, right| {
            left.partial_cmp(right)
                .expect("perf latency samples must be finite")
        });
        PhaseSummary {
            count: total_count,
            latest_ms,
            avg_ms: self.total_ms / window_count as f64,
            p50_ms: percentile(&sorted, 0.50),
            p95_ms: percentile(&sorted, 0.95),
            p99_ms: percentile(&sorted, 0.99),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerfStats {
    pub render_ms: f64,
    pub convert_ms: f64,
    pub blit_ms: f64,
    pub render_wait_ms: f64,
    pub encode_wait_ms: f64,
    pub cache_hit_rate_l1: f64,
    pub cache_hit_rate_l2: f64,
    pub queue_depth: usize,
    pub canceled_tasks: usize,
    pub in_flight_tasks: usize,
    pub render_samples: u64,
    pub convert_samples: u64,
    pub blit_samples: u64,
    pub render_wait_samples: u64,
    pub encode_wait_samples: u64,
    started_at: Instant,
    render_series: LatencySeries,
    convert_series: LatencySeries,
    blit_series: LatencySeries,
    render_wait_series: LatencySeries,
    encode_wait_series: LatencySeries,
    queue_depth_samples: Vec<TimeSeriesSample>,
    canceled_task_samples: Vec<TimeSeriesSample>,
    in_flight_samples: Vec<TimeSeriesSample>,
    redraw_input: u64,
    redraw_completion: u64,
    redraw_timer: u64,
    frames_drawn: u64,
}

impl Default for PerfStats {
    fn default() -> Self {
        Self {
            render_ms: 0.0,
            convert_ms: 0.0,
            blit_ms: 0.0,
            render_wait_ms: 0.0,
            encode_wait_ms: 0.0,
            cache_hit_rate_l1: 0.0,
            cache_hit_rate_l2: 0.0,
            queue_depth: 0,
            canceled_tasks: 0,
            in_flight_tasks: 0,
            render_samples: 0,
            convert_samples: 0,
            blit_samples: 0,
            render_wait_samples: 0,
            encode_wait_samples: 0,
            started_at: Instant::now(),
            render_series: LatencySeries {
                values_ms: Vec::new(),
                total_ms: 0.0,
            },
            convert_series: LatencySeries {
                values_ms: Vec::new(),
                total_ms: 0.0,
            },
            blit_series: LatencySeries {
                values_ms: Vec::new(),
                total_ms: 0.0,
            },
            render_wait_series: LatencySeries {
                values_ms: Vec::new(),
                total_ms: 0.0,
            },
            encode_wait_series: LatencySeries {
                values_ms: Vec::new(),
                total_ms: 0.0,
            },
            queue_depth_samples: Vec::new(),
            canceled_task_samples: Vec::new(),
            in_flight_samples: Vec::new(),
            redraw_input: 0,
            redraw_completion: 0,
            redraw_timer: 0,
            frames_drawn: 0,
        }
    }
}

impl PerfStats {
    pub fn record_render(&mut self, elapsed: Duration) {
        self.render_ms = elapsed.as_secs_f64() * 1000.0;
        self.render_series.record(self.render_ms);
        self.render_samples += 1;
    }

    pub fn record_convert(&mut self, elapsed: Duration) {
        self.convert_ms = elapsed.as_secs_f64() * 1000.0;
        self.convert_series.record(self.convert_ms);
        self.convert_samples += 1;
    }

    pub fn record_blit(&mut self, elapsed: Duration) {
        self.blit_ms = elapsed.as_secs_f64() * 1000.0;
        self.blit_series.record(self.blit_ms);
        self.blit_samples += 1;
    }

    pub fn record_render_wait(&mut self, elapsed: Duration) {
        self.render_wait_ms = elapsed.as_secs_f64() * 1000.0;
        self.render_wait_series.record(self.render_wait_ms);
        self.render_wait_samples += 1;
    }

    pub fn record_encode_wait(&mut self, elapsed: Duration) {
        self.encode_wait_ms = elapsed.as_secs_f64() * 1000.0;
        self.encode_wait_series.record(self.encode_wait_ms);
        self.encode_wait_samples += 1;
    }

    pub fn set_l1_hit_rate(&mut self, rate: f64) {
        self.cache_hit_rate_l1 = rate.clamp(0.0, 1.0);
    }

    pub fn set_l2_hit_rate(&mut self, rate: f64) {
        self.cache_hit_rate_l2 = rate.clamp(0.0, 1.0);
    }

    pub fn set_queue_depth(&mut self, depth: usize) {
        self.queue_depth = depth;
        self.push_sample(SampleSeries::QueueDepth, depth as u64);
    }

    pub fn set_in_flight_tasks(&mut self, inflight: usize) {
        self.in_flight_tasks = inflight;
        self.push_sample(SampleSeries::InFlight, inflight as u64);
    }

    pub fn set_queue_state(&mut self, depth: usize, inflight: usize) {
        self.set_queue_depth(depth);
        self.set_in_flight_tasks(inflight);
    }

    pub fn add_canceled_tasks(&mut self, canceled: usize) {
        self.canceled_tasks += canceled;
        self.push_sample(SampleSeries::CanceledTasks, self.canceled_tasks as u64);
    }

    pub fn record_redraw_request(&mut self, reason: RedrawReason) {
        match reason {
            RedrawReason::Input => self.redraw_input += 1,
            RedrawReason::Completion => self.redraw_completion += 1,
            RedrawReason::Timer => self.redraw_timer += 1,
        }
    }

    pub fn record_frame_draw(&mut self) {
        self.frames_drawn += 1;
    }

    pub fn absorb_presenter_metrics(&mut self, presenter: &PerfStats) {
        self.convert_ms = presenter.convert_ms;
        self.blit_ms = presenter.blit_ms;
        self.encode_wait_ms = presenter.encode_wait_ms;
        self.cache_hit_rate_l2 = presenter.cache_hit_rate_l2;
        self.convert_samples = presenter.convert_samples;
        self.blit_samples = presenter.blit_samples;
        self.encode_wait_samples = presenter.encode_wait_samples;
        self.convert_series = presenter.convert_series.clone();
        self.blit_series = presenter.blit_series.clone();
        self.encode_wait_series = presenter.encode_wait_series.clone();
    }

    pub fn render_summary(&self) -> PhaseSummary {
        self.render_series
            .summary(self.render_ms, self.render_samples)
    }

    pub fn convert_summary(&self) -> PhaseSummary {
        self.convert_series
            .summary(self.convert_ms, self.convert_samples)
    }

    pub fn blit_summary(&self) -> PhaseSummary {
        self.blit_series.summary(self.blit_ms, self.blit_samples)
    }

    pub fn render_wait_summary(&self) -> PhaseSummary {
        self.render_wait_series
            .summary(self.render_wait_ms, self.render_wait_samples)
    }

    pub fn encode_wait_summary(&self) -> PhaseSummary {
        self.encode_wait_series
            .summary(self.encode_wait_ms, self.encode_wait_samples)
    }

    pub fn queue_depth_samples(&self) -> &[TimeSeriesSample] {
        &self.queue_depth_samples
    }

    pub fn canceled_task_samples(&self) -> &[TimeSeriesSample] {
        &self.canceled_task_samples
    }

    pub fn in_flight_samples(&self) -> &[TimeSeriesSample] {
        &self.in_flight_samples
    }

    pub fn redraw_summary(&self) -> RedrawSummary {
        RedrawSummary {
            input: self.redraw_input,
            completion: self.redraw_completion,
            timer: self.redraw_timer,
            frames_drawn: self.frames_drawn,
        }
    }

    fn push_sample(&mut self, series: SampleSeries, value: u64) {
        let sample = TimeSeriesSample {
            at_ms: self.started_at.elapsed().as_secs_f64() * 1000.0,
            value,
        };
        let target = match series {
            SampleSeries::QueueDepth => &mut self.queue_depth_samples,
            SampleSeries::CanceledTasks => &mut self.canceled_task_samples,
            SampleSeries::InFlight => &mut self.in_flight_samples,
        };
        target.push(sample);
        if target.len() > MAX_TIMESERIES_SAMPLES {
            target.remove(0);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleSeries {
    QueueDepth,
    CanceledTasks,
    InFlight,
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MAX_LATENCY_SAMPLES, PerfStats, RedrawReason};

    #[test]
    fn records_milliseconds_rates_and_time_series() {
        let mut stats = PerfStats::default();
        stats.record_render(Duration::from_millis(12));
        stats.record_convert(Duration::from_millis(3));
        stats.record_blit(Duration::from_millis(1));
        stats.record_render_wait(Duration::from_millis(7));
        stats.record_encode_wait(Duration::from_millis(2));
        stats.set_l1_hit_rate(1.5);
        stats.set_l2_hit_rate(-0.5);
        stats.set_queue_state(7, 2);
        stats.add_canceled_tasks(2);
        stats.record_redraw_request(RedrawReason::Input);
        stats.record_redraw_request(RedrawReason::Timer);
        stats.record_frame_draw();

        assert_eq!(stats.render_ms, 12.0);
        assert_eq!(stats.convert_ms, 3.0);
        assert_eq!(stats.blit_ms, 1.0);
        assert_eq!(stats.render_wait_ms, 7.0);
        assert_eq!(stats.encode_wait_ms, 2.0);
        assert_eq!(stats.cache_hit_rate_l1, 1.0);
        assert_eq!(stats.cache_hit_rate_l2, 0.0);
        assert_eq!(stats.queue_depth, 7);
        assert_eq!(stats.in_flight_tasks, 2);
        assert_eq!(stats.canceled_tasks, 2);
        assert_eq!(stats.queue_depth_samples().len(), 1);
        assert_eq!(stats.in_flight_samples().len(), 1);
        assert_eq!(stats.canceled_task_samples().len(), 1);
        let redraw = stats.redraw_summary();
        assert_eq!(redraw.input, 1);
        assert_eq!(redraw.timer, 1);
        assert_eq!(redraw.frames_drawn, 1);
    }

    #[test]
    fn absorbs_presenter_metrics_without_overwriting_render_path() {
        let mut runtime = PerfStats::default();
        runtime.record_render(Duration::from_millis(11));

        let mut presenter = PerfStats::default();
        presenter.record_convert(Duration::from_millis(5));
        presenter.record_blit(Duration::from_millis(2));
        presenter.record_encode_wait(Duration::from_millis(1));
        presenter.set_l2_hit_rate(0.8);

        runtime.absorb_presenter_metrics(&presenter);

        assert_eq!(runtime.render_ms, 11.0);
        assert_eq!(runtime.convert_ms, 5.0);
        assert_eq!(runtime.blit_ms, 2.0);
        assert_eq!(runtime.encode_wait_ms, 1.0);
        assert_eq!(runtime.cache_hit_rate_l2, 0.8);
    }

    #[test]
    fn summaries_include_percentiles() {
        let mut stats = PerfStats::default();
        for value in [2_u64, 4, 6, 8, 20] {
            stats.record_render(Duration::from_millis(value));
        }

        let summary = stats.render_summary();
        assert_eq!(summary.count, 5);
        assert_eq!(summary.latest_ms, 20.0);
        assert_eq!(summary.avg_ms, 8.0);
        assert_eq!(summary.p50_ms, 6.0);
        assert_eq!(summary.p95_ms, 20.0);
        assert_eq!(summary.p99_ms, 20.0);
    }

    #[test]
    fn summary_count_tracks_total_samples_beyond_window() {
        let mut stats = PerfStats::default();
        for _ in 0..(MAX_LATENCY_SAMPLES + 5) {
            stats.record_render(Duration::from_millis(1));
        }

        let summary = stats.render_summary();
        assert_eq!(summary.count, (MAX_LATENCY_SAMPLES + 5) as u64);
        assert_eq!(stats.render_samples, (MAX_LATENCY_SAMPLES + 5) as u64);
    }
}
