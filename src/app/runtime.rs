use std::time::{Duration, Instant};

use crate::backend::{PdfBackend, RgbaFrame};
use crate::config::CacheConfig;
use crate::error::{AppError, AppResult};
use crate::perf::PerfStats;
use crate::presenter::{ImagePresenter, PanOffset, Viewport};
use crate::render::cache::{RenderedPageCache, RenderedPageKey};
use crate::render::prefetch::PrefetchClass;
use crate::render::scheduler::{
    NavIntent, PrefetchPolicy, RenderPriority, RenderScheduler, RenderTask,
    build_prefetch_plan_with_policy,
};

use super::frame_ops::{compose_spread_frame, prepare_presenter_frame};
use super::state::VisiblePageSlots;

#[derive(Debug, Default)]
pub struct RenderRuntime {
    pub l1_cache: RenderedPageCache,
    pub scheduler: RenderScheduler,
    pub perf_stats: PerfStats,
    pub prefetch_policy: PrefetchPolicy,
}

impl RenderRuntime {
    pub fn with_l1_cache_limits(l1_max_entries: usize, l1_memory_budget_bytes: usize) -> Self {
        Self {
            l1_cache: RenderedPageCache::new(l1_max_entries, l1_memory_budget_bytes),
            scheduler: RenderScheduler::default(),
            perf_stats: PerfStats::default(),
            prefetch_policy: PrefetchPolicy::default(),
        }
    }

    pub fn from_cache_config(cache: &CacheConfig) -> Self {
        Self::with_l1_cache_limits(cache.l1_max_entries, cache.l1_memory_budget_bytes())
    }

    pub fn schedule_navigation(
        &mut self,
        doc: &dyn PdfBackend,
        cursor: usize,
        nav_intent: NavIntent,
        scale: f32,
    ) {
        let canceled = self.scheduler.cancel_obsolete(nav_intent, scale);
        self.perf_stats.add_canceled_tasks(canceled);

        let tasks = build_prefetch_plan_with_policy(
            cursor,
            nav_intent,
            doc.page_count(),
            doc.doc_id(),
            scale,
            self.prefetch_policy,
        );
        self.enqueue_prefetch_tasks(tasks);
    }

    pub fn reset_prefetch(
        &mut self,
        doc: &dyn PdfBackend,
        cursor: usize,
        nav_intent: NavIntent,
        scale: f32,
    ) {
        let canceled = self.scheduler.clear();
        self.perf_stats.add_canceled_tasks(canceled);

        let tasks = build_prefetch_plan_with_policy(
            cursor,
            nav_intent,
            doc.page_count(),
            doc.doc_id(),
            scale,
            self.prefetch_policy,
        );
        self.enqueue_prefetch_tasks(tasks);
    }

    pub fn run_next_prefetch(&mut self, doc: &dyn PdfBackend) -> AppResult<Option<RenderTask>> {
        let Some(task) = self.scheduler.next_task() else {
            self.sync_queue_depth();
            return Ok(None);
        };

        let _ = self.resolve_task_frame(doc, &task)?;
        self.sync_queue_depth();
        Ok(Some(task))
    }

    pub fn pop_next_prefetch_task(&mut self) -> Option<RenderTask> {
        let task = self.scheduler.next_task();
        self.sync_queue_depth();
        task
    }

    pub fn has_prefetch_work(&self) -> bool {
        !self.scheduler.is_empty()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_current_page(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        page: usize,
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
    ) -> AppResult<()> {
        let task = RenderTask {
            doc_id: doc.doc_id(),
            page,
            scale,
            priority: RenderPriority::CriticalCurrent,
            generation: 0,
            reason: "current-page",
        };
        let frame = self.resolve_task_frame(doc, &task)?;
        let (frame, pan_for_presenter) =
            prepare_presenter_frame(&frame, viewport, pan, cell_px, enable_crop);
        presenter.prepare(
            RenderedPageKey::new(task.doc_id, task.page, task.scale),
            &frame,
            viewport,
            pan_for_presenter,
            task.generation,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prepare_current_page_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        page: usize,
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        generation: u64,
    ) -> AppResult<bool> {
        let key = RenderedPageKey::new(doc.doc_id(), page, scale);
        let prepared = if let Some(frame) = self.l1_cache.get(&key) {
            let (frame, pan_for_presenter) =
                prepare_presenter_frame(frame, viewport, pan, cell_px, enable_crop);
            presenter.prepare(key, &frame, viewport, pan_for_presenter, generation)?;
            true
        } else {
            false
        };
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(prepared)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prepare_spread_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        slots: VisiblePageSlots,
        presenter_key: RenderedPageKey,
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        generation: u64,
        gap_px: u32,
    ) -> AppResult<bool> {
        let anchor_key = RenderedPageKey::new(doc.doc_id(), slots.anchor_page, scale);
        let Some(anchor_frame) = self.l1_cache.get_cloned(&anchor_key) else {
            self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
            return Ok(false);
        };

        let trailing_frame = match slots.trailing_page {
            Some(page) => {
                let trailing_key = RenderedPageKey::new(doc.doc_id(), page, scale);
                let frame = self.l1_cache.get_cloned(&trailing_key);
                self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
                let Some(frame) = frame else {
                    return Ok(false);
                };
                Some(frame)
            }
            None => {
                self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
                None
            }
        };

        let left_frame = match slots.left_page {
            Some(page) if page == slots.anchor_page => Some(&anchor_frame),
            Some(_) => trailing_frame.as_ref(),
            None => None,
        };
        let right_frame = match slots.right_page {
            Some(page) if page == slots.anchor_page => Some(&anchor_frame),
            Some(_) => trailing_frame.as_ref(),
            None => None,
        };
        let spread_frame = compose_spread_frame(left_frame, right_frame, gap_px);
        let (frame, pan_for_presenter) =
            prepare_presenter_frame(&spread_frame, viewport, pan, cell_px, enable_crop);
        presenter.prepare(
            presenter_key,
            &frame,
            viewport,
            pan_for_presenter,
            generation,
        )?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prefetch_encode_from_cache(
        &mut self,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        key: RenderedPageKey,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        class: PrefetchClass,
        generation: u64,
    ) -> AppResult<bool> {
        let prepared = if let Some(frame) = self.l1_cache.get(&key) {
            let (frame, pan_for_presenter) =
                prepare_presenter_frame(frame, viewport, pan, cell_px, enable_crop);
            presenter.prefetch_encode(
                key,
                &frame,
                viewport,
                pan_for_presenter,
                class,
                generation,
            )?;
            true
        } else {
            false
        };
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(prepared)
    }

    pub fn has_cached_frame(&self, key: &RenderedPageKey) -> bool {
        self.l1_cache.contains(key)
    }

    pub fn ingest_rendered_frame(
        &mut self,
        key: RenderedPageKey,
        frame: RgbaFrame,
        elapsed: Duration,
        allow_single_oversize: bool,
    ) {
        self.perf_stats.record_render(elapsed);
        let _ = self.l1_cache.insert(key, frame, allow_single_oversize);
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
    }

    pub fn set_queue_depth_with_inflight(&mut self, inflight: usize) {
        self.perf_stats.set_queue_depth(self.scheduler.len());
        self.perf_stats.set_render_in_flight(inflight);
    }

    fn enqueue_prefetch_tasks(&mut self, tasks: Vec<RenderTask>) {
        for task in tasks {
            self.scheduler.enqueue(task);
        }
        self.sync_queue_depth();
    }

    fn resolve_task_frame(
        &mut self,
        doc: &dyn PdfBackend,
        task: &RenderTask,
    ) -> AppResult<RgbaFrame> {
        if task.doc_id != doc.doc_id() {
            return Err(AppError::invalid_argument(
                "render task does not match active document",
            ));
        }

        let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
        if let Some(cached) = self.l1_cache.get_cloned(&key) {
            self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
            return Ok(cached);
        }

        let render_start = Instant::now();
        let frame = doc.render_page(task.page, task.scale)?;
        self.perf_stats.record_render(render_start.elapsed());
        let allow_single_oversize = task.priority == RenderPriority::CriticalCurrent;
        let _ = self
            .l1_cache
            .insert(key, frame.clone(), allow_single_oversize);
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(frame)
    }

    fn sync_queue_depth(&mut self) {
        self.perf_stats.set_queue_depth(self.scheduler.len());
    }

    pub fn sync_presenter_metrics(&mut self, presenter: &dyn ImagePresenter) {
        if let Some(snapshot) = presenter.perf_snapshot() {
            self.perf_stats.absorb_presenter_metrics(&snapshot);
        }
    }
}
