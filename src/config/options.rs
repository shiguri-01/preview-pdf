use serde::{Deserialize, Serialize};

use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::presenter::GraphicsProtocol;

pub use super::keymap::{KeymapBinding, KeymapOptions, KeymapPreset, KeymapWhen};
use super::types::Config;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppOptions {
    pub render: RenderOptions,
    pub cache: CacheOptions,
    pub view: ViewOptions,
    pub input: InputOptions,
    // Compiled bindings are accumulated in source order by AppOptionsResolver,
    // separately from scalar option merging.
    #[serde(skip)]
    pub keymap: KeymapOptions,
    pub watch: WatchOptions,
}

impl From<Config> for AppOptions {
    fn from(config: Config) -> Self {
        Self {
            render: RenderOptions {
                graphics_protocol: config.render.graphics_protocol,
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
                settle_delay_ms: Some(config.watch.settle_delay_ms),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphics_protocol: Option<GraphicsProtocol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_threads: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_poll_timeout_idle_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_poll_timeout_busy_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch_pause_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch_tick_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_redraw_interval_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch_dispatch_budget_per_tick: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_render_scale: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_memory_budget_mb: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_memory_budget_mb: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_max_entries: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_max_entries: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_zoom: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_layout: Option<PageLayoutMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread_direction: Option<SpreadDirection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread_cover: Option<SpreadCoverPolicy>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_delay_ms: Option<u64>,
}
