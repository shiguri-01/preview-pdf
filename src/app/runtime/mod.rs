use std::time::{Duration, Instant};

use crate::backend::{PdfBackend, RgbaFrame};
use crate::error::{AppError, AppResult};
use crate::perf::PerfStats;
use crate::presenter::ImagePresenter;
use crate::render::cache::{RenderedPageCache, RenderedPageKey};
use crate::render::scheduler::{
    NavIntent, PrefetchPolicy, RenderScheduler, RenderTask, build_prefetch_plan_with_policy,
};
use crate::work::WorkClass;

mod prepare;
mod spread_canvas;

#[cfg(test)]
pub(crate) use prepare::CurrentPagePrepareRequest;
pub(crate) use prepare::{
    CachePrepareResult, FramePrepareOptions, PageSlotPrepareRequest, PrefetchEncodeRequest,
    SpreadCanvasPrepareRequest,
};

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

    pub fn sync_presenter_metrics(&mut self, presenter: &dyn ImagePresenter) {
        if let Some(snapshot) = presenter.perf_snapshot() {
            self.perf_stats.absorb_presenter_metrics(&snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ratatui::layout::Rect;

    use super::*;
    use crate::backend::{OutlineNode, PdfRect};
    use crate::error::AppResult;
    use crate::highlight::{
        HighlightOverlaySnapshot, HighlightSource, HighlightSpan, HighlightStyle,
    };
    use crate::presenter::{
        ImagePresenter, PanOffset, PresenterCaps, PresenterFeedback, PresenterRenderOutcome,
        PresenterRenderSlot, PresenterSlot, Viewport,
    };
    use crate::render::cache::RenderedPageKey;

    use super::super::state::VisiblePageSlots;

    #[derive(Default)]
    struct TestPresenter {
        prepared_viewports: Vec<Viewport>,
        prepared_frame_sizes: Vec<(u32, u32)>,
        prepared_slot_pages: Vec<Option<usize>>,
        last_prepare_pans: Vec<PanOffset>,
    }

    impl ImagePresenter for TestPresenter {
        fn prepare_slots(&mut self, slots: &[PresenterSlot<'_>]) -> AppResult<()> {
            self.prepared_slot_pages = slots
                .iter()
                .map(|slot| slot.cache_key.map(|key| key.page))
                .collect();
            self.last_prepare_pans = slots.iter().map(|slot| slot.pan).collect();
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

    struct CountingDimensionsPdf {
        dimensions_calls: AtomicUsize,
    }

    impl CountingDimensionsPdf {
        fn new() -> Self {
            Self {
                dimensions_calls: AtomicUsize::new(0),
            }
        }
    }

    impl PdfBackend for CountingDimensionsPdf {
        fn path(&self) -> &std::path::Path {
            std::path::Path::new("counting-dimensions.pdf")
        }

        fn doc_id(&self) -> u64 {
            12
        }

        fn page_count(&self) -> usize {
            2
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            self.dimensions_calls.fetch_add(1, Ordering::Relaxed);
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

    fn prepare_spread_canvas(
        runtime: &mut RenderRuntime,
        doc: &TwoPageRuntimePdf,
        presenter: &mut TestPresenter,
        viewport: Viewport,
        slots: VisiblePageSlots,
        pan: &mut PanOffset,
        gap_px: u32,
    ) -> AppResult<Option<[Option<Rect>; 2]>> {
        match runtime.prepare_spread_canvas_from_cache(
            doc,
            SpreadCanvasPrepareRequest {
                viewport,
                visible_pages: slots,
                scale: 1.0,
                pan: *pan,
                cell_px: Some((10, 10)),
                overlay: &HighlightOverlaySnapshot::default(),
                gap_px,
            },
        )? {
            CachePrepareResult::Prepared(prepared) => {
                let areas = prepared.render_areas();
                let slots = prepared.presenter_slots(1);
                presenter.prepare_slots(&slots)?;
                Ok(Some(areas))
            }
            CachePrepareResult::Miss => Ok(None),
        }
    }

    fn prepare_page_slots(
        runtime: &mut RenderRuntime,
        doc: &TwoPageRuntimePdf,
        presenter: &mut TestPresenter,
        page_slots: &[(Option<RenderedPageKey>, Viewport)],
        pan: &mut PanOffset,
    ) -> AppResult<bool> {
        match runtime.prepare_page_slots_from_cache(
            doc,
            PageSlotPrepareRequest {
                page_slots,
                pan: *pan,
                options: FramePrepareOptions {
                    cell_px: Some((10, 10)),
                    crop: true,
                    overlay: &HighlightOverlaySnapshot::default(),
                },
            },
        )? {
            CachePrepareResult::Prepared(prepared) => {
                let slots = prepared.presenter_slots(1);
                presenter.prepare_slots(&slots)?;
                Ok(true)
            }
            CachePrepareResult::Miss => Ok(false),
        }
    }

    fn two_page_highlight_overlay() -> HighlightOverlaySnapshot {
        HighlightOverlaySnapshot::new(
            (0..2)
                .map(|page| HighlightSpan {
                    source: HighlightSource::Search,
                    page,
                    rects: vec![PdfRect {
                        x0: 0.0,
                        y0: 0.0,
                        x1: 10.0,
                        y1: 10.0,
                    }],
                    style: HighlightStyle::SEARCH_HIT,
                })
                .collect(),
        )
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

        let areas = prepare_spread_canvas(
            &mut runtime,
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
            &mut pan,
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
    fn spread_canvas_slots_crop_from_centered_page_y_origin() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        runtime.l1_cache.insert(
            RenderedPageKey::new(doc.doc_id(), 0, 1.0),
            RgbaFrame {
                width: 100,
                height: 100,
                pixels: vec![0; 100 * 100 * 4].into(),
            },
            false,
        );
        runtime.l1_cache.insert(
            RenderedPageKey::new(doc.doc_id(), 1, 1.0),
            RgbaFrame {
                width: 100,
                height: 40,
                pixels: vec![1; 100 * 40 * 4].into(),
            },
            false,
        );
        let mut pan = PanOffset {
            cells_x: 0,
            cells_y: 2,
        };

        let areas = prepare_spread_canvas(
            &mut runtime,
            &doc,
            &mut presenter,
            Viewport {
                x: 0,
                y: 0,
                width: 25,
                height: 5,
            },
            VisiblePageSlots {
                anchor_page: 0,
                trailing_page: Some(1),
                left_page: Some(0),
                right_page: Some(1),
            },
            &mut pan,
            20,
        )
        .expect("spread canvas prepare should pass")
        .expect("cached spread should prepare");

        assert_eq!(
            areas,
            [Some(Rect::new(0, 0, 10, 5)), Some(Rect::new(12, 1, 10, 4))]
        );
        assert_eq!(presenter.prepared_frame_sizes, vec![(100, 50), (100, 40)]);
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

        let areas = prepare_spread_canvas(
            &mut runtime,
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
            &mut pan,
            20,
        )
        .expect("spread canvas prepare should pass")
        .expect("cached spread should prepare");

        assert_eq!(areas, [None, Some(Rect::new(0, 0, 10, 5))]);
        assert_eq!(presenter.prepared_slot_pages, vec![None, Some(1)]);
    }

    #[test]
    fn spread_canvas_slots_keep_pending_slot_when_partner_page_misses() {
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
        let mut pan = PanOffset {
            cells_x: 8,
            cells_y: 0,
        };

        let areas = prepare_spread_canvas(
            &mut runtime,
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
            &mut pan,
            20,
        )
        .expect("spread canvas prepare should pass")
        .expect("right cached slot should prepare");

        assert_eq!(
            areas,
            [Some(Rect::new(0, 0, 2, 5)), Some(Rect::new(4, 0, 6, 5))]
        );
        assert_eq!(presenter.prepared_slot_pages, vec![None, Some(1)]);
    }

    #[test]
    fn spread_canvas_slots_miss_when_no_visible_cached_slots_exist() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        let mut pan = PanOffset {
            cells_x: -3,
            cells_y: -7,
        };

        let areas = prepare_spread_canvas(
            &mut runtime,
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
            &mut pan,
            20,
        )
        .expect("spread canvas prepare should pass");

        assert_eq!(areas, None);
        assert_eq!(
            pan,
            PanOffset {
                cells_x: -3,
                cells_y: -7
            }
        );
        assert!(presenter.prepared_slot_pages.is_empty());
    }

    #[test]
    fn page_slots_keep_requested_negative_pan_while_presenting_effective_pan() {
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

        let prepared =
            prepare_page_slots(&mut runtime, &doc, &mut presenter, &page_slots, &mut pan)
                .expect("page slot prepare should pass");

        assert!(prepared);
        assert_eq!(
            pan,
            PanOffset {
                cells_x: -3,
                cells_y: -7
            }
        );
        assert_eq!(
            presenter.last_prepare_pans,
            vec![PanOffset::default(), PanOffset::default()]
        );
        assert_eq!(presenter.prepared_slot_pages, vec![Some(0), Some(1)]);
    }

    #[test]
    fn page_slots_prepare_clamped_pan_once_per_cached_slot() {
        let doc = CountingDimensionsPdf::new();
        let mut runtime = RenderRuntime::default();
        let overlay = two_page_highlight_overlay();
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

        let result = runtime
            .prepare_page_slots_from_cache(
                &doc,
                PageSlotPrepareRequest {
                    page_slots: &page_slots,
                    pan: PanOffset {
                        cells_x: -3,
                        cells_y: -7,
                    },
                    options: FramePrepareOptions {
                        cell_px: Some((10, 10)),
                        crop: true,
                        overlay: &overlay,
                    },
                },
            )
            .expect("page slot prepare should pass");

        assert!(matches!(result, CachePrepareResult::Prepared(_)));
        assert_eq!(doc.dimensions_calls.load(Ordering::Relaxed), 2);
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

        let prepared =
            prepare_page_slots(&mut runtime, &doc, &mut presenter, &page_slots, &mut pan)
                .expect("page slot prepare should pass");

        assert!(prepared);
        assert_eq!(
            pan,
            PanOffset {
                cells_x: -3,
                cells_y: -7
            }
        );
        assert_eq!(
            presenter.last_prepare_pans,
            vec![PanOffset::default(), PanOffset::default()]
        );
        assert_eq!(presenter.prepared_slot_pages, vec![None, Some(1)]);
    }

    #[test]
    fn page_slots_return_miss_when_all_requested_pages_miss_l1() {
        let doc = TwoPageRuntimePdf;
        let mut runtime = RenderRuntime::default();
        let mut presenter = TestPresenter::default();
        let mut pan = PanOffset {
            cells_x: -3,
            cells_y: -7,
        };
        let page_slots = [(
            Some(RenderedPageKey::new(doc.doc_id(), 0, 1.0)),
            Viewport {
                x: 0,
                y: 0,
                width: 5,
                height: 5,
            },
        )];

        let prepared =
            prepare_page_slots(&mut runtime, &doc, &mut presenter, &page_slots, &mut pan)
                .expect("page slot prepare should pass");

        assert!(!prepared);
        assert_eq!(
            pan,
            PanOffset {
                cells_x: -3,
                cells_y: -7
            }
        );
        assert!(presenter.prepared_slot_pages.is_empty());
    }
}
