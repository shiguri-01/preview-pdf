use serde::{Deserialize, Serialize};

use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::presenter::GraphicsProtocol;

pub use super::keymap::{KeymapBinding, KeymapOptions, KeymapPreset, KeymapWhen};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppOptions {
    pub render: RenderOptions,
    pub view: ViewOptions,
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
pub struct WatchOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}
