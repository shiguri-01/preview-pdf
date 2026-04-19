use std::time::{Duration, Instant};

use crate::backend::{PdfBackend, RgbaFrame};
use crate::config::CacheConfig;
use crate::error::{AppError, AppResult};
use crate::highlight::HighlightOverlaySnapshot;
use crate::perf::PerfStats;
use crate::presenter::{ImagePresenter, PanOffset, Viewport};
use crate::render::cache::{RenderedPageCache, RenderedPageKey};
use crate::render::scheduler::{
    NavIntent, PrefetchPolicy, RenderScheduler, RenderTask, build_prefetch_plan_with_policy,
};
use crate::work::WorkClass;

use super::frame_ops::{
    PageRenderSpace, apply_highlight_overlay, compose_spread_frame, prepare_presenter_frame,
};
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
        overlay: &HighlightOverlaySnapshot,
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
        let (decorated, overlay_stamp) =
            decorate_spread_frame(doc, slots, left_frame, right_frame, gap_px, overlay);
        let (frame, pan_for_presenter) =
            prepare_presenter_frame(&decorated, viewport, pan, cell_px, enable_crop);
        presenter.prepare(
            presenter_key,
            &frame,
            viewport,
            pan_for_presenter,
            overlay_stamp,
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

    pub fn sync_presenter_metrics(&mut self, presenter: &dyn ImagePresenter) {
        if let Some(snapshot) = presenter.perf_snapshot() {
            self.perf_stats.absorb_presenter_metrics(&snapshot);
        }
    }
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

fn decorate_spread_frame(
    doc: &dyn PdfBackend,
    slots: VisiblePageSlots,
    left_frame: Option<&RgbaFrame>,
    right_frame: Option<&RgbaFrame>,
    gap_px: u32,
    overlay: &HighlightOverlaySnapshot,
) -> (RgbaFrame, u64) {
    let spread_frame = compose_spread_frame(left_frame, right_frame, gap_px);
    if overlay.is_empty() {
        return (spread_frame, 0);
    }
    match spread_render_spaces(doc, slots, left_frame, right_frame, gap_px) {
        Ok(pages) if !pages.is_empty() => (
            decorate_frame(&spread_frame, overlay, &pages),
            overlay.stamp,
        ),
        _ => (spread_frame, 0),
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

fn spread_render_spaces(
    doc: &dyn PdfBackend,
    slots: VisiblePageSlots,
    left_frame: Option<&RgbaFrame>,
    right_frame: Option<&RgbaFrame>,
    gap_px: u32,
) -> AppResult<Vec<PageRenderSpace>> {
    let mut pages = Vec::new();
    if let (Some(page), Some(frame)) = (slots.left_page, left_frame)
        && let Ok((width_pt, height_pt)) = doc.page_dimensions(page)
    {
        pages.push(PageRenderSpace {
            page,
            origin_x_px: 0,
            origin_y_px: 0,
            width_px: frame.width,
            height_px: frame.height,
            width_pt,
            height_pt,
        });
    }
    if let (Some(page), Some(frame)) = (slots.right_page, right_frame) {
        let origin_x_px = spread_left_slot_width_px(left_frame, right_frame).saturating_add(gap_px);
        if let Ok((width_pt, height_pt)) = doc.page_dimensions(page) {
            pages.push(PageRenderSpace {
                page,
                origin_x_px,
                origin_y_px: 0,
                width_px: frame.width,
                height_px: frame.height,
                width_pt,
                height_pt,
            });
        }
    }
    Ok(pages)
}

fn spread_left_slot_width_px(
    left_frame: Option<&RgbaFrame>,
    right_frame: Option<&RgbaFrame>,
) -> u32 {
    left_frame
        .map(|frame| frame.width)
        .or_else(|| right_frame.map(|frame| frame.width))
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::spread_render_spaces;
    use crate::app::state::VisiblePageSlots;
    use crate::backend::{OutlineNode, PdfBackend, RgbaFrame, TextPage};
    use crate::error::{AppError, AppResult};

    #[derive(Debug)]
    struct StubBackend {
        path: PathBuf,
    }

    impl Default for StubBackend {
        fn default() -> Self {
            Self {
                path: PathBuf::from("stub.pdf"),
            }
        }
    }

    impl PdfBackend for StubBackend {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            1
        }

        fn page_count(&self) -> usize {
            2
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            Ok((100.0, 200.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_text(&self, _page: usize) -> AppResult<String> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_positioned_text(&self, _page: usize) -> AppResult<TextPage> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Err(AppError::unsupported("not needed in runtime test"))
        }
    }

    #[test]
    fn spread_render_spaces_offsets_right_page_after_blank_left_slot() {
        let doc = StubBackend::default();
        let right = RgbaFrame {
            width: 120,
            height: 240,
            pixels: vec![0; 120 * 240 * 4].into(),
        };
        let spaces = spread_render_spaces(
            &doc,
            VisiblePageSlots {
                anchor_page: 1,
                trailing_page: None,
                left_page: None,
                right_page: Some(1),
            },
            None,
            Some(&right),
            8,
        )
        .expect("spread spaces should resolve");

        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].page, 1);
        assert_eq!(spaces[0].origin_x_px, 128);
    }

    #[derive(Debug)]
    struct PartialDimensionsBackend {
        path: PathBuf,
    }

    impl Default for PartialDimensionsBackend {
        fn default() -> Self {
            Self {
                path: PathBuf::from("partial-dimensions.pdf"),
            }
        }
    }

    impl PdfBackend for PartialDimensionsBackend {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            2
        }

        fn page_count(&self) -> usize {
            2
        }

        fn page_dimensions(&self, page: usize) -> AppResult<(f32, f32)> {
            match page {
                0 => Err(AppError::unsupported("missing page dimensions")),
                1 => Ok((120.0, 240.0)),
                _ => Err(AppError::invalid_argument("unexpected page")),
            }
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_text(&self, _page: usize) -> AppResult<String> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_positioned_text(&self, _page: usize) -> AppResult<TextPage> {
            Err(AppError::unsupported("not needed in runtime test"))
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Err(AppError::unsupported("not needed in runtime test"))
        }
    }

    #[test]
    fn spread_render_spaces_skips_page_dimension_failures_per_page() {
        let doc = PartialDimensionsBackend::default();
        let left = RgbaFrame {
            width: 100,
            height: 200,
            pixels: vec![0; 100 * 200 * 4].into(),
        };
        let right = RgbaFrame {
            width: 120,
            height: 240,
            pixels: vec![0; 120 * 240 * 4].into(),
        };

        let spaces = spread_render_spaces(
            &doc,
            VisiblePageSlots {
                anchor_page: 0,
                trailing_page: Some(1),
                left_page: Some(0),
                right_page: Some(1),
            },
            Some(&left),
            Some(&right),
            8,
        )
        .expect("spread spaces should still resolve when one page fails");

        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].page, 1);
        assert_eq!(spaces[0].origin_x_px, 108);
    }
}
