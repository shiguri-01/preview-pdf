use crate::backend::PdfBackend;
use crate::presenter::{PanOffset, Viewport};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::{RenderPriority, RenderTask};
use crate::render::worker::{RenderWorker, RenderWorkerResult};

use super::actors::{RenderActor, RenderNavSyncParts};
use super::core::RenderSubsystem;
use super::frame_ops::{prefetch_class_for_completed_task, prepare_presenter_frame};
use super::scale::{scale_eq, zoom_eq};
use super::state::AppState;

pub(crate) struct CurrentTaskContext {
    pub(crate) current_scale: f32,
    pub(crate) required_pages: Vec<usize>,
    pub(crate) required_keys: Vec<RenderedPageKey>,
    pub(crate) current_interest_keys: Vec<RenderedPageKey>,
    pub(crate) current_cached: bool,
    pub(crate) preview_tasks: Vec<RenderTask>,
}

pub(crate) struct PrefetchDispatchContext {
    pub(crate) required_keys: Vec<RenderedPageKey>,
    pub(crate) current_cached: bool,
    pub(crate) prefetch_viewport: Option<Viewport>,
    pub(crate) base_pan: PanOffset,
    pub(crate) enable_crop: bool,
    pub(crate) interactive: bool,
    pub(crate) dispatch_budget: usize,
}

impl RenderSubsystem {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process_render_result(
        &mut self,
        state: &mut AppState,
        completed: RenderWorkerResult,
        current_keys: &[RenderedPageKey],
        prefetch_viewport: Option<Viewport>,
        base_pan: PanOffset,
        enable_crop: bool,
        interactive: bool,
    ) -> bool {
        let presenter_caps = self.presenter.capabilities();
        match completed.result {
            Ok(frame) => {
                self.runtime
                    .perf_stats
                    .record_render_queue_wait(completed.queue_wait);
                if !interactive
                    && !current_keys.contains(&completed.key)
                    && let Some(viewport) = prefetch_viewport
                {
                    let mut prefetch_pan = base_pan;
                    let (frame, pan_for_presenter) = prepare_presenter_frame(
                        &frame,
                        viewport,
                        &mut prefetch_pan,
                        presenter_caps.cell_px,
                        enable_crop,
                    );
                    if let Err(err) = self.presenter.prefetch_encode(
                        completed.key,
                        &frame,
                        viewport,
                        pan_for_presenter,
                        prefetch_class_for_completed_task(completed.priority),
                        completed.generation,
                    ) {
                        let _ = err;
                    }
                }
                self.runtime.ingest_rendered_frame(
                    completed.key,
                    frame,
                    completed.elapsed,
                    completed.priority == RenderPriority::CriticalCurrent,
                );
                current_keys.contains(&completed.key)
            }
            Err(err) => {
                self.runtime
                    .perf_stats
                    .record_render_queue_wait(completed.queue_wait);
                let is_current = current_keys.contains(&completed.key);
                if is_current {
                    // Only surface failures for pages the user is actively waiting on.
                    // Prefetch can fail for off-screen pages, but notifying that background
                    // work would surface a problem the user cannot act on yet.
                    state.set_error_notice(format!("render error: {err}"));
                }
                is_current
            }
        }
    }

    pub(crate) fn sync_navigation_state(
        &mut self,
        state: &AppState,
        pdf: &dyn PdfBackend,
        parts: &mut RenderNavSyncParts<'_>,
        current_scale: f32,
    ) -> bool {
        if !zoom_eq(state.zoom, *parts.tracked_zoom) {
            parts.nav.on_zoom_change();
            self.runtime
                .reset_prefetch(pdf, state.current_page, parts.nav.intent(), current_scale);
            *parts.tracked_zoom = state.zoom;
            *parts.tracked_scale = current_scale;
            *parts.tracked_page = state.current_page;
            return true;
        }

        if state.current_page != *parts.tracked_page {
            parts
                .nav
                .on_page_change(*parts.tracked_page, state.current_page, state.page_step());
            self.runtime.schedule_navigation(
                pdf,
                state.current_page,
                parts.nav.intent(),
                current_scale,
            );
            *parts.tracked_page = state.current_page;
            *parts.tracked_scale = current_scale;
            return true;
        }

        if !scale_eq(current_scale, *parts.tracked_scale) {
            parts.nav.on_scale_change();
            self.runtime
                .reset_prefetch(pdf, state.current_page, parts.nav.intent(), current_scale);
            *parts.tracked_scale = current_scale;
            return true;
        }

        false
    }

    pub(crate) fn ensure_current_task_enqueued(
        &mut self,
        _state: &mut AppState,
        pdf: &dyn PdfBackend,
        render_actor: &RenderActor,
        render_worker: &mut RenderWorker,
        ctx: CurrentTaskContext,
    ) {
        let canceled = render_worker
            .cancel_stale_prefetch_except(render_actor.generation(), &ctx.current_interest_keys);
        if canceled > 0 {
            self.runtime.perf_stats.add_canceled_tasks(canceled);
        }
        self.runtime
            .set_queue_depth_with_inflight(render_worker.in_flight_len());

        if ctx.current_cached {
            return;
        }

        for preview_task in ctx.preview_tasks {
            let preview_key =
                RenderedPageKey::new(preview_task.doc_id, preview_task.page, preview_task.scale);
            if !self.runtime.has_cached_frame(&preview_key)
                && !render_worker.has_in_flight(&preview_key)
            {
                let (enqueued, preempted) = render_worker.enqueue_current_with_preemption(
                    preview_task,
                    render_actor.generation(),
                    &ctx.current_interest_keys,
                );
                if preempted > 0 {
                    self.runtime.perf_stats.add_canceled_tasks(preempted);
                }
                if !enqueued {
                    break;
                }
                self.runtime
                    .set_queue_depth_with_inflight(render_worker.in_flight_len());
            }
        }

        debug_assert_eq!(
            ctx.required_pages.len(),
            ctx.required_keys.len(),
            "required_pages and required_keys must have equal lengths"
        );
        for (idx, page) in ctx.required_pages.into_iter().enumerate() {
            let key = ctx.required_keys[idx];
            if self.runtime.has_cached_frame(&key) || render_worker.has_in_flight(&key) {
                continue;
            }

            let (enqueued, preempted) = render_worker.enqueue_current_with_preemption(
                RenderTask {
                    doc_id: pdf.doc_id(),
                    page,
                    scale: ctx.current_scale,
                    priority: RenderPriority::CriticalCurrent,
                    generation: render_actor.generation(),
                    reason: if idx == 0 {
                        "current-page"
                    } else {
                        "current-page-spread"
                    },
                },
                render_actor.generation(),
                &ctx.current_interest_keys,
            );
            if preempted > 0 {
                self.runtime.perf_stats.add_canceled_tasks(preempted);
            }
            if !enqueued {
                break;
            }
            self.runtime
                .set_queue_depth_with_inflight(render_worker.in_flight_len());
        }
    }

    pub(crate) fn dispatch_prefetch_if_due(
        &mut self,
        _state: &mut AppState,
        render_actor: &mut RenderActor,
        render_worker: &mut RenderWorker,
        mut ctx: PrefetchDispatchContext,
    ) {
        if render_actor.take_prefetch_due() && !ctx.interactive && ctx.current_cached {
            while render_worker.available_slots() > 0 && ctx.dispatch_budget > 0 {
                let Some(task) = self.runtime.pop_next_prefetch_task() else {
                    break;
                };
                ctx.dispatch_budget -= 1;
                let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
                if ctx.required_keys.contains(&key) {
                    continue;
                }
                if !self.runtime.has_cached_frame(&key) {
                    let _ = render_worker.enqueue(task);
                    self.runtime
                        .set_queue_depth_with_inflight(render_worker.in_flight_len());
                    continue;
                }
                if let Some(viewport) = ctx.prefetch_viewport {
                    let mut prefetch_pan = ctx.base_pan;
                    let presenter_caps = self.presenter.capabilities();
                    if let Err(err) = self.runtime.try_prefetch_encode_from_cache(
                        self.presenter.as_mut(),
                        viewport,
                        key,
                        &mut prefetch_pan,
                        presenter_caps.cell_px,
                        ctx.enable_crop,
                        prefetch_class_for_completed_task(task.priority),
                        task.generation,
                    ) {
                        let _ = err;
                    }
                }
                self.runtime
                    .set_queue_depth_with_inflight(render_worker.in_flight_len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::layout::Rect;

    use super::*;
    use crate::app::{NoticeLevel, RenderRuntime};
    use crate::backend::RgbaFrame;
    use crate::error::{AppError, AppResult};
    use crate::perf::PerfStats;
    use crate::presenter::{
        ImagePresenter, PanOffset, PresenterCaps, PresenterFeedback, PresenterRenderOptions,
        PresenterRenderOutcome,
    };

    #[derive(Default)]
    struct StubPresenter;

    impl ImagePresenter for StubPresenter {
        fn prepare(
            &mut self,
            _cache_key: RenderedPageKey,
            _frame: &RgbaFrame,
            _viewport: Viewport,
            _pan: PanOffset,
            _generation: u64,
        ) -> AppResult<()> {
            Ok(())
        }

        fn render(
            &mut self,
            _frame: &mut ratatui::Frame<'_>,
            _area: Rect,
            _options: PresenterRenderOptions,
        ) -> AppResult<PresenterRenderOutcome> {
            Ok(PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
            })
        }

        fn prefetch_encode(
            &mut self,
            _cache_key: RenderedPageKey,
            _frame: &RgbaFrame,
            _viewport: Viewport,
            _pan: PanOffset,
            _class: crate::render::prefetch::PrefetchClass,
            _generation: u64,
        ) -> AppResult<()> {
            Ok(())
        }

        fn capabilities(&self) -> PresenterCaps {
            PresenterCaps {
                backend_name: "stub",
                supports_l2_cache: false,
                cell_px: None,
                preferred_max_render_scale: 2.0,
            }
        }

        fn perf_snapshot(&self) -> Option<PerfStats> {
            None
        }
    }

    fn subsystem() -> RenderSubsystem {
        RenderSubsystem {
            presenter: Box::<StubPresenter>::default(),
            runtime: RenderRuntime::default(),
            viewer_has_image: false,
        }
    }

    fn failed_result(key: RenderedPageKey, priority: RenderPriority) -> RenderWorkerResult {
        RenderWorkerResult {
            key,
            priority,
            generation: 3,
            result: Err(AppError::pdf_render(
                2,
                AppError::invalid_argument("decode failed"),
            )),
            queue_wait: Duration::from_millis(1),
            elapsed: Duration::from_millis(2),
        }
    }

    #[test]
    fn process_render_result_surfaces_visible_render_failure() {
        let key = RenderedPageKey::new(7, 2, 1.0);
        let mut render = subsystem();
        let mut state = AppState::default();

        let redraw = render.process_render_result(
            &mut state,
            failed_result(key, RenderPriority::CriticalCurrent),
            &[key],
            None,
            PanOffset::default(),
            false,
            false,
        );

        assert!(redraw);
        let notice = state.notice.expect("visible failure should set notice");
        assert_eq!(notice.level, NoticeLevel::Error);
        assert_eq!(notice.message, "render error: PDF render failed for page 2");
    }

    #[test]
    fn process_render_result_ignores_prefetch_render_failure_notice() {
        let current_key = RenderedPageKey::new(7, 0, 1.0);
        let prefetch_key = RenderedPageKey::new(7, 3, 1.0);
        let mut render = subsystem();
        let mut state = AppState::default();

        let redraw = render.process_render_result(
            &mut state,
            failed_result(prefetch_key, RenderPriority::Background),
            &[current_key],
            None,
            PanOffset::default(),
            false,
            false,
        );

        assert!(!redraw);
        assert!(state.notice.is_none());
    }
}
