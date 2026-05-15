use std::time::{Duration, Instant};

use ratatui::layout::Rect;

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

use super::frame_ops::{
    PageRenderSpace, apply_highlight_overlay, crop_frame_region, prepare_presenter_frame,
};
use super::scale::resolved_cell_size_px;
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
        prepare_presenter_slots(
            presenter,
            &[Some(PreparedPresenterSlot {
                cache_key: RenderedPageKey::new(task.doc_id, task.page, task.scale),
                frame,
                viewport,
                pan: pan_for_presenter,
                overlay_stamp,
            })],
            task.generation,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prepare_page_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        page_slots: &[(Option<RenderedPageKey>, Viewport)],
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
                normalized_pan,
                cell_px,
                enable_crop,
                overlay,
            )?
        {
            prepared = rebuilt;
        }

        *pan = normalized_pan;
        prepare_presenter_slots(presenter, &prepared, generation)?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_prepare_spread_canvas_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        presenter: &mut dyn ImagePresenter,
        viewport: Viewport,
        slots: VisiblePageSlots,
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        overlay: &HighlightOverlaySnapshot,
        generation: u64,
        gap_px: u32,
    ) -> AppResult<Option<[Option<Rect>; 2]>> {
        let Some(prepared) = self.build_spread_canvas_slots_from_cache(
            doc, viewport, slots, scale, pan, cell_px, overlay, gap_px,
        )?
        else {
            self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
            return Ok(None);
        };

        prepare_presenter_slots(presenter, &prepared.presenter_slots, generation)?;
        Ok(Some(prepared.render_areas))
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
        page_slots: &[(Option<RenderedPageKey>, Viewport)],
        requested_pan: PanOffset,
        cell_px: Option<(u16, u16)>,
        enable_crop: bool,
        overlay: &HighlightOverlaySnapshot,
    ) -> AppResult<Option<(Vec<Option<PreparedPresenterSlot>>, PanOffset)>> {
        let mut prepared = Vec::new();
        let mut normalized_pan = PanOffset {
            cells_x: requested_pan.cells_x.max(0),
            cells_y: requested_pan.cells_y.max(0),
        };
        let mut saw_cached_page = false;

        for (key, viewport) in page_slots {
            let Some(key) = *key else {
                prepared.push(None);
                continue;
            };
            let Some(frame) = self.l1_cache.get(&key) else {
                prepared.push(None);
                continue;
            };
            saw_cached_page = true;
            let (frame, overlay_stamp) = decorate_single_page_frame(doc, key.page, frame, overlay);
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
        Ok(saw_cached_page.then_some((prepared, normalized_pan)))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_spread_canvas_slots_from_cache(
        &mut self,
        doc: &dyn PdfBackend,
        viewport: Viewport,
        slots: VisiblePageSlots,
        scale: f32,
        pan: &mut PanOffset,
        cell_px: Option<(u16, u16)>,
        overlay: &HighlightOverlaySnapshot,
        gap_px: u32,
    ) -> AppResult<Option<PreparedSpreadCanvasSlots>> {
        let left = self.cached_decorated_page(doc, slots.left_page, scale, overlay)?;
        let right = self.cached_decorated_page(doc, slots.right_page, scale, overlay)?;

        let left_width = left
            .as_ref()
            .map(|page| page.frame.width)
            .or_else(|| right.as_ref().map(|page| page.frame.width))
            .unwrap_or(1);
        let right_width = right
            .as_ref()
            .map(|page| page.frame.width)
            .or_else(|| left.as_ref().map(|page| page.frame.width))
            .unwrap_or(1);
        let left_height = left
            .as_ref()
            .map(|page| page.frame.height)
            .or_else(|| right.as_ref().map(|page| page.frame.height))
            .unwrap_or(1);
        let right_height = right
            .as_ref()
            .map(|page| page.frame.height)
            .or_else(|| left.as_ref().map(|page| page.frame.height))
            .unwrap_or(1);
        let canvas_width = left_width
            .saturating_add(gap_px)
            .saturating_add(right_width);
        let canvas_height = left_height.max(right_height);
        let (cell_width_px, cell_height_px) = resolved_cell_size_px(cell_px);
        let viewport_width_px =
            u32::from(viewport.width.max(1)).saturating_mul(u32::from(cell_width_px));
        let viewport_height_px =
            u32::from(viewport.height.max(1)).saturating_mul(u32::from(cell_height_px));
        let max_x = canvas_width.saturating_sub(viewport_width_px);
        let max_y = canvas_height.saturating_sub(viewport_height_px);
        let max_cells_x = (max_x / u32::from(cell_width_px)) as i32;
        let max_cells_y = (max_y / u32::from(cell_height_px)) as i32;
        pan.cells_x = pan.cells_x.clamp(0, max_cells_x);
        pan.cells_y = pan.cells_y.clamp(0, max_cells_y);
        let view_x = pan.cells_x.saturating_mul(i32::from(cell_width_px)).max(0) as u32;
        let view_y = pan.cells_y.saturating_mul(i32::from(cell_height_px)).max(0) as u32;

        let left_slot = self.canvas_slot(
            left,
            view_x,
            view_y,
            viewport,
            cell_width_px,
            cell_height_px,
            0,
            *pan,
        );
        let right_slot = self.canvas_slot(
            right,
            view_x,
            view_y,
            viewport,
            cell_width_px,
            cell_height_px,
            left_width.saturating_add(gap_px),
            *pan,
        );

        self.perf_stats.set_l1_hit_rate(self.l1_cache.hit_rate());
        if left_slot.render_area.is_none() && right_slot.render_area.is_none() {
            return Ok(None);
        }
        Ok(Some(PreparedSpreadCanvasSlots {
            presenter_slots: [left_slot.presenter_slot, right_slot.presenter_slot],
            render_areas: [left_slot.render_area, right_slot.render_area],
        }))
    }

    fn cached_decorated_page(
        &mut self,
        doc: &dyn PdfBackend,
        page: Option<usize>,
        scale: f32,
        overlay: &HighlightOverlaySnapshot,
    ) -> AppResult<Option<CachedDecoratedPage>> {
        let Some(page) = page else {
            return Ok(None);
        };
        let key = RenderedPageKey::new(doc.doc_id(), page, scale);
        let Some(frame) = self.l1_cache.get(&key) else {
            return Ok(None);
        };
        let (frame, overlay_stamp) = decorate_single_page_frame(doc, page, frame, overlay);
        Ok(Some(CachedDecoratedPage {
            key,
            frame,
            overlay_stamp,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn canvas_slot(
        &self,
        page: Option<CachedDecoratedPage>,
        view_x: u32,
        view_y: u32,
        viewport: Viewport,
        cell_width_px: u16,
        cell_height_px: u16,
        page_origin_x: u32,
        pan: PanOffset,
    ) -> PreparedSpreadCanvasSlot {
        let Some(page) = page else {
            return PreparedSpreadCanvasSlot::inactive();
        };
        let viewport_width_px =
            u32::from(viewport.width.max(1)).saturating_mul(u32::from(cell_width_px));
        let viewport_height_px =
            u32::from(viewport.height.max(1)).saturating_mul(u32::from(cell_height_px));
        let page_x0 = page_origin_x;
        let page_x1 = page_origin_x.saturating_add(page.frame.width);
        let view_x1 = view_x.saturating_add(viewport_width_px);
        let view_y1 = view_y.saturating_add(viewport_height_px);
        let x0 = page_x0.max(view_x);
        let y0 = view_y;
        let x1 = page_x1.min(view_x1);
        let y1 = page.frame.height.min(view_y1);
        if x1 <= x0 || y1 <= y0 {
            return PreparedSpreadCanvasSlot::inactive();
        }

        let crop_x = x0.saturating_sub(page_origin_x);
        let crop_y = y0;
        let crop_width = x1.saturating_sub(x0);
        let crop_height = y1.saturating_sub(y0);
        let frame = crop_frame_region(&page.frame, crop_x, crop_y, crop_width, crop_height);
        let area = Rect::new(
            viewport
                .x
                .saturating_add(px_to_cells_floor(x0.saturating_sub(view_x), cell_width_px)),
            viewport
                .y
                .saturating_add(px_to_cells_floor(y0.saturating_sub(view_y), cell_height_px)),
            px_to_cells_ceil(crop_width, cell_width_px).min(viewport.width),
            px_to_cells_ceil(crop_height, cell_height_px).min(viewport.height),
        );
        if area.width == 0 || area.height == 0 {
            return PreparedSpreadCanvasSlot::inactive();
        }
        PreparedSpreadCanvasSlot {
            presenter_slot: Some(PreparedPresenterSlot {
                cache_key: page.key,
                frame,
                viewport: Viewport::from(area),
                pan,
                overlay_stamp: page.overlay_stamp,
            }),
            render_area: Some(area),
        }
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

struct PreparedSpreadCanvasSlots {
    presenter_slots: [Option<PreparedPresenterSlot>; 2],
    render_areas: [Option<Rect>; 2],
}

struct PreparedSpreadCanvasSlot {
    presenter_slot: Option<PreparedPresenterSlot>,
    render_area: Option<Rect>,
}

impl PreparedSpreadCanvasSlot {
    const fn inactive() -> Self {
        Self {
            presenter_slot: None,
            render_area: None,
        }
    }
}

struct CachedDecoratedPage {
    key: RenderedPageKey,
    frame: RgbaFrame,
    overlay_stamp: u64,
}

fn prepare_presenter_slots(
    presenter: &mut dyn ImagePresenter,
    prepared: &[Option<PreparedPresenterSlot>],
    generation: u64,
) -> AppResult<()> {
    let slots: Vec<_> = prepared
        .iter()
        .map(|slot| presenter_slot_from_prepared(slot.as_ref(), generation))
        .collect();
    presenter.prepare_slots(&slots)
}

fn presenter_slot_from_prepared(
    slot: Option<&PreparedPresenterSlot>,
    generation: u64,
) -> PresenterSlot<'_> {
    PresenterSlot {
        cache_key: slot.map(|slot| slot.cache_key),
        frame: slot.map(|slot| &slot.frame),
        viewport: slot.map(|slot| slot.viewport).unwrap_or(Viewport {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }),
        pan: slot.map(|slot| slot.pan).unwrap_or_default(),
        overlay_stamp: slot.map(|slot| slot.overlay_stamp).unwrap_or(0),
        generation,
    }
}

fn px_to_cells_floor(px: u32, cell_px: u16) -> u16 {
    (px / u32::from(cell_px.max(1))).min(u32::from(u16::MAX)) as u16
}

fn px_to_cells_ceil(px: u32, cell_px: u16) -> u16 {
    let cell_px = u32::from(cell_px.max(1));
    px.saturating_add(cell_px.saturating_sub(1))
        .saturating_div(cell_px)
        .min(u32::from(u16::MAX)) as u16
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

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;
    use crate::backend::OutlineNode;
    use crate::error::AppResult;
    use crate::presenter::{
        PresenterCaps, PresenterFeedback, PresenterRenderOutcome, PresenterRenderSlot,
    };

    #[derive(Default)]
    struct TestPresenter {
        prepared_viewports: Vec<Viewport>,
        prepared_frame_sizes: Vec<(u32, u32)>,
        prepared_slot_pages: Vec<Option<usize>>,
    }

    impl ImagePresenter for TestPresenter {
        fn prepare_slots(&mut self, slots: &[PresenterSlot<'_>]) -> AppResult<()> {
            self.prepared_slot_pages = slots
                .iter()
                .map(|slot| slot.cache_key.map(|key| key.page))
                .collect();
            for slot in slots {
                let Some(frame) = slot.frame else {
                    continue;
                };
                self.prepared_viewports.push(slot.viewport);
                self.prepared_frame_sizes.push((frame.width, frame.height));
            }
            Ok(())
        }

        fn render_slots(
            &mut self,
            _frame: &mut ratatui::Frame<'_>,
            _slots: &[PresenterRenderSlot],
        ) -> AppResult<PresenterRenderOutcome> {
            Ok(PresenterRenderOutcome {
                drew_image: true,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
                slots: Vec::new(),
            })
        }

        fn capabilities(&self) -> PresenterCaps {
            PresenterCaps {
                backend_name: "test-presenter",
                supports_l2_cache: false,
                cell_px: None,
                preferred_max_render_scale: 2.5,
            }
        }
    }

    struct TwoPageRuntimePdf;

    impl PdfBackend for TwoPageRuntimePdf {
        fn path(&self) -> &std::path::Path {
            std::path::Path::new("two-page-runtime.pdf")
        }

        fn doc_id(&self) -> u64 {
            11
        }

        fn page_count(&self) -> usize {
            2
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            Ok((100.0, 100.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_text(&self, _page: usize) -> AppResult<String> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_positioned_text(&self, _page: usize) -> AppResult<crate::backend::TextPage> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Err(AppError::unsupported("not needed in runtime test"))
        }
    }

    #[test]
    fn spread_canvas_slots_crop_from_shared_pan_coordinate_space() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        for page in 0..2 {
            runtime.l1_cache.insert(
                RenderedPageKey::new(doc.doc_id(), page, 1.0),
                RgbaFrame {
                    width: 100,
                    height: 50,
                    pixels: vec![page as u8; 100 * 50 * 4].into(),
                },
                false,
            );
        }
        let mut pan = PanOffset {
            cells_x: 8,
            cells_y: 0,
        };

        let areas = runtime
            .try_prepare_spread_canvas_slots_from_cache(
                &doc,
                &mut presenter,
                Viewport {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 5,
                },
                VisiblePageSlots {
                    anchor_page: 0,
                    trailing_page: Some(1),
                    left_page: Some(0),
                    right_page: Some(1),
                },
                1.0,
                &mut pan,
                Some((10, 10)),
                &HighlightOverlaySnapshot::default(),
                1,
                20,
            )
            .expect("spread canvas prepare should pass")
            .expect("cached spread should prepare");

        assert_eq!(
            areas,
            [Some(Rect::new(0, 0, 2, 5)), Some(Rect::new(4, 0, 6, 5))]
        );
        assert_eq!(presenter.prepared_frame_sizes, vec![(20, 50), (60, 50)]);
        assert_eq!(presenter.prepared_viewports.len(), 2);
    }

    #[test]
    fn spread_canvas_slots_keep_slot_identity_when_left_page_is_offscreen() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        for page in 0..2 {
            runtime.l1_cache.insert(
                RenderedPageKey::new(doc.doc_id(), page, 1.0),
                RgbaFrame {
                    width: 100,
                    height: 50,
                    pixels: vec![page as u8; 100 * 50 * 4].into(),
                },
                false,
            );
        }
        let mut pan = PanOffset {
            cells_x: 12,
            cells_y: 0,
        };

        let areas = runtime
            .try_prepare_spread_canvas_slots_from_cache(
                &doc,
                &mut presenter,
                Viewport {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 5,
                },
                VisiblePageSlots {
                    anchor_page: 0,
                    trailing_page: Some(1),
                    left_page: Some(0),
                    right_page: Some(1),
                },
                1.0,
                &mut pan,
                Some((10, 10)),
                &HighlightOverlaySnapshot::default(),
                1,
                20,
            )
            .expect("spread canvas prepare should pass")
            .expect("cached spread should prepare");

        assert_eq!(areas, [None, Some(Rect::new(0, 0, 10, 5))]);
        assert_eq!(presenter.prepared_slot_pages, vec![None, Some(1)]);
    }

    #[test]
    fn spread_canvas_slots_keep_cached_slot_when_partner_page_misses() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        runtime.l1_cache.insert(
            RenderedPageKey::new(doc.doc_id(), 1, 1.0),
            RgbaFrame {
                width: 100,
                height: 50,
                pixels: vec![1; 100 * 50 * 4].into(),
            },
            false,
        );
        let mut pan = PanOffset {
            cells_x: 8,
            cells_y: 0,
        };

        let areas = runtime
            .try_prepare_spread_canvas_slots_from_cache(
                &doc,
                &mut presenter,
                Viewport {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 5,
                },
                VisiblePageSlots {
                    anchor_page: 0,
                    trailing_page: Some(1),
                    left_page: Some(0),
                    right_page: Some(1),
                },
                1.0,
                &mut pan,
                Some((10, 10)),
                &HighlightOverlaySnapshot::default(),
                1,
                20,
            )
            .expect("spread canvas prepare should pass")
            .expect("right cached slot should prepare");

        assert_eq!(areas, [None, Some(Rect::new(4, 0, 6, 5))]);
        assert_eq!(presenter.prepared_slot_pages, vec![None, Some(1)]);
    }

    #[test]
    fn spread_canvas_slots_miss_when_no_visible_cached_slots_exist() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        let mut pan = PanOffset::default();

        let areas = runtime
            .try_prepare_spread_canvas_slots_from_cache(
                &doc,
                &mut presenter,
                Viewport {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 5,
                },
                VisiblePageSlots {
                    anchor_page: 0,
                    trailing_page: Some(1),
                    left_page: Some(0),
                    right_page: Some(1),
                },
                1.0,
                &mut pan,
                Some((10, 10)),
                &HighlightOverlaySnapshot::default(),
                1,
                20,
            )
            .expect("spread canvas prepare should pass");

        assert_eq!(areas, None);
        assert!(presenter.prepared_slot_pages.is_empty());
    }

    #[test]
    fn page_slots_clamp_negative_pan_before_saving_state() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        for page in 0..2 {
            runtime.l1_cache.insert(
                RenderedPageKey::new(doc.doc_id(), page, 1.0),
                RgbaFrame {
                    width: 100,
                    height: 100,
                    pixels: vec![page as u8; 100 * 100 * 4].into(),
                },
                false,
            );
        }
        let mut pan = PanOffset {
            cells_x: -3,
            cells_y: -7,
        };
        let page_slots = [
            (
                Some(RenderedPageKey::new(doc.doc_id(), 0, 1.0)),
                Viewport {
                    x: 0,
                    y: 0,
                    width: 5,
                    height: 5,
                },
            ),
            (
                Some(RenderedPageKey::new(doc.doc_id(), 1, 1.0)),
                Viewport {
                    x: 5,
                    y: 0,
                    width: 5,
                    height: 5,
                },
            ),
        ];

        let prepared = runtime
            .try_prepare_page_slots_from_cache(
                &doc,
                &mut presenter,
                &page_slots,
                &mut pan,
                Some((10, 10)),
                true,
                &HighlightOverlaySnapshot::default(),
                1,
            )
            .expect("page slot prepare should pass");

        assert!(prepared);
        assert_eq!(pan, PanOffset::default());
        assert_eq!(presenter.prepared_slot_pages, vec![Some(0), Some(1)]);
    }

    #[test]
    fn page_slots_keep_cached_slot_when_partner_page_misses() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        runtime.l1_cache.insert(
            RenderedPageKey::new(doc.doc_id(), 1, 1.0),
            RgbaFrame {
                width: 100,
                height: 100,
                pixels: vec![1; 100 * 100 * 4].into(),
            },
            false,
        );
        let mut pan = PanOffset::default();
        let page_slots = [
            (
                Some(RenderedPageKey::new(doc.doc_id(), 0, 1.0)),
                Viewport {
                    x: 0,
                    y: 0,
                    width: 5,
                    height: 5,
                },
            ),
            (
                Some(RenderedPageKey::new(doc.doc_id(), 1, 1.0)),
                Viewport {
                    x: 5,
                    y: 0,
                    width: 5,
                    height: 5,
                },
            ),
        ];

        let prepared = runtime
            .try_prepare_page_slots_from_cache(
                &doc,
                &mut presenter,
                &page_slots,
                &mut pan,
                Some((10, 10)),
                true,
                &HighlightOverlaySnapshot::default(),
                1,
            )
            .expect("page slot prepare should pass");

        assert!(prepared);
        assert_eq!(presenter.prepared_slot_pages, vec![None, Some(1)]);
    }

    #[test]
    fn page_slots_return_miss_when_all_requested_pages_miss_l1() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        let mut pan = PanOffset::default();
        let page_slots = [(
            Some(RenderedPageKey::new(doc.doc_id(), 0, 1.0)),
            Viewport {
                x: 0,
                y: 0,
                width: 5,
                height: 5,
            },
        )];

        let prepared = runtime
            .try_prepare_page_slots_from_cache(
                &doc,
                &mut presenter,
                &page_slots,
                &mut pan,
                Some((10, 10)),
                true,
                &HighlightOverlaySnapshot::default(),
                1,
            )
            .expect("page slot prepare should pass");

        assert!(!prepared);
        assert!(presenter.prepared_slot_pages.is_empty());
    }
}
