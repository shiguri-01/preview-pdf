use ratatui::Frame;
use ratatui::layout::Rect;

use crate::backend::RgbaFrame;
use crate::error::AppResult;
use crate::perf::PerfStats;
use crate::render::cache::RenderedPageKey;
use crate::render::prefetch::PrefetchClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenterKind {
    RatatuiImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Viewport {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PanOffset {
    pub cells_x: i32,
    pub cells_y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresenterCaps {
    pub backend_name: &'static str,
    pub supports_l2_cache: bool,
    pub cell_px: Option<(u16, u16)>,
    /// Maximum render scale the presenter benefits from.
    /// Kitty/iTerm2 send raw pixels so high-res rendering pays off (2.5).
    /// Sixel is color-quantized so returns diminish above 1.5.
    /// Halfblocks have very limited resolution so 1.0 suffices.
    pub preferred_max_render_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterRuntimeInfo {
    pub graphics_protocol: Option<&'static str>,
}

pub trait ImagePresenter {
    fn initialize_terminal(&mut self) -> AppResult<()> {
        Ok(())
    }

    fn status_label(&self) -> String {
        self.capabilities().backend_name.to_string()
    }

    fn runtime_info(&self) -> PresenterRuntimeInfo {
        PresenterRuntimeInfo::default()
    }

    fn prepare(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
        generation: u64,
    ) -> AppResult<()>;

    fn prefetch_encode(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
        class: PrefetchClass,
        generation: u64,
    ) -> AppResult<()> {
        let _ = (cache_key, frame, viewport, pan, class, generation);
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, area: Rect) -> AppResult<bool>;
    fn capabilities(&self) -> PresenterCaps;

    fn has_pending_work(&self) -> bool {
        false
    }

    fn drain_background_events(&mut self) -> bool {
        false
    }

    fn perf_snapshot(&self) -> Option<PerfStats> {
        None
    }
}
