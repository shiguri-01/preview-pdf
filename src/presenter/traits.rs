use std::future::{Future, pending};
use std::pin::Pin;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresenterFeedback {
    #[default]
    None,
    Pending,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresenterRenderMode {
    #[default]
    Full,
    InitialPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterRenderOptions {
    pub allow_stale_fallback: bool,
    pub render_mode: PresenterRenderMode,
}

impl PresenterRenderOptions {
    pub const fn new(allow_stale_fallback: bool, render_mode: PresenterRenderMode) -> Self {
        Self {
            allow_stale_fallback,
            render_mode,
        }
    }

    pub const fn is_initial_preview(self) -> bool {
        matches!(self.render_mode, PresenterRenderMode::InitialPreview)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterRenderOutcome {
    pub drew_image: bool,
    pub feedback: PresenterFeedback,
    pub used_stale_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenterBackgroundEvent {
    EncodeComplete { redraw_requested: bool },
}

pub trait ImagePresenter {
    fn initialize_terminal(&mut self) -> AppResult<()> {
        Ok(())
    }

    fn initialize_headless_for_perf(&mut self) -> AppResult<()> {
        self.reset_perf_metrics();
        self.initialize_terminal()
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

    fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        options: PresenterRenderOptions,
    ) -> AppResult<PresenterRenderOutcome>;
    fn capabilities(&self) -> PresenterCaps;

    fn has_pending_work(&self) -> bool {
        false
    }

    fn drain_background_events(&mut self) -> bool {
        false
    }

    fn recv_background_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<PresenterBackgroundEvent>> + 'a>> {
        Box::pin(pending())
    }

    fn perf_snapshot(&self) -> Option<PerfStats> {
        None
    }

    fn reset_perf_metrics(&mut self) {}

    fn enable_perf_sample_collection(&mut self) {}

    fn clear_perf_blit_metrics(&mut self) {}
}
