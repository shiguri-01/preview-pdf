use serde::{Deserialize, Serialize};

use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::presenter::GraphicsProtocol;

pub use super::keymap::{KeymapBinding, KeymapOptions, KeymapPreset, KeymapWhen};

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
