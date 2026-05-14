use std::time::{Duration, Instant};

use crate::backend::{PdfBackend, RgbaFrame};
use crate::config::CacheConfig;
use crate::error::{AppError, AppResult};
use crate::highlight::HighlightOverlaySnapshot;
use crate::perf::PerfStats;
use crate::presenter::{ImagePresenter, PanOffset, PresenterSlot, Viewport};
use crate::render::cache::{RenderedPageCache, RenderedPageKey};
use crate::render::scheduler::{
    NavIntent, PrefetchPolicy, RenderScheduler, RenderTask, build_prefetch_plan_with_policy,
};
use crate::work::WorkClass;

use super::frame_ops::{PageRenderSpace, apply_highlight_overlay, prepare_presenter_frame};

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
        overlay: &HighlightOverlaySnapshot,
    ) -> AppResult<()> {
        let task = RenderTask {
            doc_id: doc.doc_id(),
            page,
            scale,
            class: WorkClass::CriticalCurrent,
            generation: 0,
            reason: "current-page",
        };
        let frame = self.resolve_task_frame(doc, &task)?;
        let (frame, overlay_stamp) = decorate_single_page_frame(doc, task.page, &frame, overlay);
        let (frame, pan_for_presenter) =
            prepare_presenter_frame(&frame, viewport, pan, cell_px, enable_crop);
        presenter.prepare(
            RenderedPageKey::new(task.doc_id, task.page, task.scale),
            &frame,
            viewport,
            pan_for_presenter,
            overlay_stamp,
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
        overlay: &HighlightOverlaySnapshot,
        generation: u64,
    ) -> AppResult<bool> {
        let key = RenderedPageKey::new(doc.doc_id(), page, scale);
        self.try_prepare_cached_page_from_cache(
            doc,
            presenter,
            viewport,
            key,
            pan,
            cell_px,
            enable_crop,
            overlay,
            generation,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prepare_cached_page_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        key: RenderedPageKey,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        overlay: &HighlightOverlaySnapshot,
        generation: u64,
    ) -> AppResult<bool> {
        let prepared = if let Some(frame) = self.l1_cache.get(&key) {
            let (frame, overlay_stamp) = decorate_single_page_frame(doc, key.page, frame, overlay);
            let (frame, pan_for_presenter) =
                prepare_presenter_frame(&frame, viewport, pan, cell_px, enable_crop);
            presenter.prepare(
                key,
                &frame,
                viewport,
                pan_for_presenter,
                overlay_stamp,
                generation,
            )?;
            true
        } else {
            false
        };
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(prepared)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prepare_page_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        page_slots: &[(Option<usize>, Viewport)],
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        overlay: &HighlightOverlaySnapshot,
        generation: u64,
    ) -> AppResult<bool> {
        let requested_pan = *pan;
        let Some((mut prepared, normalized_pan)) = self.build_page_slots_from_cache(
            doc,
            page_slots,
            scale,
            requested_pan,
            cell_px,
            enable_crop,
            overlay,
        )?
        else {
            self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
            return Ok(false);
        };

        if normalized_pan != requested_pan
            && let Some((rebuilt, _)) = self.build_page_slots_from_cache(
                doc,
                page_slots,
                scale,
                normalized_pan,
                cell_px,
                enable_crop,
                overlay,
            )?
        {
            prepared = rebuilt;
        }

        *pan = normalized_pan;
        let slots: Vec<_> = prepared
            .iter()
            .map(|slot| PresenterSlot {
                cache_key: slot.as_ref().map(|slot| slot.cache_key),
                frame: slot.as_ref().map(|slot| &slot.frame),
                viewport: slot.as_ref().map(|slot| slot.viewport).unwrap_or(Viewport {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                }),
                pan: slot.as_ref().map(|slot| slot.pan).unwrap_or_default(),
                overlay_stamp: slot.as_ref().map(|slot| slot.overlay_stamp).unwrap_or(0),
                generation,
            })
            .collect();
        presenter.prepare_slots(&slots)?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prefetch_encode_from_cache(
        &mut self,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        key: RenderedPageKey,
        pan: &mut PanOffset,
        overlay_stamp: u64,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        class: WorkClass,
        generation: u64,
    ) -> AppResult<bool> {
        if overlay_stamp != 0 {
            // Prefetch encoding has no overlay snapshot to apply, so skip it while highlights are
            // active instead of caching an undecorated frame under the highlighted identity.
            return Ok(false);
        }
        let prepared = if let Some(frame) = self.l1_cache.get(&key) {
            let (frame, pan_for_presenter) =
                prepare_presenter_frame(frame, viewport, pan, cell_px, enable_crop);
            presenter.prefetch_encode(
                key,
                &frame,
                viewport,
                pan_for_presenter,
                overlay_stamp,
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
        let allow_single_oversize = task.class == WorkClass::CriticalCurrent;
        let _ = self
            .l1_cache
            .insert(key, frame.clone(), allow_single_oversize);
        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(frame)
    }

    fn sync_queue_depth(&mut self) {
        self.perf_stats.set_queue_depth(self.scheduler.len());
    }

    #[allow(clippy::too_many_arguments)]
    fn build_page_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        page_slots: &[(Option<usize>, Viewport)],
        scale: f32,
        requested_pan: PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        overlay: &HighlightOverlaySnapshot,
    ) -> AppResult<Option<(Vec<Option<PreparedPresenterSlot>>, PanOffset)>> {
        let mut prepared = Vec::new();
        let mut normalized_pan = requested_pan;
        let mut saw_page = false;

        for (page, viewport) in page_slots {
            let Some(page) = *page else {
                prepared.push(None);
                continue;
            };
            saw_page = true;
            let key = RenderedPageKey::new(doc.doc_id(), page, scale);
            let Some(frame) = self.l1_cache.get(&key) else {
                return Ok(None);
            };
            let (frame, overlay_stamp) = decorate_single_page_frame(doc, page, frame, overlay);
            let mut slot_pan = requested_pan;
            let (frame, pan_for_presenter) =
                prepare_presenter_frame(&frame, *viewport, &mut slot_pan, cell_px, enable_crop);
            normalized_pan.cells_x = normalized_pan.cells_x.min(slot_pan.cells_x);
            normalized_pan.cells_y = normalized_pan.cells_y.min(slot_pan.cells_y);
            prepared.push(Some(PreparedPresenterSlot {
                cache_key: key,
                frame,
                viewport: *viewport,
                pan: pan_for_presenter,
                overlay_stamp,
            }));
        }

        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        Ok(saw_page.then_some((prepared, normalized_pan)))
    }

    pub fn sync_presenter_metrics(&mut self, presenter: &dyn ImagePresenter) {
        if let Some(snapshot) = presenter.perf_snapshot() {
            self.perf_stats.absorb_presenter_metrics(&snapshot);
        }
    }
}

struct PreparedPresenterSlot {
    cache_key: RenderedPageKey,
    frame: RgbaFrame,
    viewport: Viewport,
    pan: PanOffset,
    overlay_stamp: u64,
}

fn decorate_frame(
    frame: &RgbaFrame,
    overlay: &HighlightOverlaySnapshot,
    pages: &[PageRenderSpace],
) -> RgbaFrame {
    if overlay.is_empty() {
        frame.clone()
    } else {
        apply_highlight_overlay(frame, overlay, pages)
    }
}

fn decorate_single_page_frame(
    doc: &dyn PdfBackend,
    page: usize,
    frame: &RgbaFrame,
    overlay: &HighlightOverlaySnapshot,
) -> (RgbaFrame, u64) {
    if overlay.is_empty() {
        return (frame.clone(), 0);
    }
    match page_render_space(doc, page, frame, 0) {
        Ok(page_space) => (decorate_frame(frame, overlay, &[page_space]), overlay.stamp),
        Err(_) => (frame.clone(), 0),
    }
}

fn page_render_space(
    doc: &dyn PdfBackend,
    page: usize,
    frame: &RgbaFrame,
    origin_x_px: u32,
) -> AppResult<PageRenderSpace> {
    let (width_pt, height_pt) = doc.page_dimensions(page)?;
    Ok(PageRenderSpace {
        page,
        origin_x_px,
        origin_y_px: 0,
        width_px: frame.width,
        height_px: frame.height,
        width_pt,
        height_pt,
    })
}
