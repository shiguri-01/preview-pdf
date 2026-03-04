use ratatui::widgets::Clear;

use crate::app::PageLayoutMode;
use crate::backend::PdfBackend;
use crate::command::ActionId;
use crate::config::Config;
use crate::error::AppResult;
use crate::palette::PaletteView;
use crate::presenter::{
    PanOffset, PresenterFeedback, PresenterRenderOptions, PresenterRenderOutcome, Viewport,
};
use crate::render::cache::RenderedPageKey;
use crate::ui;

use super::constants::DEFAULT_PAGE_SIZE_PT;
use super::core::{App, RenderSubsystem};
use super::scale::{compute_render_scale, compute_scale, quantize_scale, resolved_cell_size_px};
use super::state::{AppState, VisiblePageSlots};
use super::terminal_session::TerminalSurface;

const SPREAD_GAP_CELLS: u16 = 2;

pub(super) struct RenderFramePlan {
    pub(super) palette_view: Option<PaletteView>,
    pub(super) status_bar_segments: Vec<String>,
    pub(super) page_count: usize,
    pub(super) visible_pages: VisiblePageSlots,
    pub(super) current_scale: f32,
    pub(super) presenter_key: RenderedPageKey,
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
        let (page_width_pt, page_height_pt) = resolve_layout_dimensions(pdf, slots);
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
            cells_x: self.state.scroll_x,
            cells_y: self.state.scroll_y,
        }
    }
}

impl RenderSubsystem {
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
            status_bar_segments,
            page_count,
            visible_pages,
            current_scale,
            presenter_key,
            generation,
            nav_streak: _nav_streak,
        } = plan;
        let allow_stale_fallback = false;
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
            cells_x: state.scroll_x,
            cells_y: state.scroll_y,
        };
        let mut render_error: Option<String> = None;
        let mut render_feedback = PresenterFeedback::None;
        let mut viewer_has_image = self.viewer_has_image;
        let loading_label = format_loading_target(visible_pages);
        let render_target = format_render_target(visible_pages, page_count);
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
                &self.runtime.perf_stats,
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

            let prepare_result = match state.page_layout_mode {
                PageLayoutMode::Single => self.runtime.try_prepare_current_page_from_cache(
                    pdf,
                    self.presenter.as_mut(),
                    viewport,
                    visible_pages.anchor_page,
                    current_scale,
                    &mut pan,
                    presenter_caps.cell_px,
                    enable_crop,
                    generation,
                ),
                PageLayoutMode::Spread => self.runtime.try_prepare_spread_from_cache(
                    pdf,
                    self.presenter.as_mut(),
                    viewport,
                    visible_pages,
                    presenter_key,
                    current_scale,
                    &mut pan,
                    presenter_caps.cell_px,
                    enable_crop,
                    generation,
                    spread_gap_px,
                ),
            };

            match prepare_result {
                Ok(true) => match self.presenter.render(
                    frame,
                    image_area,
                    PresenterRenderOptions {
                        allow_stale_fallback,
                    },
                ) {
                    Ok(outcome) => {
                        render_feedback = outcome.feedback;
                        if outcome.drew_image {
                            viewer_has_image = true;
                        }
                        draw_viewer_outcome(
                            frame,
                            image_area,
                            outcome,
                            loading_label.as_str(),
                            None,
                            viewer_has_image,
                        );
                    }
                    Err(err) => {
                        let message = err.to_string();
                        render_error = Some(message.clone());
                        let outcome = PresenterRenderOutcome {
                            drew_image: false,
                            feedback: PresenterFeedback::Failed,
                            used_stale_fallback: false,
                        };
                        draw_viewer_outcome(
                            frame,
                            image_area,
                            outcome,
                            loading_label.as_str(),
                            Some(&message),
                            viewer_has_image,
                        );
                    }
                },
                Ok(false) => {
                    render_feedback = PresenterFeedback::Pending;
                    let outcome = PresenterRenderOutcome {
                        drew_image: false,
                        feedback: PresenterFeedback::Pending,
                        used_stale_fallback: false,
                    };
                    draw_viewer_outcome(
                        frame,
                        image_area,
                        outcome,
                        loading_label.as_str(),
                        None,
                        viewer_has_image,
                    );
                }
                Err(err) => {
                    let message = err.to_string();
                    render_error = Some(message.clone());
                    let outcome = PresenterRenderOutcome {
                        drew_image: false,
                        feedback: PresenterFeedback::Failed,
                        used_stale_fallback: false,
                    };
                    draw_viewer_outcome(
                        frame,
                        image_area,
                        outcome,
                        loading_label.as_str(),
                        Some(&message),
                        viewer_has_image,
                    );
                }
            }

            if let Some(view) = palette_view.as_ref() {
                ui::draw_palette_overlay(frame, image_area, view);
            }
        })?;
        state.scroll_x = pan.cells_x;
        state.scroll_y = pan.cells_y;
        self.runtime.sync_presenter_metrics(self.presenter.as_ref());
        self.viewer_has_image = viewer_has_image;

        if let Some(err) = render_error {
            state.status.last_action_id = Some(ActionId::RenderPage);
            state.status.message = format!("render error: {err}");
        } else if render_feedback == PresenterFeedback::Failed {
            state.status.last_action_id = Some(ActionId::RenderPage);
            state.status.message = format!("render error: failed to render {render_target}");
        } else if render_feedback == PresenterFeedback::Pending {
            state.status.last_action_id = Some(ActionId::RenderPending);
            state.status.message = format!("rendering {render_target}...");
        }

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
    outcome: PresenterRenderOutcome,
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

fn draw_viewer_outcome(
    frame: &mut ratatui::Frame<'_>,
    image_area: ratatui::layout::Rect,
    outcome: PresenterRenderOutcome,
    loading_label: &str,
    error_message: Option<&str>,
    viewer_has_image: bool,
) {
    let decision = decide_viewer_display(outcome, viewer_has_image);
    if decision.clear {
        frame.render_widget(Clear, image_area);
    }
    if decision.show_loading {
        ui::draw_loading_overlay(frame, image_area, loading_label);
    }
    if decision.show_error {
        let message = error_message
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("Failed to render {loading_label}"));
        ui::draw_error_overlay(frame, image_area, &message);
    }
}

fn resolve_layout_dimensions(pdf: &dyn PdfBackend, slots: VisiblePageSlots) -> (f32, f32) {
    let (anchor_width, anchor_height) = pdf
        .page_dimensions(slots.anchor_page)
        .unwrap_or(DEFAULT_PAGE_SIZE_PT);
    match slots.trailing_page {
        None => (anchor_width, anchor_height),
        Some(trailing_page) => {
            let (trailing_width, trailing_height) = pdf
                .page_dimensions(trailing_page)
                .unwrap_or((anchor_width, anchor_height));
            (
                anchor_width + trailing_width,
                anchor_height.max(trailing_height),
            )
        }
    }
}

fn format_loading_target(slots: VisiblePageSlots) -> String {
    match slots.trailing_page {
        Some(trailing) => format!("pages {}-{}", slots.anchor_page + 1, trailing + 1),
        None => format!("page {}", slots.anchor_page + 1),
    }
}

fn format_render_target(slots: VisiblePageSlots, page_count: usize) -> String {
    let total = page_count.max(1);
    match slots.trailing_page {
        Some(trailing) => format!("pages {}-{}/{}", slots.anchor_page + 1, trailing + 1, total),
        None => format!("page {}/{}", slots.anchor_page + 1, total),
    }
}

#[cfg(test)]
mod tests {
    use super::{ViewerDisplayDecision, decide_viewer_display};
    use crate::presenter::{PresenterFeedback, PresenterRenderOutcome};

    #[test]
    fn display_decision_clears_when_no_image_drawn() {
        let decision = decide_viewer_display(
            PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
            },
            false,
        );
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
    fn display_decision_overlays_loading_on_pending_image() {
        let decision = decide_viewer_display(
            PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::Pending,
                used_stale_fallback: true,
            },
            true,
        );
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
        let decision = decide_viewer_display(
            PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::Failed,
                used_stale_fallback: true,
            },
            true,
        );
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
        let decision = decide_viewer_display(
            PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::Pending,
                used_stale_fallback: false,
            },
            false,
        );
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
        let decision = decide_viewer_display(
            PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::Failed,
                used_stale_fallback: false,
            },
            false,
        );
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
    fn display_decision_keeps_previous_image_when_pending_and_viewer_has_image() {
        let decision = decide_viewer_display(
            PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::Pending,
                used_stale_fallback: false,
            },
            true,
        );
        assert_eq!(
            decision,
            ViewerDisplayDecision {
                clear: false,
                show_loading: true,
                show_error: false,
            }
        );
    }
}
