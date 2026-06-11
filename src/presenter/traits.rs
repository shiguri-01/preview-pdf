use std::future::{Future, pending};
use std::pin::Pin;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::backend::RgbaFrame;
use crate::error::AppResult;
use crate::perf::PerfStats;
use crate::render::cache::RenderedPageKey;
use crate::work::WorkClass;

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

impl From<Rect> for Viewport {
    fn from(area: Rect) -> Self {
        Self {
            x: area.x,
            y: area.y,
            width: area.width.max(1),
            height: area.height.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PanOffset {
    pub cells_x: i32,
    pub cells_y: i32,
}

impl PanOffset {
    pub(crate) fn clamp_to_pixel_bounds(
        &mut self,
        max_x_px: u32,
        max_y_px: u32,
        cell_width_px: u16,
        cell_height_px: u16,
    ) {
        self.cells_x = self
            .cells_x
            .clamp(0, max_pan_cells(max_x_px, cell_width_px));
        self.cells_y = self
            .cells_y
            .clamp(0, max_pan_cells(max_y_px, cell_height_px));
    }

    pub(crate) fn pixel_origin(
        self,
        max_x_px: u32,
        max_y_px: u32,
        cell_width_px: u16,
        cell_height_px: u16,
    ) -> (u32, u32) {
        (
            pan_cell_origin_px(self.cells_x, cell_width_px, max_x_px),
            pan_cell_origin_px(self.cells_y, cell_height_px, max_y_px),
        )
    }
}

fn max_pan_cells(max_px: u32, cell_px: u16) -> i32 {
    (max_px / u32::from(cell_px.max(1))).min(i32::MAX as u32) as i32
}

fn pan_cell_origin_px(cells: i32, cell_px: u16, max_px: u32) -> u32 {
    u32::try_from(cells)
        .unwrap_or(0)
        .saturating_mul(u32::from(cell_px.max(1)))
        .min(max_px)
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
    pub preserve_stable_image: bool,
    pub force_image_redraw: bool,
}

impl PresenterRenderOptions {
    pub const fn new(allow_stale_fallback: bool, render_mode: PresenterRenderMode) -> Self {
        Self {
            allow_stale_fallback,
            render_mode,
            preserve_stable_image: false,
            force_image_redraw: false,
        }
    }

    pub const fn is_initial_preview(self) -> bool {
        matches!(self.render_mode, PresenterRenderMode::InitialPreview)
    }
}

pub struct PresenterSlot<'a> {
    pub cache_key: Option<RenderedPageKey>,
    pub frame: Option<&'a RgbaFrame>,
    pub viewport: Viewport,
    pub pan: PanOffset,
    pub overlay_stamp: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenterRenderSlot {
    pub area: Rect,
    pub options: PresenterRenderOptions,
    pub active: bool,
    pub horizontal_align: PresenterHorizontalAlign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresenterHorizontalAlign {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterSlotOutcome {
    pub area: Rect,
    pub active: bool,
    pub drew_image: bool,
    pub feedback: PresenterFeedback,
    pub used_stale_fallback: bool,
}

impl PresenterSlotOutcome {
    pub const fn active(
        area: Rect,
        drew_image: bool,
        feedback: PresenterFeedback,
        used_stale_fallback: bool,
    ) -> Self {
        Self {
            area,
            active: true,
            drew_image,
            feedback,
            used_stale_fallback,
        }
    }

    pub const fn inactive(area: Rect) -> Self {
        Self {
            area,
            active: false,
            drew_image: false,
            feedback: PresenterFeedback::None,
            used_stale_fallback: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresenterRenderOutcome {
    pub drew_image: bool,
    pub feedback: PresenterFeedback,
    pub used_stale_fallback: bool,
    pub slots: Vec<PresenterSlotOutcome>,
}

impl PresenterRenderOutcome {
    pub fn pending() -> Self {
        Self {
            feedback: PresenterFeedback::Pending,
            ..Self::default()
        }
    }

    pub fn failed() -> Self {
        Self {
            feedback: PresenterFeedback::Failed,
            ..Self::default()
        }
    }

    pub fn from_slot(slot: PresenterSlotOutcome) -> Self {
        Self {
            drew_image: slot.active && slot.drew_image,
            feedback: if slot.active {
                slot.feedback
            } else {
                PresenterFeedback::None
            },
            used_stale_fallback: slot.active && slot.used_stale_fallback,
            slots: vec![slot],
        }
    }

    pub fn aggregate_slots(slots: Vec<PresenterSlotOutcome>) -> Self {
        let mut outcome = Self {
            slots,
            ..Self::default()
        };
        for slot in &outcome.slots {
            if !slot.active {
                continue;
            }
            outcome.drew_image |= slot.drew_image;
            outcome.used_stale_fallback |= slot.used_stale_fallback;
            outcome.feedback = combine_feedback(outcome.feedback, slot.feedback);
        }
        outcome
    }
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
        overlay_stamp: u64,
        generation: u64,
    ) -> AppResult<()> {
        self.prepare_slots(&[PresenterSlot {
            cache_key: Some(cache_key),
            frame: Some(frame),
            viewport,
            pan,
            overlay_stamp,
            generation,
        }])
    }

    fn prepare_slots(&mut self, slots: &[PresenterSlot<'_>]) -> AppResult<()>;

    #[allow(clippy::too_many_arguments)]
    fn prefetch_encode(
        &mut self,
        cache_key: RenderedPageKey,
        frame: &RgbaFrame,
        viewport: Viewport,
        pan: PanOffset,
        overlay_stamp: u64,
        class: WorkClass,
        generation: u64,
    ) -> AppResult<()> {
        let _ = (
            cache_key,
            frame,
            viewport,
            pan,
            overlay_stamp,
            class,
            generation,
        );
        Ok(())
    }

    fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        options: PresenterRenderOptions,
    ) -> AppResult<PresenterRenderOutcome> {
        self.render_slots(
            frame,
            &[PresenterRenderSlot {
                area,
                options,
                active: true,
                horizontal_align: PresenterHorizontalAlign::Center,
            }],
        )
    }

    fn render_slots(
        &mut self,
        frame: &mut Frame<'_>,
        slots: &[PresenterRenderSlot],
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

pub fn combine_feedback(left: PresenterFeedback, right: PresenterFeedback) -> PresenterFeedback {
    match (left, right) {
        (PresenterFeedback::Failed, _) | (_, PresenterFeedback::Failed) => {
            PresenterFeedback::Failed
        }
        (PresenterFeedback::Pending, _) | (_, PresenterFeedback::Pending) => {
            PresenterFeedback::Pending
        }
        (PresenterFeedback::None, PresenterFeedback::None) => PresenterFeedback::None,
    }
}

#[cfg(test)]
mod tests {
    use super::PanOffset;

    #[test]
    fn pan_pixel_bounds_cap_cells_before_signed_overflow() {
        let mut pan = PanOffset {
            cells_x: i32::MAX,
            cells_y: i32::MAX,
        };

        pan.clamp_to_pixel_bounds(u32::MAX, u32::MAX, 1, 1);

        assert_eq!(pan.cells_x, i32::MAX);
        assert_eq!(pan.cells_y, i32::MAX);
    }

    #[test]
    fn pan_pixel_origin_saturates_before_unsigned_overflow() {
        let pan = PanOffset {
            cells_x: i32::MAX,
            cells_y: i32::MAX,
        };

        let origin = pan.pixel_origin(u32::MAX, u32::MAX, 3, 3);

        assert_eq!(origin, (u32::MAX, u32::MAX));
    }

    #[test]
    fn pan_pixel_bounds_clamp_negative_cells_to_zero() {
        let mut pan = PanOffset {
            cells_x: -1,
            cells_y: -2,
        };

        pan.clamp_to_pixel_bounds(100, 200, 10, 20);
        let origin = pan.pixel_origin(100, 200, 10, 20);

        assert_eq!(pan, PanOffset::default());
        assert_eq!(origin, (0, 0));
    }
}
