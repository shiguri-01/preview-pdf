use crate::backend::PdfBackend;
use crate::presenter::{PanOffset, Viewport};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::RenderTask;
use crate::render::worker::{RenderWorker, RenderWorkerResult};
use crate::work::WorkClass;

use super::actors::{RenderActor, RenderNavSyncParts};
use super::core::RenderSubsystem;
use super::frame_ops::{encode_work_class_for_completed_render, prepare_presenter_frame};
use super::scale::{scale_eq, zoom_eq};
use super::state::{AppState, VisiblePageSlots};
use super::view_ops::{InitialPreviewPlan, compute_initial_preview_plan};

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequiredRenderPages {
    pages: [usize; 2],
    keys: [RenderedPageKey; 2],
    len: usize,
}

impl RequiredRenderPages {
    pub(crate) fn new(anchor_page: usize, anchor_key: RenderedPageKey) -> Self {
        Self {
            pages: [anchor_page, 0],
            keys: [anchor_key, anchor_key],
            len: 1,
        }
    }

    pub(crate) fn push_trailing(&mut self, page: usize, key: RenderedPageKey) {
        debug_assert!(self.len < self.pages.len());
        self.pages[self.len] = page;
        self.keys[self.len] = key;
        self.len += 1;
    }

    pub(crate) fn keys(&self) -> &[RenderedPageKey] {
        &self.keys[..self.len]
    }

    fn iter(&self) -> impl Iterator<Item = (usize, usize, RenderedPageKey)> + '_ {
        (0..self.len).map(|idx| (idx, self.pages[idx], self.keys[idx]))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CurrentInterestKeys {
    keys: [RenderedPageKey; 4],
    len: usize,
}

impl CurrentInterestKeys {
    pub(crate) fn from_required(required: &RequiredRenderPages) -> Self {
        let mut keys = [required.keys[0]; 4];
        keys[..required.len].copy_from_slice(required.keys());
        Self {
            keys,
            len: required.len,
        }
    }

    pub(crate) fn extend(&mut self, keys: impl IntoIterator<Item = RenderedPageKey>) {
        for key in keys {
            debug_assert!(self.len < self.keys.len());
            self.keys[self.len] = key;
            self.len += 1;
        }
    }

    pub(crate) fn as_slice(&self) -> &[RenderedPageKey] {
        &self.keys[..self.len]
    }
}

pub(crate) struct CurrentTaskContext {
    pub(crate) current_scale: f32,
    pub(crate) required: RequiredRenderPages,
    pub(crate) current_interest_keys: CurrentInterestKeys,
    pub(crate) current_cached: bool,
    pub(crate) preview_tasks: Vec<RenderTask>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrefetchDispatchContext {
    pub(crate) required: RequiredRenderPages,
    pub(crate) current_cached: bool,
    pub(crate) overlay_stamp: u64,
    pub(crate) prefetch_viewport: Option<Viewport>,
    pub(crate) base_pan: PanOffset,
    pub(crate) enable_crop: bool,
    pub(crate) interactive: bool,
    pub(crate) dispatch_budget: usize,
}

pub(crate) struct PrefetchDispatchPlan {
    pub(crate) overlay_stamp: u64,
    pub(crate) prefetch_viewport: Option<Viewport>,
    pub(crate) base_pan: PanOffset,
    pub(crate) interactive: bool,
    pub(crate) dispatch_budget: usize,
}

pub(crate) struct CurrentRenderView {
    pub(crate) visible_pages: VisiblePageSlots,
    pub(crate) current_scale: f32,
    pub(crate) required: RequiredRenderPages,
    pub(crate) current_interest_keys: CurrentInterestKeys,
    pub(crate) initial_preview: Option<InitialPreviewPlan>,
    pub(crate) presenter_key: RenderedPageKey,
    pub(crate) current_cached: bool,
}

impl CurrentRenderView {
    pub(crate) fn preview_tasks(&self, generation: u64) -> Vec<RenderTask> {
        self.initial_preview
            .as_ref()
            .map(|plan| {
                plan.page_keys
                    .iter()
                    .enumerate()
                    .map(|(idx, key)| RenderTask {
                        doc_id: key.doc_id,
                        page: key.page,
                        scale: key.scale_milli as f32 / 1000.0,
                        class: WorkClass::CriticalCurrent,
                        generation,
                        reason: if idx == 0 {
                            "initial-preview"
                        } else {
                            "initial-preview-spread"
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn prefetch_dispatch_context(
        &self,
        state: &AppState,
        plan: PrefetchDispatchPlan,
    ) -> PrefetchDispatchContext {
        PrefetchDispatchContext {
            required: self.required,
            current_cached: self.current_cached,
            overlay_stamp: plan.overlay_stamp,
            prefetch_viewport: plan.prefetch_viewport,
            base_pan: plan.base_pan,
            enable_crop: state.zoom > 1.0,
            interactive: plan.interactive,
            dispatch_budget: plan.dispatch_budget,
        }
    }
}

pub(crate) fn cold_start_initial_preview_plan(
    is_cold_start: bool,
    current_cached: bool,
    doc_id: u64,
    visible_pages: VisiblePageSlots,
    page_layout_mode: super::state::PageLayoutMode,
    current_scale: f32,
) -> Option<InitialPreviewPlan> {
    if !is_cold_start || current_cached {
        return None;
    }

    compute_initial_preview_plan(doc_id, visible_pages, page_layout_mode, current_scale)
}

impl RenderSubsystem {
    pub(crate) fn build_current_render_view(
        &self,
        state: &AppState,
        pdf: &dyn PdfBackend,
        visible_pages: VisiblePageSlots,
        current_scale: f32,
        is_cold_start: bool,
    ) -> CurrentRenderView {
        let mut required = RequiredRenderPages::new(
            visible_pages.anchor_page,
            RenderedPageKey::new(pdf.doc_id(), visible_pages.anchor_page, current_scale),
        );
        if let Some(trailing_page) = visible_pages.trailing_page {
            required.push_trailing(
                trailing_page,
                RenderedPageKey::new(pdf.doc_id(), trailing_page, current_scale),
            );
        }
        let current_cached = required
            .keys()
            .iter()
            .all(|key| self.runtime.has_cached_frame(key));
        let presenter_layout_tag =
            state.presenter_layout_tag(visible_pages.trailing_page.is_some());
        let initial_preview = cold_start_initial_preview_plan(
            is_cold_start,
            current_cached,
            pdf.doc_id(),
            visible_pages,
            state.page_layout_mode,
            current_scale,
        );
        let mut current_interest_keys = CurrentInterestKeys::from_required(&required);
        if let Some(preview_plan) = initial_preview.as_ref() {
            current_interest_keys.extend(preview_plan.page_keys.iter().copied());
        }
        let presenter_key = RenderedPageKey::with_layout(
            pdf.doc_id(),
            visible_pages.anchor_page,
            current_scale,
            presenter_layout_tag,
        );

        CurrentRenderView {
            visible_pages,
            current_scale,
            required,
            current_interest_keys,
            initial_preview,
            presenter_key,
            current_cached,
        }
    }

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
                        0,
                        encode_work_class_for_completed_render(completed.class),
                        completed.generation,
                    ) {
                        let _ = err;
                    }
                }
                self.runtime.ingest_rendered_frame(
                    completed.key,
                    frame,
                    completed.elapsed,
                    completed.class == WorkClass::CriticalCurrent,
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
                    let _ = err;
                    state.set_error_notice("Could not render the current page.");
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
        let canceled = render_worker.cancel_stale_prefetch_except(
            render_actor.generation(),
            ctx.current_interest_keys.as_slice(),
        );
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
                    ctx.current_interest_keys.as_slice(),
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

        for (idx, page, key) in ctx.required.iter() {
            if self.runtime.has_cached_frame(&key) || render_worker.has_in_flight(&key) {
                continue;
            }

            let (enqueued, preempted) = render_worker.enqueue_current_with_preemption(
                RenderTask {
                    doc_id: pdf.doc_id(),
                    page,
                    scale: ctx.current_scale,
                    class: WorkClass::CriticalCurrent,
                    generation: render_actor.generation(),
                    reason: if idx == 0 {
                        "current-page"
                    } else {
                        "current-page-spread"
                    },
                },
                render_actor.generation(),
                ctx.current_interest_keys.as_slice(),
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
                if ctx.required.keys().contains(&key) {
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
                        ctx.overlay_stamp,
                        presenter_caps.cell_px,
                        ctx.enable_crop,
                        encode_work_class_for_completed_render(task.class),
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

    use super::*;
    use crate::app::state::{PageLayoutMode, VisiblePageSlots};
    use crate::app::{NoticeLevel, RenderRuntime};
    use crate::backend::RgbaFrame;
    use crate::error::{AppError, AppResult};
    use crate::perf::PerfStats;
    use crate::presenter::{
        ImagePresenter, PanOffset, PresenterCaps, PresenterFeedback, PresenterRenderOutcome,
        PresenterRenderSlot, PresenterSlot,
    };

    #[derive(Default)]
    struct StubPresenter;

    impl ImagePresenter for StubPresenter {
        fn prepare_slots(&mut self, _slots: &[PresenterSlot<'_>]) -> AppResult<()> {
            Ok(())
        }

        fn render_slots(
            &mut self,
            _frame: &mut ratatui::Frame<'_>,
            _slots: &[PresenterRenderSlot],
        ) -> AppResult<PresenterRenderOutcome> {
            Ok(PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
                slots: Vec::new(),
            })
        }

        fn prefetch_encode(
            &mut self,
            _cache_key: RenderedPageKey,
            _frame: &RgbaFrame,
            _viewport: Viewport,
            _pan: PanOffset,
            _overlay_stamp: u64,
            _class: crate::work::WorkClass,
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
        RenderSubsystem::new(Box::<StubPresenter>::default(), RenderRuntime::default())
    }

    fn failed_result(key: RenderedPageKey, class: WorkClass) -> RenderWorkerResult {
        RenderWorkerResult {
            key,
            class,
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
            failed_result(key, WorkClass::CriticalCurrent),
            &[key],
            None,
            PanOffset::default(),
            false,
            false,
        );

        assert!(redraw);
        let notice = state.notice.expect("visible failure should set notice");
        assert_eq!(notice.level, NoticeLevel::Error);
        assert_eq!(notice.message, "Could not render the current page.");
    }

    #[test]
    fn process_render_result_ignores_prefetch_render_failure_notice() {
        let current_key = RenderedPageKey::new(7, 0, 1.0);
        let prefetch_key = RenderedPageKey::new(7, 3, 1.0);
        let mut render = subsystem();
        let mut state = AppState::default();

        let redraw = render.process_render_result(
            &mut state,
            failed_result(prefetch_key, WorkClass::Background),
            &[current_key],
            None,
            PanOffset::default(),
            false,
            false,
        );

        assert!(!redraw);
        assert!(state.notice.is_none());
    }

    #[test]
    fn cold_start_initial_preview_plan_is_disabled_after_first_image() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(true, true, 7, slots, PageLayoutMode::Single, 1.0);

        assert_eq!(preview, None);
    }

    #[test]
    fn cold_start_initial_preview_plan_is_available_before_first_image() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(true, false, 7, slots, PageLayoutMode::Single, 1.0);

        assert!(preview.is_some());
    }

    #[test]
    fn cold_start_initial_preview_plan_stays_available_until_current_frame_is_cached() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: Some(1),
            left_page: Some(0),
            right_page: Some(1),
        };

        let preview =
            cold_start_initial_preview_plan(true, false, 7, slots, PageLayoutMode::Spread, 1.0)
                .expect("spread cold start should include preview pages");

        assert_eq!(preview.page_keys.len(), 2);
        assert_eq!(preview.page_keys[0], RenderedPageKey::new(7, 0, 0.25));
        assert_eq!(preview.page_keys[1], RenderedPageKey::new(7, 1, 0.25));
    }

    #[test]
    fn cold_start_initial_preview_plan_is_disabled_after_navigation_begins() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(false, false, 7, slots, PageLayoutMode::Single, 1.0);

        assert_eq!(preview, None);
    }
}
