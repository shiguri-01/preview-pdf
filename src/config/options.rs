use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};

pub use super::keymap::{KeymapBinding, KeymapOptions, KeymapPreset, KeymapWhen};
use super::types::Config;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppOptions {
    pub render: RenderOptions,
    pub cache: CacheOptions,
    pub view: ViewOptions,
    pub input: InputOptions,
    pub keymap: KeymapOptions,
    pub watch: WatchOptions,
}

impl AppOptions {
    pub fn merge(mut self, next: Self) -> Self {
        self.render = self.render.merge(next.render);
        self.cache = self.cache.merge(next.cache);
        self.view = self.view.merge(next.view);
        self.input = self.input.merge(next.input);
        self.keymap = self.keymap.merge(next.keymap);
        self.watch = self.watch.merge(next.watch);
        self
    }
}

impl From<Config> for AppOptions {
    fn from(config: Config) -> Self {
        Self {
            render: RenderOptions {
                worker_threads: Some(config.render.worker_threads),
                input_poll_timeout_idle_ms: Some(config.render.input_poll_timeout_idle_ms),
                input_poll_timeout_busy_ms: Some(config.render.input_poll_timeout_busy_ms),
                prefetch_pause_ms: Some(config.render.prefetch_pause_ms),
                prefetch_tick_ms: Some(config.render.prefetch_tick_ms),
                pending_redraw_interval_ms: Some(config.render.pending_redraw_interval_ms),
                prefetch_dispatch_budget_per_tick: Some(
                    config.render.prefetch_dispatch_budget_per_tick,
                ),
                max_render_scale: Some(config.render.max_render_scale),
            },
            cache: CacheOptions {
                l1_memory_budget_mb: Some(config.cache.l1_memory_budget_mb),
                l2_memory_budget_mb: Some(config.cache.l2_memory_budget_mb),
                l1_max_entries: Some(config.cache.l1_max_entries),
                l2_max_entries: Some(config.cache.l2_max_entries),
            },
            view: ViewOptions {
                initial_page: Some(config.view.initial_page),
                initial_zoom: Some(config.view.initial_zoom),
                initial_layout: Some(config.view.initial_layout),
                spread_direction: Some(config.view.spread_direction),
                spread_cover: Some(config.view.spread_cover),
            },
            input: InputOptions {
                sequence_timeout_ms: Some(config.input.sequence_timeout_ms),
            },
            keymap: KeymapOptions::default(),
            watch: WatchOptions {
                enabled: Some(config.watch.enabled),
                poll_interval_ms: Some(config.watch.poll_interval_ms),
                settle_delay_ms: Some(config.watch.settle_delay_ms),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderOptions {
    pub worker_threads: Option<usize>,
    pub input_poll_timeout_idle_ms: Option<u64>,
    pub input_poll_timeout_busy_ms: Option<u64>,
    pub prefetch_pause_ms: Option<u64>,
    pub prefetch_tick_ms: Option<u64>,
    pub pending_redraw_interval_ms: Option<u64>,
    pub prefetch_dispatch_budget_per_tick: Option<usize>,
    pub max_render_scale: Option<f32>,
}

impl RenderOptions {
    pub(super) fn merge(self, next: Self) -> Self {
        Self {
            worker_threads: next.worker_threads.or(self.worker_threads),
            input_poll_timeout_idle_ms: next
                .input_poll_timeout_idle_ms
                .or(self.input_poll_timeout_idle_ms),
            input_poll_timeout_busy_ms: next
                .input_poll_timeout_busy_ms
                .or(self.input_poll_timeout_busy_ms),
            prefetch_pause_ms: next.prefetch_pause_ms.or(self.prefetch_pause_ms),
            prefetch_tick_ms: next.prefetch_tick_ms.or(self.prefetch_tick_ms),
            pending_redraw_interval_ms: next
                .pending_redraw_interval_ms
                .or(self.pending_redraw_interval_ms),
            prefetch_dispatch_budget_per_tick: next
                .prefetch_dispatch_budget_per_tick
                .or(self.prefetch_dispatch_budget_per_tick),
            max_render_scale: next.max_render_scale.or(self.max_render_scale),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheOptions {
    pub l1_memory_budget_mb: Option<usize>,
    pub l2_memory_budget_mb: Option<usize>,
    pub l1_max_entries: Option<usize>,
    pub l2_max_entries: Option<usize>,
}

impl CacheOptions {
    pub(super) fn merge(self, next: Self) -> Self {
        Self {
            l1_memory_budget_mb: next.l1_memory_budget_mb.or(self.l1_memory_budget_mb),
            l2_memory_budget_mb: next.l2_memory_budget_mb.or(self.l2_memory_budget_mb),
            l1_max_entries: next.l1_max_entries.or(self.l1_max_entries),
            l2_max_entries: next.l2_max_entries.or(self.l2_max_entries),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewOptions {
    pub initial_page: Option<usize>,
    pub initial_zoom: Option<f32>,
    pub initial_layout: Option<PageLayoutMode>,
    pub spread_direction: Option<SpreadDirection>,
    pub spread_cover: Option<SpreadCoverPolicy>,
}

impl ViewOptions {
    pub(super) fn merge(self, next: Self) -> Self {
        Self {
            initial_page: next.initial_page.or(self.initial_page),
            initial_zoom: next.initial_zoom.or(self.initial_zoom),
            initial_layout: next.initial_layout.or(self.initial_layout),
            spread_direction: next.spread_direction.or(self.spread_direction),
            spread_cover: next.spread_cover.or(self.spread_cover),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputOptions {
    pub sequence_timeout_ms: Option<u64>,
}

impl InputOptions {
    pub(super) fn merge(self, next: Self) -> Self {
        Self {
            sequence_timeout_ms: next.sequence_timeout_ms.or(self.sequence_timeout_ms),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchOptions {
    pub enabled: Option<bool>,
    pub poll_interval_ms: Option<u64>,
    pub settle_delay_ms: Option<u64>,
}

impl WatchOptions {
    pub(super) fn merge(self, next: Self) -> Self {
        Self {
            enabled: next.enabled.or(self.enabled),
            poll_interval_ms: next.poll_interval_ms.or(self.poll_interval_ms),
            settle_delay_ms: next.settle_delay_ms.or(self.settle_delay_ms),
        }
    }
}
