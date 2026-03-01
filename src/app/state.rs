use crate::command::ActionId;
use crate::palette::PaletteKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Palette,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteRequest {
    Open {
        kind: PaletteKind,
        seed: Option<String>,
    },
    Close,
}

#[derive(Debug, Clone, Default)]
pub struct StatusState {
    pub message: String,
    pub last_action_id: Option<ActionId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchUiState {
    pub active: bool,
    pub in_progress: bool,
    pub scanned_pages: usize,
    pub total_pages: usize,
    pub hits_found: usize,
    /// 0-based index into current result set.
    pub current_hit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheHandle {
    pub name: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheRefs {
    pub l1_rendered_pages: Option<CacheHandle>,
    pub l2_terminal_frames: Option<CacheHandle>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub current_page: usize,
    pub zoom: f32,
    pub scroll_x: i32,
    pub scroll_y: i32,
    pub debug_status_visible: bool,
    pub mode: Mode,
    pub status: StatusState,
    pub search_ui: SearchUiState,
    pub caches: CacheRefs,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_page: 0,
            zoom: 1.0,
            scroll_x: 0,
            scroll_y: 0,
            debug_status_visible: false,
            mode: Mode::Normal,
            status: StatusState::default(),
            search_ui: SearchUiState::default(),
            caches: CacheRefs::default(),
        }
    }
}
