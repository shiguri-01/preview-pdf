use std::time::Duration;

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
}

impl PerfStats {
    pub fn record_render(&mut self, elapsed: Duration) {
        self.render_ms = elapsed.as_secs_f64() * 1000.0;
        self.render_samples += 1;
    }

    pub fn record_convert(&mut self, elapsed: Duration) {
        self.convert_ms = elapsed.as_secs_f64() * 1000.0;
        self.convert_samples += 1;
    }

    pub fn record_blit(&mut self, elapsed: Duration) {
        self.blit_ms = elapsed.as_secs_f64() * 1000.0;
        self.blit_samples += 1;
    }

    pub fn set_l1_hit_rate(&mut self, rate: f64) {
        self.cache_hit_rate_l1 = rate.clamp(0.0, 1.0);
    }

    pub fn set_l2_hit_rate(&mut self, rate: f64) {
        self.cache_hit_rate_l2 = rate.clamp(0.0, 1.0);
    }

    pub fn set_queue_depth(&mut self, depth: usize) {
        self.queue_depth = depth;
    }

    pub fn add_canceled_tasks(&mut self, canceled: usize) {
        self.canceled_tasks += canceled;
    }

    pub fn absorb_presenter_metrics(&mut self, presenter: &PerfStats) {
        self.convert_ms = presenter.convert_ms;
        self.blit_ms = presenter.blit_ms;
        self.cache_hit_rate_l2 = presenter.cache_hit_rate_l2;
        self.convert_samples = presenter.convert_samples;
        self.blit_samples = presenter.blit_samples;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::PerfStats;

    #[test]
    fn records_milliseconds_and_clamped_rates() {
        let mut stats = PerfStats::default();
        stats.record_render(Duration::from_millis(12));
        stats.record_convert(Duration::from_millis(3));
        stats.record_blit(Duration::from_millis(1));
        stats.set_l1_hit_rate(1.5);
        stats.set_l2_hit_rate(-0.5);
        stats.set_queue_depth(7);
        stats.add_canceled_tasks(2);

        assert_eq!(stats.render_ms, 12.0);
        assert_eq!(stats.convert_ms, 3.0);
        assert_eq!(stats.blit_ms, 1.0);
        assert_eq!(stats.cache_hit_rate_l1, 1.0);
        assert_eq!(stats.cache_hit_rate_l2, 0.0);
        assert_eq!(stats.queue_depth, 7);
        assert_eq!(stats.canceled_tasks, 2);
    }

    #[test]
    fn absorbs_presenter_metrics_without_overwriting_render_path() {
        let mut runtime = PerfStats::default();
        runtime.record_render(Duration::from_millis(11));

        let mut presenter = PerfStats::default();
        presenter.record_convert(Duration::from_millis(5));
        presenter.record_blit(Duration::from_millis(2));
        presenter.set_l2_hit_rate(0.8);

        runtime.absorb_presenter_metrics(&presenter);

        assert_eq!(runtime.render_ms, 11.0);
        assert_eq!(runtime.convert_ms, 5.0);
        assert_eq!(runtime.blit_ms, 2.0);
        assert_eq!(runtime.cache_hit_rate_l2, 0.8);
    }
}
