use crate::backend::PdfBackend;
use crate::command::ActionId;
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
    pub(crate) current_cached: bool,
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
                        state.status.last_action_id = Some(ActionId::PrefetchEncode);
                        state.status.message = format!("encode prefetch error: {err}");
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
                state.status.last_action_id = Some(ActionId::RenderWorker);
                state.status.message = format!("render error: {err}");
                current_keys.contains(&completed.key)
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
        state: &mut AppState,
        pdf: &dyn PdfBackend,
        render_actor: &RenderActor,
        render_worker: &mut RenderWorker,
        ctx: CurrentTaskContext,
    ) {
        let canceled = render_worker
            .cancel_stale_prefetch_except(render_actor.generation(), &ctx.required_keys);
        if canceled > 0 {
            self.runtime.perf_stats.add_canceled_tasks(canceled);
        }
        self.runtime
            .set_queue_depth_with_inflight(render_worker.in_flight_len());

        if ctx.current_cached {
            return;
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
                &ctx.required_keys,
            );
            if preempted > 0 {
                self.runtime.perf_stats.add_canceled_tasks(preempted);
            }
            if !enqueued {
                state.status.last_action_id = Some(ActionId::RenderQueue);
                state.status.message = format!("render queue busy; retrying page {}", page + 1);
                break;
            }
            self.runtime
                .set_queue_depth_with_inflight(render_worker.in_flight_len());
        }
    }

    pub(crate) fn dispatch_prefetch_if_due(
        &mut self,
        state: &mut AppState,
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
                        state.status.last_action_id = Some(ActionId::PrefetchEncode);
                        state.status.message = format!("encode prefetch error: {err}");
                    }
                }
            }
        }
        self.runtime
            .set_queue_depth_with_inflight(render_worker.in_flight_len());
    }
}
