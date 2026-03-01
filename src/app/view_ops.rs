use ratatui::widgets::Clear;

use crate::backend::PdfBackend;
use crate::command::ActionId;
use crate::config::Config;
use crate::error::AppResult;
use crate::palette::PaletteView;
use crate::presenter::{PanOffset, Viewport};
use crate::ui;

use super::constants::DEFAULT_PAGE_SIZE_PT;
use super::core::{App, RenderSubsystem};
use super::scale::{compute_render_scale, compute_scale, quantize_scale};
use super::state::AppState;
use super::terminal_session::TerminalSurface;

pub(super) struct RenderFramePlan {
    pub(super) palette_view: Option<PaletteView>,
    pub(super) page_count: usize,
    pub(super) generation: u64,
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
            page_count,
            generation,
        } = plan;
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
        let mut render_pending = false;

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
            );

            let viewport = Viewport {
                x: layout.viewer_inner.x,
                y: layout.viewer_inner.y,
                width: layout.viewer_inner.width.max(1),
                height: layout.viewer_inner.height.max(1),
            };
            let image_area = layout.viewer_inner;
            // Clear the full viewer area first so transient overlays (palette popup, loading box)
            // never leave stale cells behind after mode transitions.
            frame.render_widget(Clear, image_area);
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
                Ok(true) => match self.presenter.render(frame, image_area) {
                    Ok(true) => {}
                    Ok(false) => {
                        render_pending = true;
                        ui::draw_loading_overlay(frame, image_area, state.current_page + 1);
                    }
                    Err(err) => {
                        render_error = Some(err.to_string());
                    }
                },
                Ok(false) => {
                    render_pending = true;
                    ui::draw_loading_overlay(frame, image_area, state.current_page + 1);
                }
                Err(err) => {
                    render_error = Some(err.to_string());
                }
            }

            if let Some(view) = palette_view.as_ref() {
                ui::draw_palette_overlay(frame, image_area, view);
            }
        })?;
        state.scroll_x = pan.cells_x;
        state.scroll_y = pan.cells_y;
        self.runtime.sync_presenter_metrics(self.presenter.as_ref());

        if let Some(err) = render_error {
            state.status.last_action_id = Some(ActionId::RenderPage);
            state.status.message = format!("render error: {err}");
        } else if render_pending {
            state.status.last_action_id = Some(ActionId::RenderPending);
            state.status.message = format!("rendering page {}...", state.current_page + 1);
        }

        Ok(())
    }
}
