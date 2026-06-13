use serde::Deserialize;

use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::input::sequence::DEFAULT_SEQUENCE_TIMEOUT;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub render: RenderConfig,
    pub cache: CacheConfig,
    pub view: ViewConfig,
    pub input: InputConfig,
    pub watch: WatchConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct RenderConfig {
    pub worker_threads: usize,
    pub input_poll_timeout_idle_ms: u64,
    pub input_poll_timeout_busy_ms: u64,
    pub prefetch_pause_ms: u64,
    pub prefetch_tick_ms: u64,
    pub pending_redraw_interval_ms: u64,
    pub prefetch_dispatch_budget_per_tick: usize,
    pub max_render_scale: f32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            worker_threads: 3,
            input_poll_timeout_idle_ms: 16,
            input_poll_timeout_busy_ms: 8,
            prefetch_pause_ms: 120,
            prefetch_tick_ms: 8,
            pending_redraw_interval_ms: 33,
            prefetch_dispatch_budget_per_tick: 6,
            max_render_scale: 2.5,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CacheConfig {
    pub l1_memory_budget_mb: usize,
    pub l2_memory_budget_mb: usize,
    pub l1_max_entries: usize,
    pub l2_max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_memory_budget_mb: 512,
            l2_memory_budget_mb: 64,
            l1_max_entries: 128,
            l2_max_entries: 96,
        }
    }
}

impl CacheConfig {
    const MEBIBYTE: usize = 1024 * 1024;

    pub fn l1_memory_budget_bytes(&self) -> usize {
        self.l1_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }

    pub fn l2_memory_budget_bytes(&self) -> usize {
        self.l2_memory_budget_mb
            .saturating_mul(Self::MEBIBYTE)
            .max(1)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewConfig {
    pub initial_page: usize,
    pub initial_zoom: f32,
    pub initial_layout: PageLayoutMode,
    pub spread_direction: SpreadDirection,
    pub spread_cover: SpreadCoverPolicy,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            initial_page: 1,
            initial_zoom: 1.0,
            initial_layout: PageLayoutMode::Single,
            spread_direction: SpreadDirection::Ltr,
            spread_cover: SpreadCoverPolicy::Paired,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputConfig {
    pub sequence_timeout_ms: u64,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            sequence_timeout_ms: DEFAULT_SEQUENCE_TIMEOUT.as_millis() as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchConfig {
    pub enabled: bool,
    pub poll_interval_ms: u64,
    pub settle_delay_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_ms: 250,
            settle_delay_ms: 500,
        }
    }
}
