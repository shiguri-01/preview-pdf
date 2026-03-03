use ratatui::widgets::Clear;

use crate::backend::PdfBackend;
use crate::command::ActionId;
use crate::config::Config;
use crate::error::AppResult;
use crate::palette::PaletteView;
use crate::presenter::{
    PanOffset, PresenterFeedback, PresenterRenderOptions, PresenterRenderOutcome, Viewport,
};
use crate::ui;

use super::constants::DEFAULT_PAGE_SIZE_PT;
use super::core::{App, RenderSubsystem};
use super::scale::{compute_render_scale, compute_scale, quantize_scale};
use super::state::AppState;
use super::terminal_session::TerminalSurface;

pub(super) struct RenderFramePlan {
    pub(super) palette_view: Option<PaletteView>,
    pub(super) status_bar_segments: Vec<String>,
    pub(super) page_count: usize,
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

        let (page_width_pt, page_height_pt) =
            pdf.page_dimensions(page).unwrap_or(DEFAULT_PAGE_SIZE_PT);
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
        config: &Config,
        session: &mut impl TerminalSurface,
        pdf: &dyn PdfBackend,
        plan: RenderFramePlan,
    ) -> AppResult<()> {
        let RenderFramePlan {
            palette_view,
            status_bar_segments,
            page_count,
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
        let (page_width_pt, page_height_pt) = pdf
            .page_dimensions(state.current_page)
            .unwrap_or(DEFAULT_PAGE_SIZE_PT);
        let enable_crop = state.zoom > 1.0;
        let mut pan = PanOffset {
            cells_x: state.scroll_x,
            cells_y: state.scroll_y,
        };
        let mut render_error: Option<String> = None;
        let mut render_feedback = PresenterFeedback::None;
        let mut viewer_has_image = self.viewer_has_image;

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
            let max_scale = presenter_caps
                .preferred_max_render_scale
                .clamp(1.0, config.render.max_render_scale);
            let render_scale = compute_render_scale(
                viewport,
                presenter_caps.cell_px,
                page_width_pt,
                page_height_pt,
                max_scale,
            );
            let scale = compute_scale(state.zoom, render_scale);

            match self.runtime.try_prepare_current_page_from_cache(
                pdf,
                self.presenter.as_mut(),
                viewport,
                state.current_page,
                scale,
                &mut pan,
                presenter_caps.cell_px,
                enable_crop,
                generation,
            ) {
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
                            state.current_page + 1,
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
                            state.current_page + 1,
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
                        state.current_page + 1,
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
                        state.current_page + 1,
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
            state.status.message = format!(
                "render error: failed to render page {}",
                state.current_page + 1
            );
        } else if render_feedback == PresenterFeedback::Pending {
            state.status.last_action_id = Some(ActionId::RenderPending);
            state.status.message = format!("rendering page {}...", state.current_page + 1);
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
    page: usize,
    error_message: Option<&str>,
    viewer_has_image: bool,
) {
    let decision = decide_viewer_display(outcome, viewer_has_image);
    if decision.clear {
        frame.render_widget(Clear, image_area);
    }
    if decision.show_loading {
        ui::draw_loading_overlay(frame, image_area, page);
    }
    if decision.show_error {
        let message = error_message
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("Failed to render page {page}"));
        ui::draw_error_overlay(frame, image_area, &message);
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
