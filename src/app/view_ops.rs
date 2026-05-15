use ratatui::layout::Rect;
use ratatui::widgets::Clear;

use crate::app::PageLayoutMode;
use crate::backend::PdfBackend;
use crate::config::Config;
use crate::error::AppResult;
use crate::highlight::HighlightOverlaySnapshot;
use crate::input::sequence::SequenceRegistrySnapshot;
use crate::palette::PaletteView;
use crate::presenter::{
    PanOffset, PresenterFeedback, PresenterHorizontalAlign, PresenterRenderMode,
    PresenterRenderOptions, PresenterRenderOutcome, PresenterRenderSlot, PresenterSlotOutcome,
    Viewport,
};
use crate::render::cache::RenderedPageKey;
use crate::ui;

use super::constants::DEFAULT_PAGE_SIZE_PT;
use super::core::{App, RenderSubsystem};
use super::scale::{
    compute_render_scale, compute_scale, quantize_scale, resolved_cell_size_px, scale_eq,
};
use super::state::{AppState, VisiblePageSlots};
use super::terminal_session::TerminalSurface;

const SPREAD_GAP_CELLS: u16 = 2;
const INITIAL_PREVIEW_SCALE_RATIO: f32 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct InitialPreviewPlan {
    pub(super) scale: f32,
    pub(super) page_keys: Vec<RenderedPageKey>,
    pub(super) presenter_key: RenderedPageKey,
}

pub(super) struct RenderFramePlan {
    pub(super) palette_view: Option<PaletteView>,
    pub(super) help_keymap: SequenceRegistrySnapshot,
    pub(super) status_bar_segments: Vec<String>,
    pub(super) page_count: usize,
    pub(super) visible_pages: VisiblePageSlots,
    pub(super) current_scale: f32,
    pub(super) initial_preview: Option<InitialPreviewPlan>,
    pub(super) presenter_key: RenderedPageKey,
    pub(super) highlight_overlay: HighlightOverlaySnapshot,
    pub(super) generation: u64,
    pub(super) nav_streak: usize,
}

impl App {
    pub(super) fn current_viewport<S: TerminalSurface>(
        session: &S,
        debug_status_visible: bool,
    ) -> Option<Viewport> {
        let area = session.size().ok()?.into();
        let layout = ui::split_layout(area, debug_status_visible);
        if layout.viewer_inner.width == 0 || layout.viewer_inner.height == 0 {
            return None;
        }

        Some(Viewport {
            x: layout.viewer_inner.x,
            y: layout.viewer_inner.y,
            width: layout.viewer_inner.width.max(1),
            height: layout.viewer_inner.height.max(1),
        })
    }

    pub(super) fn compute_current_scale(
        &self,
        pdf: &dyn PdfBackend,
        page: usize,
        viewport: Option<Viewport>,
    ) -> f32 {
        let Some(viewport) = viewport else {
            return quantize_scale(self.state.zoom);
        };

        let slots = self
            .state
            .visible_page_slots_for_page(page, pdf.page_count());
        let (page_width_pt, page_height_pt) =
            resolve_layout_dimensions(pdf, self.state.page_layout_mode, slots);
        let caps = self.render.presenter.capabilities();
        let max_scale = caps
            .preferred_max_render_scale
            .clamp(1.0, self.config.render.max_render_scale);
        let render_scale = compute_render_scale(
            viewport,
            caps.cell_px,
            page_width_pt,
            page_height_pt,
            max_scale,
        );
        compute_scale(self.state.zoom, render_scale)
    }

    pub(super) fn current_pan(&self) -> PanOffset {
        PanOffset {
            cells_x: self.state.pan_x,
            cells_y: self.state.pan_y,
        }
    }
}

impl RenderSubsystem {
    #[allow(clippy::too_many_arguments)]
    fn prepare_single_page_or_preview_from_cache(
        &mut self,
        pdf: &dyn PdfBackend,
        viewport: Viewport,
        page: usize,
        full_scale: f32,
        initial_preview: Option<&InitialPreviewPlan>,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        highlight_overlay: &HighlightOverlaySnapshot,
        generation: u64,
    ) -> AppResult<Option<PresenterRenderMode>> {
        if self.runtime.try_prepare_current_page_from_cache(
            pdf,
            self.presenter.as_mut(),
            viewport,
            page,
            full_scale,
            pan,
            cell_px,
            enable_crop,
            highlight_overlay,
            generation,
        )? {
            return Ok(Some(PresenterRenderMode::Full));
        }

        let Some(preview_plan) = initial_preview else {
            return Ok(None);
        };

        if self.runtime.try_prepare_cached_page_from_cache(
            pdf,
            self.presenter.as_mut(),
            viewport,
            preview_plan.page_keys[0],
            pan,
            cell_px,
            enable_crop,
            highlight_overlay,
            generation,
        )? {
            return Ok(Some(PresenterRenderMode::InitialPreview));
        }

        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_spread_or_preview_from_cache(
        &mut self,
        pdf: &dyn PdfBackend,
        viewport: Viewport,
        visible_pages: VisiblePageSlots,
        slot_areas: SpreadSlotAreas,
        full_scale: f32,
        initial_preview: Option<&InitialPreviewPlan>,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        highlight_overlay: &HighlightOverlaySnapshot,
        generation: u64,
        spread_gap_px: u32,
    ) -> AppResult<Option<(PresenterRenderMode, Vec<PresenterRenderSlot>)>> {
        let page_slots = slot_areas.page_slots(visible_pages);
        let attempts = initial_preview.map_or_else(
            || vec![(PresenterRenderMode::Full, full_scale)],
            |preview| {
                vec![
                    (PresenterRenderMode::Full, full_scale),
                    (PresenterRenderMode::InitialPreview, preview.scale),
                ]
            },
        );
        for (render_mode, scale) in attempts {
            if let Some(render_slots) = self.try_prepare_spread_slots_from_cache(
                pdf,
                viewport,
                visible_pages,
                slot_areas,
                &page_slots,
                scale,
                pan,
                cell_px,
                enable_crop,
                highlight_overlay,
                generation,
                spread_gap_px,
                render_mode,
            )? {
                return Ok(Some((render_mode, render_slots)));
            }
        }

        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_prepare_spread_slots_from_cache(
        &mut self,
        pdf: &dyn PdfBackend,
        viewport: Viewport,
        visible_pages: VisiblePageSlots,
        slot_areas: SpreadSlotAreas,
        page_slots: &[(Option<usize>, Viewport); 2],
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        highlight_overlay: &HighlightOverlaySnapshot,
        generation: u64,
        spread_gap_px: u32,
        render_mode: PresenterRenderMode,
    ) -> AppResult<Option<Vec<PresenterRenderSlot>>> {
        if enable_crop {
            return self
                .runtime
                .try_prepare_spread_canvas_slots_from_cache(
                    pdf,
                    self.presenter.as_mut(),
                    viewport,
                    visible_pages,
                    scale,
                    pan,
                    cell_px,
                    highlight_overlay,
                    generation,
                    spread_gap_px,
                )
                .map(|areas| areas.map(|areas| render_areas_to_slots(areas, render_mode)));
        }

        if self.runtime.try_prepare_page_slots_from_cache(
            pdf,
            self.presenter.as_mut(),
            page_slots,
            scale,
            pan,
            cell_px,
            false,
            highlight_overlay,
            generation,
        )? {
            let options = PresenterRenderOptions::new(false, render_mode);
            return Ok(Some(
                slot_areas.render_slots_for_pages(visible_pages, options),
            ));
        }

        Ok(None)
    }

    pub(super) fn render_frame(
        &mut self,
        state: &mut AppState,
        _config: &Config,
        session: &mut impl TerminalSurface,
        pdf: &dyn PdfBackend,
        plan: RenderFramePlan,
    ) -> AppResult<()> {
        let RenderFramePlan {
            palette_view,
            help_keymap,
            status_bar_segments,
            page_count,
            visible_pages,
            current_scale,
            initial_preview,
            presenter_key: _presenter_key,
            highlight_overlay,
            generation,
            nav_streak: _nav_streak,
        } = plan;
        let image_occluded = palette_view.is_some() || state.mode == super::state::Mode::Help;
        // Keep the last ready frame visible while the next page is still preparing
        // so page flips do not briefly expose the terminal background.
        // Stable image slots are preserved while overlays are open; once an
        // overlay closes, force one redraw to restore cells it covered.
        let render_options = presenter_render_options(
            self.viewer_has_image,
            PresenterRenderMode::Full,
            image_occluded,
            self.image_occluded_last_frame && !image_occluded,
        );
        let file_name = pdf
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| pdf.path().display().to_string());
        let presenter_caps = self.presenter.capabilities();
        let presenter_runtime = self.presenter.runtime_info();
        let enable_crop = state.zoom > 1.0;
        let mut pan = PanOffset {
            cells_x: state.pan_x,
            cells_y: state.pan_y,
        };
        let mut render_failed = false;
        let mut render_feedback = PresenterFeedback::None;
        let mut viewer_has_image = self.viewer_has_image;
        let loading_label = format_loading_target(visible_pages);
        let render_target = format_render_target(visible_pages);
        let spread_gap_px = u32::from(
            resolved_cell_size_px(presenter_caps.cell_px)
                .0
                .saturating_mul(SPREAD_GAP_CELLS),
        );
        session.draw(|frame| {
            let layout = ui::split_layout(frame.area(), state.debug_status_visible);
            ui::draw_chrome(
                frame,
                layout,
                state,
                &file_name,
                page_count,
                presenter_caps.backend_name,
                presenter_runtime.graphics_protocol,
                &status_bar_segments,
            );

            let viewport = Viewport {
                x: layout.viewer_inner.x,
                y: layout.viewer_inner.y,
                width: layout.viewer_inner.width.max(1),
                height: layout.viewer_inner.height.max(1),
            };
            let image_area = layout.viewer_inner;
            let spread_slot_areas = split_spread_slot_areas(image_area, SPREAD_GAP_CELLS);

            let prepare_result = match state.page_layout_mode {
                PageLayoutMode::Single => self
                    .prepare_single_page_or_preview_from_cache(
                        pdf,
                        viewport,
                        visible_pages.anchor_page,
                        current_scale,
                        initial_preview.as_ref(),
                        &mut pan,
                        presenter_caps.cell_px,
                        enable_crop,
                        &highlight_overlay,
                        generation,
                    )
                    .map(|mode| mode.map(|mode| (mode, Vec::new()))),
                PageLayoutMode::Spread => self.prepare_spread_or_preview_from_cache(
                    pdf,
                    viewport,
                    visible_pages,
                    spread_slot_areas,
                    current_scale,
                    initial_preview.as_ref(),
                    &mut pan,
                    presenter_caps.cell_px,
                    enable_crop,
                    &highlight_overlay,
                    generation,
                    spread_gap_px,
                ),
            };

            match prepare_result {
                Ok(Some((render_mode, spread_render_slots))) => {
                    let options = PresenterRenderOptions {
                        render_mode,
                        ..render_options
                    };
                    let render_result = match state.page_layout_mode {
                        PageLayoutMode::Single => self.presenter.render(frame, image_area, options),
                        PageLayoutMode::Spread => {
                            spread_slot_areas.clear_gap(frame);
                            let render_slots: Vec<_> = spread_render_slots
                                .into_iter()
                                .map(|slot| PresenterRenderSlot { options, ..slot })
                                .collect();
                            self.presenter.render_slots(frame, &render_slots)
                        }
                    };
                    match render_result {
                        Ok(outcome) => {
                            let outcome = normalize_render_outcome(render_mode, outcome);
                            render_feedback = outcome.feedback;
                            if outcome.drew_image {
                                viewer_has_image = true;
                            }
                            let allow_viewer_loading =
                                state.page_layout_mode == PageLayoutMode::Single;
                            draw_viewer_outcome(
                                frame,
                                image_area,
                                &outcome,
                                loading_label.as_str(),
                                None,
                                viewer_has_image,
                                allow_viewer_loading,
                            );
                            if state.page_layout_mode == PageLayoutMode::Spread {
                                draw_spread_loading_overlays(frame, &outcome, visible_pages);
                            }
                        }
                        Err(err) => {
                            let _ = err;
                            render_failed = true;
                            let outcome = PresenterRenderOutcome {
                                drew_image: false,
                                feedback: PresenterFeedback::Failed,
                                used_stale_fallback: false,
                                slots: Vec::new(),
                            };
                            draw_viewer_outcome(
                                frame,
                                image_area,
                                &outcome,
                                loading_label.as_str(),
                                Some(render_target.as_str()),
                                viewer_has_image,
                                true,
                            );
                        }
                    }
                }
                Ok(None) => {
                    render_feedback = PresenterFeedback::Pending;
                    let outcome = match state.page_layout_mode {
                        PageLayoutMode::Single => PresenterRenderOutcome {
                            drew_image: false,
                            feedback: PresenterFeedback::Pending,
                            used_stale_fallback: false,
                            slots: Vec::new(),
                        },
                        PageLayoutMode::Spread => pending_spread_outcome(
                            spread_slot_areas,
                            visible_pages,
                            PresenterFeedback::Pending,
                        ),
                    };
                    let allow_viewer_loading = state.page_layout_mode == PageLayoutMode::Single;
                    if state.page_layout_mode == PageLayoutMode::Spread {
                        clear_pending_spread_regions(frame, spread_slot_areas, &outcome);
                    }
                    draw_viewer_outcome(
                        frame,
                        image_area,
                        &outcome,
                        loading_label.as_str(),
                        None,
                        viewer_has_image,
                        allow_viewer_loading,
                    );
                    if state.page_layout_mode == PageLayoutMode::Spread {
                        draw_spread_loading_overlays(frame, &outcome, visible_pages);
                    }
                }
                Err(err) => {
                    let _ = err;
                    render_failed = true;
                    let outcome = PresenterRenderOutcome {
                        drew_image: false,
                        feedback: PresenterFeedback::Failed,
                        used_stale_fallback: false,
                        slots: Vec::new(),
                    };
                    draw_viewer_outcome(
                        frame,
                        image_area,
                        &outcome,
                        loading_label.as_str(),
                        Some(render_target.as_str()),
                        viewer_has_image,
                        true,
                    );
                }
            }

            if let Some(view) = palette_view.as_ref() {
                ui::draw_palette_overlay(frame, image_area, view);
            }
            if state.mode == super::state::Mode::Help {
                ui::draw_help_overlay(frame, image_area, state.help_scroll, &help_keymap);
            }
        })?;
        state.pan_x = pan.cells_x;
        state.pan_y = pan.cells_y;
        self.runtime.sync_presenter_metrics(self.presenter.as_ref());
        self.viewer_has_image = viewer_has_image;
        self.image_occluded_last_frame = image_occluded;

        sync_render_notice(state, render_failed, render_feedback, &render_target);

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ViewerDisplayDecision {
    clear: bool,
    show_loading: bool,
    show_error: bool,
}

fn decide_viewer_display(
    outcome: &PresenterRenderOutcome,
    viewer_has_image: bool,
) -> ViewerDisplayDecision {
    let clear = !outcome.drew_image && !viewer_has_image;
    let mut show_loading = false;
    let mut show_error = false;
    match outcome.feedback {
        PresenterFeedback::None => {
            if clear {
                show_loading = true;
            }
        }
        PresenterFeedback::Pending => show_loading = true,
        PresenterFeedback::Failed => show_error = true,
    }
    ViewerDisplayDecision {
        clear,
        show_loading,
        show_error,
    }
}

fn normalize_render_outcome(
    render_mode: PresenterRenderMode,
    mut outcome: PresenterRenderOutcome,
) -> PresenterRenderOutcome {
    match render_mode {
        PresenterRenderMode::InitialPreview => {
            outcome.feedback = PresenterFeedback::Pending;
            for slot in &mut outcome.slots {
                if slot.active && slot.feedback == PresenterFeedback::None {
                    slot.feedback = PresenterFeedback::Pending;
                }
            }
            outcome
        }
        PresenterRenderMode::Full => outcome,
    }
}

fn pending_spread_outcome(
    slot_areas: SpreadSlotAreas,
    visible_pages: VisiblePageSlots,
    feedback: PresenterFeedback,
) -> PresenterRenderOutcome {
    PresenterRenderOutcome::aggregate_slots(vec![
        match visible_pages.left_page {
            Some(_) => PresenterSlotOutcome::active(slot_areas.left, false, feedback, false),
            None => PresenterSlotOutcome::inactive(slot_areas.left),
        },
        match visible_pages.right_page {
            Some(_) => PresenterSlotOutcome::active(slot_areas.right, false, feedback, false),
            None => PresenterSlotOutcome::inactive(slot_areas.right),
        },
    ])
}

fn spread_loading_overlays(
    outcome: &PresenterRenderOutcome,
    visible_pages: VisiblePageSlots,
) -> Vec<(ratatui::layout::Rect, String)> {
    let pages = [visible_pages.left_page, visible_pages.right_page];
    outcome
        .slots
        .iter()
        .zip(pages)
        .filter_map(|(slot, page)| {
            (slot.active && slot.feedback == PresenterFeedback::Pending)
                .then_some((slot.area, format_page_target(page?)))
        })
        .collect()
}

fn draw_spread_loading_overlays(
    frame: &mut ratatui::Frame<'_>,
    outcome: &PresenterRenderOutcome,
    visible_pages: VisiblePageSlots,
) {
    for (area, label) in spread_loading_overlays(outcome, visible_pages) {
        ui::draw_loading_overlay(frame, area, &label);
    }
}

fn draw_viewer_outcome(
    frame: &mut ratatui::Frame<'_>,
    image_area: ratatui::layout::Rect,
    outcome: &PresenterRenderOutcome,
    loading_label: &str,
    render_target: Option<&str>,
    viewer_has_image: bool,
    allow_loading_overlay: bool,
) {
    let decision = decide_viewer_display(outcome, viewer_has_image);
    if decision.clear {
        frame.render_widget(Clear, image_area);
    }
    if allow_loading_overlay && decision.show_loading {
        ui::draw_loading_overlay(frame, image_area, loading_label);
    }
    if decision.show_error {
        let message = render_failure_message(render_target);
        ui::draw_error_overlay(frame, image_area, &message);
    }
}

fn render_failure_message(render_target: Option<&str>) -> String {
    match render_target {
        Some(target) => format!("Could not render {target}."),
        None => "Could not render the current page.".to_string(),
    }
}

fn sync_render_notice(
    state: &mut AppState,
    render_failed: bool,
    render_feedback: PresenterFeedback,
    render_target: &str,
) {
    if render_failed || render_feedback == PresenterFeedback::Failed {
        state.set_error_notice(render_failure_message(Some(render_target)));
        return;
    }
    state.clear_render_notice();
}

fn presenter_render_options(
    viewer_has_image: bool,
    render_mode: PresenterRenderMode,
    image_occluded: bool,
    force_image_redraw: bool,
) -> PresenterRenderOptions {
    let mut options = PresenterRenderOptions::new(viewer_has_image, render_mode);
    options.preserve_stable_image = true;
    options.force_image_redraw = force_image_redraw || image_occluded && !viewer_has_image;
    options
}

fn resolve_layout_dimensions(
    pdf: &dyn PdfBackend,
    mode: PageLayoutMode,
    slots: VisiblePageSlots,
) -> (f32, f32) {
    let (anchor_width, anchor_height) = pdf
        .page_dimensions(slots.anchor_page)
        .unwrap_or(DEFAULT_PAGE_SIZE_PT);
    match slots.trailing_page {
        None => match mode {
            PageLayoutMode::Single => (anchor_width, anchor_height),
            // Tail spread still reserves a blank partner slot, so the scale
            // stays consistent with regular spread slot layout.
            PageLayoutMode::Spread => (anchor_width + anchor_width, anchor_height),
        },
        Some(trailing_page) => {
            let (trailing_width, trailing_height) = pdf
                .page_dimensions(trailing_page)
                .unwrap_or((anchor_width, anchor_height));
            let slot_width = anchor_width.max(trailing_width);
            (slot_width + slot_width, anchor_height.max(trailing_height))
        }
    }
}

pub(super) fn compute_initial_preview_plan(
    doc_id: u64,
    visible_pages: VisiblePageSlots,
    page_layout_mode: PageLayoutMode,
    current_scale: f32,
) -> Option<InitialPreviewPlan> {
    let preview_scale = quantize_scale(current_scale * INITIAL_PREVIEW_SCALE_RATIO);
    if scale_eq(preview_scale, current_scale) {
        return None;
    }

    let page_keys = match page_layout_mode {
        PageLayoutMode::Single => vec![RenderedPageKey::new(
            doc_id,
            visible_pages.anchor_page,
            preview_scale,
        )],
        PageLayoutMode::Spread => visible_pages
            .existing_pages()
            .into_iter()
            .flatten()
            .map(|page| RenderedPageKey::new(doc_id, page, preview_scale))
            .collect(),
    };
    let presenter_key = page_keys[0];

    Some(InitialPreviewPlan {
        scale: preview_scale,
        page_keys,
        presenter_key,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpreadSlotAreas {
    left: Rect,
    gap: Rect,
    right: Rect,
}

impl SpreadSlotAreas {
    fn page_slots(self, visible_pages: VisiblePageSlots) -> [(Option<usize>, Viewport); 2] {
        [
            (visible_pages.left_page, viewport_from_rect(self.left)),
            (visible_pages.right_page, viewport_from_rect(self.right)),
        ]
    }

    fn render_slots_for_pages(
        self,
        visible_pages: VisiblePageSlots,
        options: PresenterRenderOptions,
    ) -> Vec<PresenterRenderSlot> {
        vec![
            PresenterRenderSlot {
                area: self.left,
                options,
                active: visible_pages.left_page.is_some(),
                horizontal_align: PresenterHorizontalAlign::End,
            },
            PresenterRenderSlot {
                area: self.right,
                options,
                active: visible_pages.right_page.is_some(),
                horizontal_align: PresenterHorizontalAlign::Start,
            },
        ]
    }

    fn clear_gap(self, frame: &mut ratatui::Frame<'_>) {
        if self.gap.width > 0 && self.gap.height > 0 {
            frame.render_widget(Clear, self.gap);
        }
    }
}

fn clear_pending_spread_regions(
    frame: &mut ratatui::Frame<'_>,
    slot_areas: SpreadSlotAreas,
    outcome: &PresenterRenderOutcome,
) {
    slot_areas.clear_gap(frame);
    for slot in &outcome.slots {
        if !slot.active && slot.area.width > 0 && slot.area.height > 0 {
            frame.render_widget(Clear, slot.area);
        }
    }
}

fn split_spread_slot_areas(area: Rect, gap_cells: u16) -> SpreadSlotAreas {
    let gap = gap_cells.min(area.width);
    let content_width = area.width.saturating_sub(gap);
    let left_width = content_width / 2;
    let right_width = content_width.saturating_sub(left_width);
    let right_x = area.x.saturating_add(left_width).saturating_add(gap);
    let gap_x = area.x.saturating_add(left_width);
    SpreadSlotAreas {
        left: Rect::new(area.x, area.y, left_width, area.height),
        gap: Rect::new(gap_x, area.y, gap, area.height),
        right: Rect::new(right_x, area.y, right_width, area.height),
    }
}

fn viewport_from_rect(area: Rect) -> Viewport {
    Viewport {
        x: area.x,
        y: area.y,
        width: area.width.max(1),
        height: area.height.max(1),
    }
}

fn render_areas_to_slots(
    render_areas: [Option<Rect>; 2],
    render_mode: PresenterRenderMode,
) -> Vec<PresenterRenderSlot> {
    let options = PresenterRenderOptions::new(false, render_mode);
    render_areas
        .into_iter()
        .map(|area| PresenterRenderSlot {
            area: area.unwrap_or_default(),
            options,
            active: area.is_some(),
            horizontal_align: PresenterHorizontalAlign::Start,
        })
        .collect()
}

fn format_page_target(page: usize) -> String {
    format!("p.{}", page + 1)
}

fn format_loading_target(slots: VisiblePageSlots) -> String {
    match slots.trailing_page {
        Some(trailing) => format!("pp.{}-{}", slots.anchor_page + 1, trailing + 1),
        None => format_page_target(slots.anchor_page),
    }
}

fn format_render_target(slots: VisiblePageSlots) -> String {
    match slots.trailing_page {
        Some(trailing) => format!("pp.{}-{}", slots.anchor_page + 1, trailing + 1),
        None => format!("p.{}", slots.anchor_page + 1),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        InitialPreviewPlan, SpreadSlotAreas, ViewerDisplayDecision, compute_initial_preview_plan,
        decide_viewer_display, format_loading_target, format_render_target,
        normalize_render_outcome, pending_spread_outcome, presenter_render_options,
        render_areas_to_slots, render_failure_message, resolve_layout_dimensions,
        split_spread_slot_areas, spread_loading_overlays, sync_render_notice,
    };
    use crate::app::{AppState, PageLayoutMode, VisiblePageSlots};
    use crate::backend::{PdfBackend, RgbaFrame, TextPage};
    use crate::presenter::{
        PresenterFeedback, PresenterHorizontalAlign, PresenterRenderMode, PresenterRenderOutcome,
        PresenterSlotOutcome,
    };
    use crate::render::cache::RenderedPageKey;
    use ratatui::layout::Rect;

    struct DimPdf {
        path: PathBuf,
        dims: Vec<(f32, f32)>,
    }

    fn render_outcome(
        drew_image: bool,
        feedback: PresenterFeedback,
        used_stale_fallback: bool,
    ) -> PresenterRenderOutcome {
        PresenterRenderOutcome {
            drew_image,
            feedback,
            used_stale_fallback,
            slots: Vec::new(),
        }
    }

    impl DimPdf {
        fn new(dims: Vec<(f32, f32)>) -> Self {
            Self {
                path: PathBuf::from("dims.pdf"),
                dims,
            }
        }
    }

    impl PdfBackend for DimPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            1
        }

        fn page_count(&self) -> usize {
            self.dims.len()
        }

        fn page_dimensions(&self, page: usize) -> crate::error::AppResult<(f32, f32)> {
            self.dims
                .get(page)
                .copied()
                .ok_or(crate::error::AppError::invalid_argument("out of range"))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> crate::error::AppResult<RgbaFrame> {
            Ok(RgbaFrame {
                width: 1,
                height: 1,
                pixels: vec![0_u8; 4].into(),
            })
        }

        fn extract_text(&self, _page: usize) -> crate::error::AppResult<String> {
            Ok(String::new())
        }

        fn extract_positioned_text(&self, _page: usize) -> crate::error::AppResult<TextPage> {
            Ok(TextPage {
                width_pt: 612.0,
                height_pt: 792.0,
                glyphs: Vec::new(),
                dropped_glyphs: 0,
            })
        }

        fn extract_outline(&self) -> crate::error::AppResult<Vec<crate::backend::OutlineNode>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn display_decision_clears_when_no_image_drawn() {
        let outcome = render_outcome(false, PresenterFeedback::None, false);
        let decision = decide_viewer_display(&outcome, false);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: true,
                show_loading: true,
                show_error: false,
            }
        );
    }

    #[test]
    fn normalize_render_outcome_keeps_loading_feedback_for_initial_preview() {
        let outcome = normalize_render_outcome(
            PresenterRenderMode::InitialPreview,
            PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::None,
                used_stale_fallback: true,
                slots: vec![PresenterSlotOutcome::active(
                    Rect::new(2, 3, 10, 5),
                    true,
                    PresenterFeedback::None,
                    true,
                )],
            },
        );

        assert!(outcome.drew_image);
        assert_eq!(outcome.feedback, PresenterFeedback::Pending);
        assert!(outcome.used_stale_fallback);
        assert_eq!(outcome.slots[0].feedback, PresenterFeedback::Pending);
    }

    #[test]
    fn normalize_render_outcome_keeps_full_feedback_unchanged() {
        let outcome = normalize_render_outcome(
            PresenterRenderMode::Full,
            PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::Failed,
                used_stale_fallback: true,
                slots: Vec::new(),
            },
        );

        assert!(outcome.drew_image);
        assert_eq!(outcome.feedback, PresenterFeedback::Failed);
        assert!(outcome.used_stale_fallback);
    }

    #[test]
    fn display_decision_overlays_loading_on_pending_stale_fallback() {
        let outcome = render_outcome(true, PresenterFeedback::Pending, true);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }

    #[test]
    fn display_decision_overlays_loading_on_pending_fresh_image() {
        let outcome = render_outcome(true, PresenterFeedback::Pending, false);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }

    #[test]
    fn display_decision_overlays_error_on_failed_image() {
        let outcome = render_outcome(true, PresenterFeedback::Failed, true);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: false,
                show_error: true,
            }
        );
    }

    #[test]
    fn display_decision_clears_and_loading_for_pending_without_image() {
        let outcome = render_outcome(false, PresenterFeedback::Pending, false);
        let decision = decide_viewer_display(&outcome, false);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: true,
                show_loading: true,
                show_error: false,
            }
        );
    }

    #[test]
    fn display_decision_clears_and_error_for_failed_without_image() {
        let outcome = render_outcome(false, PresenterFeedback::Failed, false);
        let decision = decide_viewer_display(&outcome, false);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: true,
                show_loading: false,
                show_error: true,
            }
        );
    }

    #[test]
    fn display_decision_overlays_loading_when_pending_without_drawn_image() {
        let outcome = render_outcome(false, PresenterFeedback::Pending, false);
        let decision = decide_viewer_display(&outcome, true);
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }

    #[test]
    fn spread_loading_overlays_selects_pending_slots_with_page_labels() {
        let left_area = Rect::new(0, 1, 10, 8);
        let right_area = Rect::new(12, 1, 10, 8);
        let visible_pages = VisiblePageSlots {
            anchor_page: 10,
            trailing_page: Some(11),
            left_page: Some(10),
            right_page: Some(11),
        };
        let outcome = PresenterRenderOutcome::aggregate_slots(vec![
            PresenterSlotOutcome::active(left_area, false, PresenterFeedback::Pending, false),
            PresenterSlotOutcome::active(right_area, true, PresenterFeedback::Pending, true),
        ]);

        assert_eq!(
            spread_loading_overlays(&outcome, visible_pages),
            vec![
                (left_area, "p.11".to_string()),
                (right_area, "p.12".to_string())
            ]
        );
    }

    #[test]
    fn pending_spread_outcome_uses_slot_loading_areas_from_first_pending_frame() {
        let slot_areas = SpreadSlotAreas {
            left: Rect::new(0, 1, 10, 8),
            gap: Rect::new(10, 1, 2, 8),
            right: Rect::new(12, 1, 10, 8),
        };
        let visible_pages = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };

        let outcome = pending_spread_outcome(slot_areas, visible_pages, PresenterFeedback::Pending);

        assert!(!outcome.drew_image);
        assert_eq!(outcome.feedback, PresenterFeedback::Pending);
        assert_eq!(
            spread_loading_overlays(&outcome, visible_pages),
            vec![
                (slot_areas.left, "p.1".to_string()),
                (slot_areas.right, "p.2".to_string())
            ]
        );
    }

    #[test]
    fn spread_loading_overlays_ignores_fresh_ready_slots() {
        let visible_pages = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };
        let outcome = PresenterRenderOutcome::aggregate_slots(vec![
            PresenterSlotOutcome::active(
                Rect::new(0, 1, 10, 8),
                true,
                PresenterFeedback::None,
                false,
            ),
            PresenterSlotOutcome::active(
                Rect::new(12, 1, 10, 8),
                true,
                PresenterFeedback::None,
                false,
            ),
        ]);

        assert!(spread_loading_overlays(&outcome, visible_pages).is_empty());
    }

    #[test]
    fn spread_loading_overlays_ignores_inactive_tail_slot() {
        let visible_pages = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };
        let outcome = PresenterRenderOutcome::aggregate_slots(vec![
            PresenterSlotOutcome::active(
                Rect::new(0, 1, 10, 8),
                true,
                PresenterFeedback::None,
                false,
            ),
            PresenterSlotOutcome::inactive(Rect::new(12, 1, 10, 8)),
        ]);

        assert!(spread_loading_overlays(&outcome, visible_pages).is_empty());
        assert_eq!(outcome.feedback, PresenterFeedback::None);
    }

    #[test]
    fn sync_render_notice_clears_stale_render_error_after_success() {
        let mut app = AppState::default();
        app.set_error_notice("Could not render p.12.");

        sync_render_notice(&mut app, false, PresenterFeedback::None, "p.12");

        assert!(app.notice.is_none());
    }

    #[test]
    fn sync_render_notice_clears_stale_render_error_while_pending() {
        let mut app = AppState::default();
        app.set_error_notice("Could not render p.12.");

        sync_render_notice(&mut app, false, PresenterFeedback::Pending, "p.12");

        assert!(app.notice.is_none());
    }

    #[test]
    fn sync_render_notice_preserves_non_render_notice() {
        let mut app = AppState::default();
        app.set_error_notice("search failed: backend failed");

        sync_render_notice(&mut app, false, PresenterFeedback::None, "p.12");

        assert_eq!(
            app.notice.as_ref().map(|notice| notice.message.as_str()),
            Some("search failed: backend failed")
        );
    }

    #[test]
    fn render_failure_message_uses_single_page_label() {
        assert_eq!(
            render_failure_message(Some("p.12")),
            "Could not render p.12."
        );
    }

    #[test]
    fn render_failure_message_uses_spread_label() {
        assert_eq!(
            render_failure_message(Some("pp.12-13")),
            "Could not render pp.12-13."
        );
    }

    #[test]
    fn render_failure_message_falls_back_to_current_page() {
        assert_eq!(
            render_failure_message(None),
            "Could not render the current page."
        );
    }

    #[test]
    fn presenter_render_options_derive_stale_fallback_from_viewer_image_state() {
        let with_image = presenter_render_options(true, PresenterRenderMode::Full, false, false);
        let without_image =
            presenter_render_options(false, PresenterRenderMode::InitialPreview, false, false);

        assert!(with_image.allow_stale_fallback);
        assert!(!without_image.allow_stale_fallback);
        assert!(with_image.preserve_stable_image);
        assert!(!with_image.force_image_redraw);
        assert_eq!(with_image.render_mode, PresenterRenderMode::Full);
        assert_eq!(
            without_image.render_mode,
            PresenterRenderMode::InitialPreview
        );
    }

    #[test]
    fn presenter_render_options_force_redraw_after_occlusion() {
        let after_overlay = presenter_render_options(true, PresenterRenderMode::Full, false, true);
        assert!(after_overlay.force_image_redraw);
    }

    #[test]
    fn resolve_layout_dimensions_uses_blank_partner_width_for_tail_spread() {
        let pdf = DimPdf::new(vec![(200.0, 300.0)]);
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let single = resolve_layout_dimensions(&pdf, PageLayoutMode::Single, slots);
        let spread = resolve_layout_dimensions(&pdf, PageLayoutMode::Spread, slots);

        assert_eq!(single, (200.0, 300.0));
        assert_eq!(spread, (400.0, 300.0));
    }

    #[test]
    fn resolve_layout_dimensions_uses_both_pages_when_trailing_exists() {
        let pdf = DimPdf::new(vec![(200.0, 300.0), (180.0, 280.0)]);
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };

        let spread = resolve_layout_dimensions(&pdf, PageLayoutMode::Spread, slots);
        assert_eq!(spread, (400.0, 300.0));
    }

    #[test]
    fn split_spread_slot_areas_preserves_gap_and_stable_widths() {
        let slots = split_spread_slot_areas(Rect::new(10, 2, 41, 20), 3);

        assert_eq!(slots.left, Rect::new(10, 2, 19, 20));
        assert_eq!(slots.gap, Rect::new(29, 2, 3, 20));
        assert_eq!(slots.right, Rect::new(32, 2, 19, 20));
        assert_eq!(slots.right.x - (slots.left.x + slots.left.width), 3);
    }

    #[test]
    fn render_areas_to_slots_preserves_offscreen_spread_slot_positions() {
        let right_area = Rect::new(4, 2, 12, 8);

        let slots = render_areas_to_slots([None, Some(right_area)], PresenterRenderMode::Full);

        assert_eq!(slots.len(), 2);
        assert!(!slots[0].active);
        assert_eq!(slots[0].area, Rect::default());
        assert!(slots[1].active);
        assert_eq!(slots[1].area, right_area);
        assert_eq!(slots[1].horizontal_align, PresenterHorizontalAlign::Start);
    }

    #[test]
    fn compute_initial_preview_plan_uses_lower_scale_on_cold_start() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview = compute_initial_preview_plan(7, slots, PageLayoutMode::Single, 1.0);

        assert_eq!(
            preview,
            Some(InitialPreviewPlan {
                scale: 0.25,
                page_keys: vec![RenderedPageKey::new(7, 0, 0.25)],
                presenter_key: RenderedPageKey::new(7, 0, 0.25),
            })
        );
    }

    #[test]
    fn compute_initial_preview_plan_includes_both_spread_pages() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };

        let preview = compute_initial_preview_plan(7, slots, PageLayoutMode::Spread, 1.0);

        assert_eq!(
            preview,
            Some(InitialPreviewPlan {
                scale: 0.25,
                page_keys: vec![
                    RenderedPageKey::new(7, 0, 0.25),
                    RenderedPageKey::new(7, 1, 0.25),
                ],
                presenter_key: RenderedPageKey::new(7, 0, 0.25),
            })
        );
    }

    #[test]
    fn compute_initial_preview_plan_handles_tail_spread() {
        let slots = VisiblePageSlots {
            anchor_page: 2,
            trailing_page: None,
            left_page: Some(2),
            right_page: None,
        };

        let preview = compute_initial_preview_plan(7, slots, PageLayoutMode::Spread, 1.0);

        assert_eq!(
            preview,
            Some(InitialPreviewPlan {
                scale: 0.25,
                page_keys: vec![RenderedPageKey::new(7, 2, 0.25)],
                presenter_key: RenderedPageKey::new(7, 2, 0.25),
            })
        );
    }

    #[test]
    fn loading_target_formats_single_page_with_p_prefix() {
        let label = format_loading_target(VisiblePageSlots {
            anchor_page: 11,
            trailing_page: None,
            left_page: Some(11),
            right_page: None,
        });

        assert_eq!(label, "p.12");
    }

    #[test]
    fn loading_target_formats_spread_with_pp_prefix() {
        let label = format_loading_target(VisiblePageSlots {
            anchor_page: 11,
            trailing_page: Some(12),
            left_page: Some(11),
            right_page: Some(12),
        });

        assert_eq!(label, "pp.12-13");
    }

    #[test]
    fn render_target_uses_error_label_convention() {
        let label = format_render_target(VisiblePageSlots {
            anchor_page: 11,
            trailing_page: Some(12),
            left_page: Some(11),
            right_page: Some(12),
        });

        assert_eq!(label, "pp.12-13");
    }
}
