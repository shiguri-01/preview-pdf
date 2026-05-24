use crate::app::PageLayoutMode;
use crate::backend::PdfBackend;
use crate::config::Config;
use crate::error::AppResult;
use crate::highlight::HighlightOverlaySnapshot;
use crate::input::sequence::SequenceRegistrySnapshot;
use crate::palette::PaletteView;
use crate::presenter::{
    PanOffset, PresenterFeedback, PresenterHorizontalAlign, PresenterRenderMode,
    PresenterRenderOptions, PresenterRenderOutcome, PresenterRenderSlot, PresenterRuntimeInfo,
    PresenterSlotOutcome, Viewport,
};
use crate::render::cache::RenderedPageKey;
use crate::ui;
use ratatui::layout::Rect;

mod spread;
mod viewer_outcome;

use spread::{
    SpreadSlotAreas, clear_pending_spread_regions, format_loading_target, format_render_target,
    render_areas_to_slots, split_spread_slot_areas,
};
use viewer_outcome::{
    draw_spread_loading_overlays, draw_viewer_outcome, normalize_render_outcome,
    pending_spread_outcome, presenter_render_options, sync_render_notice,
};

use super::constants::DEFAULT_PAGE_SIZE_PT;
use super::core::{App, RenderSubsystem};
use super::runtime::{
    CachePrepareResult, FramePrepareOptions, PageSlotPrepareRequest, SpreadCanvasPrepareRequest,
};
use super::scale::{
    compute_render_scale, compute_scale, quantize_scale, resolved_cell_size_px, scale_eq,
};
use super::state::{AppState, Mode, VisiblePageSlots};
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

struct RenderFrameDrawPlan {
    palette_view: Option<PaletteView>,
    help_keymap: SequenceRegistrySnapshot,
    status_bar_segments: Vec<String>,
    page_count: usize,
    visible_pages: VisiblePageSlots,
    current_scale: f32,
    initial_preview: Option<InitialPreviewPlan>,
    highlight_overlay: HighlightOverlaySnapshot,
    generation: u64,
    mode: Mode,
    help_scroll: usize,
    debug_status_visible: bool,
    chrome: ui::ChromeViewState,
    page_presentation: PageLayoutMode,
    enable_crop: bool,
    file_name: String,
    presenter_backend_name: &'static str,
    presenter_runtime: PresenterRuntimeInfo,
    presenter_cell_px: Option<(u16, u16)>,
    render_options: PresenterRenderOptions,
    pan: PanOffset,
    image_occluded: bool,
    loading_label: String,
    render_target: String,
    spread_gap_px: u32,
}

struct RenderFramePresenterInfo {
    backend_name: &'static str,
    runtime: PresenterRuntimeInfo,
    cell_px: Option<(u16, u16)>,
}

struct RenderFrameFeedback {
    pan: PanOffset,
    render_failed: bool,
    render_feedback: PresenterFeedback,
    viewer_has_image: bool,
    image_occluded: bool,
    render_target: String,
}

struct SinglePagePrepareRequest<'a> {
    pdf: &'a dyn PdfBackend,
    viewport: Viewport,
    page: usize,
    full_scale: f32,
    initial_preview: Option<&'a InitialPreviewPlan>,
    pan: &'a mut PanOffset,
    cell_px: Option<(u16, u16)>,
    enable_crop: bool,
    highlight_overlay: &'a HighlightOverlaySnapshot,
    generation: u64,
}

struct SpreadPrepareRequest<'a> {
    pdf: &'a dyn PdfBackend,
    viewport: Viewport,
    visible_pages: VisiblePageSlots,
    slot_areas: SpreadSlotAreas,
    full_scale: f32,
    initial_preview: Option<&'a InitialPreviewPlan>,
    pan: &'a mut PanOffset,
    cell_px: Option<(u16, u16)>,
    enable_crop: bool,
    highlight_overlay: &'a HighlightOverlaySnapshot,
    generation: u64,
    spread_gap_px: u32,
}

struct SpreadCachePrepareRequest<'a> {
    pdf: &'a dyn PdfBackend,
    viewport: Viewport,
    visible_pages: VisiblePageSlots,
    slot_areas: SpreadSlotAreas,
    scale: f32,
    pan: &'a mut PanOffset,
    cell_px: Option<(u16, u16)>,
    enable_crop: bool,
    highlight_overlay: &'a HighlightOverlaySnapshot,
    generation: u64,
    spread_gap_px: u32,
    render_mode: PresenterRenderMode,
}

impl RenderFrameDrawPlan {
    fn new(
        state: &AppState,
        pdf: &dyn PdfBackend,
        plan: RenderFramePlan,
        viewer_has_image: bool,
        image_occluded_last_frame: bool,
        presenter: RenderFramePresenterInfo,
    ) -> Self {
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
        let image_occluded = palette_view.is_some() || state.mode == Mode::Help;
        let render_options = presenter_render_options(
            viewer_has_image,
            PresenterRenderMode::Full,
            image_occluded,
            image_occluded_last_frame && !image_occluded,
        );
        let file_name = pdf
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| pdf.path().display().to_string());
        let loading_label = format_loading_target(visible_pages);
        let render_target = format_render_target(visible_pages);
        let page_presentation = state.page_presentation_for_slots(visible_pages);
        let spread_gap_px = u32::from(
            resolved_cell_size_px(presenter.cell_px)
                .0
                .saturating_mul(SPREAD_GAP_CELLS),
        );

        Self {
            palette_view,
            help_keymap,
            status_bar_segments,
            page_count,
            visible_pages,
            current_scale,
            initial_preview,
            highlight_overlay,
            generation,
            mode: state.mode,
            help_scroll: state.help_scroll,
            debug_status_visible: state.debug_status_visible,
            chrome: ui::ChromeViewState {
                visible_pages,
                page_presentation,
                zoom: state.zoom,
                debug_status_visible: state.debug_status_visible,
                notice: state.notice.clone(),
            },
            page_presentation,
            enable_crop: state.zoom > 1.0,
            file_name,
            presenter_backend_name: presenter.backend_name,
            presenter_runtime: presenter.runtime,
            presenter_cell_px: presenter.cell_px,
            render_options,
            pan: PanOffset {
                cells_x: state.pan_x,
                cells_y: state.pan_y,
            },
            image_occluded,
            loading_label,
            render_target,
            spread_gap_px,
        }
    }
}

pub(super) fn current_viewport_for_session<S: TerminalSurface>(
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

pub(super) fn compute_current_scale_for_state(
    state: &AppState,
    render: &RenderSubsystem,
    config: &Config,
    pdf: &dyn PdfBackend,
    page: usize,
    viewport: Option<Viewport>,
) -> f32 {
    let Some(viewport) = viewport else {
        return quantize_scale(state.zoom);
    };

    let slots = state.visible_page_slots_for_page(page, pdf.page_count());
    let page_presentation = state.page_presentation_for_slots(slots);
    let (page_width_pt, page_height_pt) = resolve_layout_dimensions(pdf, page_presentation, slots);
    let caps = render.presenter.capabilities();
    let max_scale = caps
        .preferred_max_render_scale
        .clamp(1.0, config.render.max_render_scale);
    let render_scale = compute_render_scale(
        viewport,
        caps.cell_px,
        page_width_pt,
        page_height_pt,
        max_scale,
    );
    compute_scale(state.zoom, render_scale)
}

impl App {
    pub(super) fn current_viewport<S: TerminalSurface>(
        session: &S,
        debug_status_visible: bool,
    ) -> Option<Viewport> {
        current_viewport_for_session(session, debug_status_visible)
    }

    pub(super) fn compute_current_scale(
        &self,
        pdf: &dyn PdfBackend,
        page: usize,
        viewport: Option<Viewport>,
    ) -> f32 {
        compute_current_scale_for_state(
            &self.state,
            &self.render,
            &self.config,
            pdf,
            page,
            viewport,
        )
    }

    pub(super) fn current_pan(&self) -> PanOffset {
        PanOffset {
            cells_x: self.state.pan_x,
            cells_y: self.state.pan_y,
        }
    }
}

impl RenderSubsystem {
    fn prepare_single_page_or_preview_from_cache(
        &mut self,
        request: SinglePagePrepareRequest<'_>,
    ) -> AppResult<Option<(PresenterRenderMode, Vec<PresenterRenderSlot>)>> {
        let attempts = request.initial_preview.map_or_else(
            || {
                vec![(
                    PresenterRenderMode::Full,
                    RenderedPageKey::new(request.pdf.doc_id(), request.page, request.full_scale),
                )]
            },
            |preview| {
                vec![
                    (
                        PresenterRenderMode::Full,
                        RenderedPageKey::new(
                            request.pdf.doc_id(),
                            request.page,
                            request.full_scale,
                        ),
                    ),
                    (PresenterRenderMode::InitialPreview, preview.page_keys[0]),
                ]
            },
        );
        for (render_mode, key) in attempts {
            let page_slots = [(Some(key), request.viewport)];
            let result = self.runtime.prepare_page_slots_from_cache(
                request.pdf,
                PageSlotPrepareRequest {
                    page_slots: &page_slots,
                    pan: *request.pan,
                    options: FramePrepareOptions {
                        cell_px: request.cell_px,
                        crop: request.enable_crop,
                        overlay: request.highlight_overlay,
                    },
                },
            )?;
            if let CachePrepareResult::Prepared(prepared) = result {
                *request.pan = prepared.pan();
                prepared.prepare_into(self.presenter.as_mut(), request.generation)?;
                return Ok(Some((
                    render_mode,
                    vec![PresenterRenderSlot {
                        area: Rect::new(
                            request.viewport.x,
                            request.viewport.y,
                            request.viewport.width,
                            request.viewport.height,
                        ),
                        options: PresenterRenderOptions::new(false, render_mode),
                        active: true,
                        horizontal_align: PresenterHorizontalAlign::Center,
                    }],
                )));
            }
        }

        Ok(None)
    }

    fn prepare_spread_or_preview_from_cache(
        &mut self,
        request: SpreadPrepareRequest<'_>,
    ) -> AppResult<Option<(PresenterRenderMode, Vec<PresenterRenderSlot>)>> {
        let attempts = request.initial_preview.map_or_else(
            || vec![(PresenterRenderMode::Full, request.full_scale)],
            |preview| {
                vec![
                    (PresenterRenderMode::Full, request.full_scale),
                    (PresenterRenderMode::InitialPreview, preview.scale),
                ]
            },
        );
        for (render_mode, scale) in attempts {
            if let Some(render_slots) =
                self.try_prepare_spread_slots_from_cache(SpreadCachePrepareRequest {
                    pdf: request.pdf,
                    viewport: request.viewport,
                    visible_pages: request.visible_pages,
                    slot_areas: request.slot_areas,
                    scale,
                    pan: &mut *request.pan,
                    cell_px: request.cell_px,
                    enable_crop: request.enable_crop,
                    highlight_overlay: request.highlight_overlay,
                    generation: request.generation,
                    spread_gap_px: request.spread_gap_px,
                    render_mode,
                })?
            {
                return Ok(Some((render_mode, render_slots)));
            }
        }

        Ok(None)
    }

    fn try_prepare_spread_slots_from_cache(
        &mut self,
        request: SpreadCachePrepareRequest<'_>,
    ) -> AppResult<Option<Vec<PresenterRenderSlot>>> {
        if request.enable_crop {
            let result = self.runtime.prepare_spread_canvas_from_cache(
                request.pdf,
                SpreadCanvasPrepareRequest {
                    viewport: request.viewport,
                    visible_pages: request.visible_pages,
                    scale: request.scale,
                    pan: *request.pan,
                    cell_px: request.cell_px,
                    overlay: request.highlight_overlay,
                    gap_px: request.spread_gap_px,
                },
            )?;
            return match result {
                CachePrepareResult::Prepared(prepared) => {
                    *request.pan = prepared.pan();
                    let areas = prepared.render_areas();
                    prepared.prepare_into(self.presenter.as_mut(), request.generation)?;
                    Ok(Some(render_areas_to_slots(areas, request.render_mode)))
                }
                CachePrepareResult::Miss {
                    pan: normalized_pan,
                } => {
                    *request.pan = normalized_pan;
                    Ok(None)
                }
            };
        }

        let page_slots = request.slot_areas.page_slots(
            request.pdf.doc_id(),
            request.visible_pages,
            request.scale,
        );
        let result = self.runtime.prepare_page_slots_from_cache(
            request.pdf,
            PageSlotPrepareRequest {
                page_slots: &page_slots,
                pan: *request.pan,
                options: FramePrepareOptions {
                    cell_px: request.cell_px,
                    crop: false,
                    overlay: request.highlight_overlay,
                },
            },
        )?;
        if let CachePrepareResult::Prepared(prepared) = result {
            *request.pan = prepared.pan();
            prepared.prepare_into(self.presenter.as_mut(), request.generation)?;
            let options = PresenterRenderOptions::new(false, request.render_mode);
            return Ok(Some(
                request
                    .slot_areas
                    .render_slots_for_pages(request.visible_pages, options),
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
        let presenter_caps = self.presenter.capabilities();
        let draw_plan = RenderFrameDrawPlan::new(
            state,
            pdf,
            plan,
            self.viewer_has_image,
            self.image_occluded_last_frame,
            RenderFramePresenterInfo {
                backend_name: presenter_caps.backend_name,
                runtime: self.presenter.runtime_info(),
                cell_px: presenter_caps.cell_px,
            },
        );
        let feedback = self.draw_render_frame(session, pdf, draw_plan)?;
        self.apply_render_frame_feedback(state, feedback);

        Ok(())
    }

    fn draw_render_frame(
        &mut self,
        session: &mut impl TerminalSurface,
        pdf: &dyn PdfBackend,
        draw_plan: RenderFrameDrawPlan,
    ) -> AppResult<RenderFrameFeedback> {
        let mut pan = draw_plan.pan;
        let mut render_failed = false;
        let mut render_feedback = PresenterFeedback::None;
        let mut viewer_has_image = self.viewer_has_image;
        session.draw(|frame| {
            let layout = ui::split_layout(frame.area(), draw_plan.debug_status_visible);
            ui::draw_chrome(
                frame,
                layout,
                &draw_plan.chrome,
                &draw_plan.file_name,
                draw_plan.page_count,
                draw_plan.presenter_backend_name,
                draw_plan.presenter_runtime.graphics_protocol,
                &draw_plan.status_bar_segments,
            );

            let viewport = Viewport {
                x: layout.viewer_inner.x,
                y: layout.viewer_inner.y,
                width: layout.viewer_inner.width.max(1),
                height: layout.viewer_inner.height.max(1),
            };
            let image_area = layout.viewer_inner;
            let spread_slot_areas = split_spread_slot_areas(image_area, SPREAD_GAP_CELLS);

            let prepare_result = match draw_plan.page_presentation {
                PageLayoutMode::Single => {
                    self.prepare_single_page_or_preview_from_cache(SinglePagePrepareRequest {
                        pdf,
                        viewport,
                        page: draw_plan.visible_pages.anchor_page,
                        full_scale: draw_plan.current_scale,
                        initial_preview: draw_plan.initial_preview.as_ref(),
                        pan: &mut pan,
                        cell_px: draw_plan.presenter_cell_px,
                        enable_crop: draw_plan.enable_crop,
                        highlight_overlay: &draw_plan.highlight_overlay,
                        generation: draw_plan.generation,
                    })
                }
                PageLayoutMode::Spread => {
                    self.prepare_spread_or_preview_from_cache(SpreadPrepareRequest {
                        pdf,
                        viewport,
                        visible_pages: draw_plan.visible_pages,
                        slot_areas: spread_slot_areas,
                        full_scale: draw_plan.current_scale,
                        initial_preview: draw_plan.initial_preview.as_ref(),
                        pan: &mut pan,
                        cell_px: draw_plan.presenter_cell_px,
                        enable_crop: draw_plan.enable_crop,
                        highlight_overlay: &draw_plan.highlight_overlay,
                        generation: draw_plan.generation,
                        spread_gap_px: draw_plan.spread_gap_px,
                    })
                }
            };

            match prepare_result {
                Ok(Some((render_mode, spread_render_slots))) => {
                    let options = PresenterRenderOptions {
                        render_mode,
                        ..draw_plan.render_options
                    };
                    let render_result = match draw_plan.page_presentation {
                        PageLayoutMode::Single => {
                            let render_slots: Vec<_> = spread_render_slots
                                .into_iter()
                                .map(|slot| PresenterRenderSlot { options, ..slot })
                                .collect();
                            self.presenter.render_slots(frame, &render_slots)
                        }
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
                                draw_plan.page_presentation == PageLayoutMode::Single;
                            draw_viewer_outcome(
                                frame,
                                image_area,
                                &outcome,
                                draw_plan.loading_label.as_str(),
                                None,
                                viewer_has_image,
                                allow_viewer_loading,
                            );
                            if draw_plan.page_presentation == PageLayoutMode::Spread {
                                draw_spread_loading_overlays(
                                    frame,
                                    &outcome,
                                    draw_plan.visible_pages,
                                );
                            }
                        }
                        Err(err) => {
                            let _ = err;
                            render_failed = true;
                            let outcome = PresenterRenderOutcome::failed();
                            draw_viewer_outcome(
                                frame,
                                image_area,
                                &outcome,
                                draw_plan.loading_label.as_str(),
                                Some(draw_plan.render_target.as_str()),
                                viewer_has_image,
                                true,
                            );
                        }
                    }
                }
                Ok(None) => {
                    render_feedback = PresenterFeedback::Pending;
                    let outcome = match draw_plan.page_presentation {
                        PageLayoutMode::Single => PresenterRenderOutcome {
                            slots: vec![PresenterSlotOutcome::active(
                                image_area,
                                false,
                                PresenterFeedback::Pending,
                                false,
                            )],
                            ..PresenterRenderOutcome::pending()
                        },
                        PageLayoutMode::Spread => pending_spread_outcome(
                            spread_slot_areas,
                            draw_plan.visible_pages,
                            PresenterFeedback::Pending,
                        ),
                    };
                    let allow_viewer_loading =
                        draw_plan.page_presentation == PageLayoutMode::Single;
                    if draw_plan.page_presentation == PageLayoutMode::Spread {
                        clear_pending_spread_regions(frame, spread_slot_areas, &outcome);
                    }
                    draw_viewer_outcome(
                        frame,
                        image_area,
                        &outcome,
                        draw_plan.loading_label.as_str(),
                        None,
                        viewer_has_image,
                        allow_viewer_loading,
                    );
                    if draw_plan.page_presentation == PageLayoutMode::Spread {
                        draw_spread_loading_overlays(frame, &outcome, draw_plan.visible_pages);
                    }
                }
                Err(err) => {
                    let _ = err;
                    render_failed = true;
                    let outcome = PresenterRenderOutcome::failed();
                    draw_viewer_outcome(
                        frame,
                        image_area,
                        &outcome,
                        draw_plan.loading_label.as_str(),
                        Some(draw_plan.render_target.as_str()),
                        viewer_has_image,
                        true,
                    );
                }
            }

            if let Some(view) = draw_plan.palette_view.as_ref() {
                ui::draw_palette_overlay(frame, image_area, view);
            }
            if draw_plan.mode == Mode::Help {
                ui::draw_help_overlay(
                    frame,
                    image_area,
                    draw_plan.help_scroll,
                    &draw_plan.help_keymap,
                );
            }
        })?;

        Ok(RenderFrameFeedback {
            pan,
            render_failed,
            render_feedback,
            viewer_has_image,
            image_occluded: draw_plan.image_occluded,
            render_target: draw_plan.render_target,
        })
    }

    fn apply_render_frame_feedback(&mut self, state: &mut AppState, feedback: RenderFrameFeedback) {
        state.pan_x = feedback.pan.cells_x;
        state.pan_y = feedback.pan.cells_y;
        self.runtime.sync_presenter_metrics(self.presenter.as_ref());
        self.viewer_has_image = feedback.viewer_has_image;
        self.image_occluded_last_frame = feedback.image_occluded;

        sync_render_notice(
            state,
            feedback.render_failed,
            feedback.render_feedback,
            &feedback.render_target,
        );
    }
}

fn resolve_layout_dimensions(
    pdf: &dyn PdfBackend,
    page_presentation: PageLayoutMode,
    slots: VisiblePageSlots,
) -> (f32, f32) {
    let (anchor_width, anchor_height) = pdf
        .page_dimensions(slots.anchor_page)
        .unwrap_or(DEFAULT_PAGE_SIZE_PT);
    match slots.trailing_page {
        None => match page_presentation {
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
    page_presentation: PageLayoutMode,
    current_scale: f32,
) -> Option<InitialPreviewPlan> {
    let preview_scale = quantize_scale(current_scale * INITIAL_PREVIEW_SCALE_RATIO);
    if scale_eq(preview_scale, current_scale) {
        return None;
    }

    let page_keys = match page_presentation {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use crate::app::{PageLayoutMode, VisiblePageSlots};
    use crate::backend::{PdfBackend, RgbaFrame, TextPage};
    use crate::render::cache::RenderedPageKey;

    struct DimPdf {
        path: PathBuf,
        dims: Vec<(f32, f32)>,
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
}
