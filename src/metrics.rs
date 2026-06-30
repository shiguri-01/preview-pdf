use std::time::Duration;

use serde::Serialize;

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
    pub fn reset(&mut self) {
        *self = Self::default();
    }

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
        self.encode_samples_ms = presenter.encode_samples_ms.clone();
        self.blit_samples_ms = presenter.blit_samples_ms.clone();
        self.encode_queue_wait_samples_ms = presenter.encode_queue_wait_samples_ms.clone();
        self.encode_queue_depth_samples = presenter.encode_queue_depth_samples.clone();
        self.encode_in_flight_samples = presenter.encode_in_flight_samples.clone();
    }

    pub fn clear_blit_metrics(&mut self) {
        self.blit_ms = 0.0;
        self.blit_samples = 0;
        self.blit_samples_ms.clear();
    }

    pub(crate) fn render_samples_ms(&self) -> &[f64] {
        &self.render_samples_ms
    }

    pub(crate) fn encode_samples_ms(&self) -> &[f64] {
        &self.encode_samples_ms
    }

    pub(crate) fn blit_samples_ms(&self) -> &[f64] {
        &self.blit_samples_ms
    }

    pub(crate) fn render_queue_wait_samples_ms(&self) -> &[f64] {
        &self.render_queue_wait_samples_ms
    }

    pub(crate) fn encode_queue_wait_samples_ms(&self) -> &[f64] {
        &self.encode_queue_wait_samples_ms
    }

    pub(crate) fn render_queue_depth_samples(&self) -> &[usize] {
        &self.render_queue_depth_samples
    }

    pub(crate) fn render_in_flight_samples(&self) -> &[usize] {
        &self.render_in_flight_samples
    }

    pub(crate) fn encode_queue_depth_samples(&self) -> &[usize] {
        &self.encode_queue_depth_samples
    }

    pub(crate) fn encode_in_flight_samples(&self) -> &[usize] {
        &self.encode_in_flight_samples
    }

    pub(crate) fn extend_render_samples_ms(&mut self, samples: &[f64]) {
        self.render_samples_ms.extend_from_slice(samples);
    }

    pub(crate) fn extend_encode_samples_ms(&mut self, samples: &[f64]) {
        self.encode_samples_ms.extend_from_slice(samples);
    }

    pub(crate) fn extend_blit_samples_ms(&mut self, samples: &[f64]) {
        self.blit_samples_ms.extend_from_slice(samples);
    }

    pub(crate) fn extend_render_queue_wait_samples_ms(&mut self, samples: &[f64]) {
        self.render_queue_wait_samples_ms.extend_from_slice(samples);
    }

    pub(crate) fn extend_encode_queue_wait_samples_ms(&mut self, samples: &[f64]) {
        self.encode_queue_wait_samples_ms.extend_from_slice(samples);
    }

    pub(crate) fn extend_render_queue_depth_samples(&mut self, samples: &[usize]) {
        self.render_queue_depth_samples.extend_from_slice(samples);
    }

    pub(crate) fn extend_render_in_flight_samples(&mut self, samples: &[usize]) {
        self.render_in_flight_samples.extend_from_slice(samples);
    }

    pub(crate) fn extend_encode_queue_depth_samples(&mut self, samples: &[usize]) {
        self.encode_queue_depth_samples.extend_from_slice(samples);
    }

    pub(crate) fn extend_encode_in_flight_samples(&mut self, samples: &[usize]) {
        self.encode_in_flight_samples.extend_from_slice(samples);
    }
}
