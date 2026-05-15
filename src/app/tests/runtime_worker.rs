use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::super::runtime::RenderRuntime;
use crate::backend::test_support::{build_pdf, unique_temp_path};
use crate::backend::{OutlineNode, PdfBackend, PdfDoc, PdfRect, RgbaFrame, SharedPdfBackend};
use crate::error::{AppError, AppResult};
use crate::highlight::{HighlightOverlaySnapshot, HighlightSource, HighlightSpan, HighlightStyle};
use crate::perf::PerfStats;
use crate::presenter::{
    ImagePresenter, PanOffset, PresenterCaps, PresenterFeedback, PresenterRenderOptions,
    PresenterRenderOutcome, PresenterRenderSlot, PresenterSlot, Viewport,
};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::{NavDirection, NavIntent, RenderTask};
use crate::render::worker::RenderWorker;
use crate::work::WorkClass;

#[derive(Default)]
struct TestPresenter {
    prepare_calls: usize,
    prefetch_calls: usize,
    render_calls: usize,
    last_prepare_overlay_stamp: Option<u64>,
    prepared_viewports: Vec<Viewport>,
    prepared_frame_sizes: Vec<(u32, u32)>,
    prepared_slot_pages: Vec<Option<usize>>,
    stats: PerfStats,
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
            self.prepare_calls += 1;
            self.last_prepare_overlay_stamp = Some(slot.overlay_stamp);
            self.prepared_viewports.push(slot.viewport);
            self.prepared_frame_sizes.push((frame.width, frame.height));
            self.stats.record_convert(Duration::from_millis(4));
            self.stats.set_l2_hit_rate(0.5);
        }
        Ok(())
    }

    fn render_slots(
        &mut self,
        _frame: &mut ratatui::Frame<'_>,
        _slots: &[PresenterRenderSlot],
    ) -> AppResult<PresenterRenderOutcome> {
        self.render_calls += 1;
        self.stats.record_blit(Duration::from_millis(2));
        Ok(PresenterRenderOutcome {
            drew_image: true,
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
        _class: WorkClass,
        _generation: u64,
    ) -> AppResult<()> {
        self.prefetch_calls += 1;
        Ok(())
    }

    fn capabilities(&self) -> PresenterCaps {
        PresenterCaps {
            backend_name: "test-presenter",
            supports_l2_cache: false,
            cell_px: None,
            preferred_max_render_scale: 2.5,
        }
    }

    fn perf_snapshot(&self) -> Option<PerfStats> {
        Some(self.stats.clone())
    }
}

struct PageDimensionFailingPdf;

impl PdfBackend for PageDimensionFailingPdf {
    fn path(&self) -> &std::path::Path {
        std::path::Path::new("page-dimension-failing.pdf")
    }

    fn doc_id(&self) -> u64 {
        7
    }

    fn page_count(&self) -> usize {
        1
    }

    fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
        Err(AppError::invalid_argument("page dimensions unavailable"))
    }

    fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
        Ok(RgbaFrame {
            width: 4,
            height: 4,
            pixels: vec![255; 64].into(),
        })
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
fn schedule_navigation_updates_queue_and_cancellation_metrics() {
    let file = unique_temp_path("runtime_schedule.pdf");
    fs::write(&file, build_pdf(&["p1", "p2", "p3", "p4"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");

    let mut runtime = RenderRuntime::default();
    runtime.schedule_navigation(
        &doc,
        1,
        NavIntent {
            dir: NavDirection::Forward,
            streak: 6,
            generation: 1,
        },
        1.0,
    );
    assert!(runtime.scheduler.len() >= 4);
    assert_eq!(runtime.perf_stats.queue_depth, runtime.scheduler.len());

    let canceled_before = runtime.perf_stats.canceled_tasks;
    runtime.schedule_navigation(
        &doc,
        1,
        NavIntent {
            dir: NavDirection::Backward,
            streak: 2,
            generation: 2,
        },
        1.0,
    );
    assert!(runtime.perf_stats.canceled_tasks > canceled_before);
    assert_eq!(runtime.perf_stats.queue_depth, runtime.scheduler.len());

    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn prepare_current_page_updates_l1_and_presenter_metrics() {
    let file = unique_temp_path("runtime_render.pdf");
    fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");

    let mut runtime = RenderRuntime::default();
    let mut presenter = TestPresenter::default();
    let mut pan = PanOffset::default();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    runtime
        .prepare_current_page(
            &doc,
            &mut presenter,
            viewport,
            0,
            1.0,
            &mut pan,
            None,
            false,
            &HighlightOverlaySnapshot::default(),
        )
        .expect("first prepare should succeed");
    runtime
        .prepare_current_page(
            &doc,
            &mut presenter,
            viewport,
            0,
            1.0,
            &mut pan,
            None,
            false,
            &HighlightOverlaySnapshot::default(),
        )
        .expect("second prepare should succeed");
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal
        .draw(|frame| {
            presenter
                .render(
                    frame,
                    Rect::new(0, 0, 80, 24),
                    PresenterRenderOptions::default(),
                )
                .expect("presenter render should succeed");
        })
        .expect("test terminal draw should succeed");
    runtime.sync_presenter_metrics(&presenter);

    assert_eq!(presenter.prepare_calls, 2);
    assert_eq!(presenter.render_calls, 1);
    assert_eq!(runtime.l1_cache.len(), 1);
    assert!(runtime.perf_stats.render_samples >= 1);
    assert!(runtime.perf_stats.cache_hit_rate_l1 > 0.0);
    assert_eq!(runtime.perf_stats.convert_ms, 4.0);
    assert_eq!(runtime.perf_stats.blit_ms, 2.0);
    assert_eq!(runtime.perf_stats.cache_hit_rate_l2, 0.5);

    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn prepare_current_page_uses_zero_overlay_stamp_when_decoration_falls_back() {
    let doc = PageDimensionFailingPdf;
    let mut runtime = RenderRuntime::default();
    let mut presenter = TestPresenter::default();
    let mut pan = PanOffset::default();
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let overlay = HighlightOverlaySnapshot::new(vec![HighlightSpan {
        source: HighlightSource::Search,
        page: 0,
        rects: vec![PdfRect {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        }],
        style: HighlightStyle::SEARCH_HIT,
    }]);

    runtime
        .prepare_current_page(
            &doc,
            &mut presenter,
            viewport,
            0,
            1.0,
            &mut pan,
            None,
            false,
            &overlay,
        )
        .expect("prepare should succeed");

    assert_eq!(presenter.last_prepare_overlay_stamp, Some(0));
}

#[test]
fn run_next_prefetch_reduces_queue_depth() {
    let file = unique_temp_path("runtime_prefetch.pdf");
    fs::write(&file, build_pdf(&["a", "b", "c"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");

    let mut runtime = RenderRuntime::default();
    runtime.reset_prefetch(
        &doc,
        0,
        NavIntent {
            dir: NavDirection::Forward,
            streak: 3,
            generation: 1,
        },
        1.0,
    );
    let queued = runtime.scheduler.len();
    assert!(queued > 0);

    let task = runtime
        .run_next_prefetch(&doc)
        .expect("prefetch should run")
        .expect("task should exist");
    assert_eq!(task.doc_id, doc.doc_id());
    assert_eq!(runtime.perf_stats.queue_depth, queued - 1);

    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn prefetch_encode_from_cache_invokes_presenter() {
    let mut runtime = RenderRuntime::default();
    let mut presenter = TestPresenter::default();
    let key = RenderedPageKey::new(5, 2, 1.0);
    runtime.l1_cache.insert(
        key,
        RgbaFrame {
            width: 2,
            height: 2,
            pixels: vec![255; 16].into(),
        },
        false,
    );
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let mut pan = PanOffset::default();

    let prefetched = runtime
        .try_prefetch_encode_from_cache(
            &mut presenter,
            viewport,
            key,
            &mut pan,
            0,
            None,
            false,
            WorkClass::DirectionalLead,
            1,
        )
        .expect("prefetch from cache should succeed");
    assert!(prefetched);
    assert_eq!(presenter.prefetch_calls, 1);
}

#[test]
fn prefetch_encode_from_cache_skips_when_overlay_active() {
    let mut runtime = RenderRuntime::default();
    let mut presenter = TestPresenter::default();
    let key = RenderedPageKey::new(5, 2, 1.0);
    runtime.l1_cache.insert(
        key,
        RgbaFrame {
            width: 2,
            height: 2,
            pixels: vec![255; 16].into(),
        },
        false,
    );
    let viewport = Viewport {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let mut pan = PanOffset::default();

    let prefetched = runtime
        .try_prefetch_encode_from_cache(
            &mut presenter,
            viewport,
            key,
            &mut pan,
            1,
            None,
            false,
            WorkClass::DirectionalLead,
            1,
        )
        .expect("prefetch from cache should succeed");
    assert!(!prefetched);
    assert_eq!(presenter.prefetch_calls, 0);
}

#[test]
fn render_worker_accepts_up_to_three_inflight_tasks() {
    let file = unique_temp_path("render_worker_parallel.pdf");
    fs::write(&file, build_pdf(&["p1", "p2", "p3", "p4"])).expect("test pdf should be created");
    let doc = Arc::new(PdfDoc::open(&file).expect("pdf should open"));
    let mut worker = spawn_worker(Arc::clone(&doc), 3);

    assert!(worker.enqueue(render_task(doc.as_ref(), 0, WorkClass::CriticalCurrent, 1)));
    assert!(worker.enqueue(render_task(doc.as_ref(), 1, WorkClass::DirectionalLead, 1)));
    assert!(worker.enqueue(render_task(doc.as_ref(), 2, WorkClass::Background, 1)));
    assert!(!worker.enqueue(render_task(doc.as_ref(), 3, WorkClass::Background, 1)));
    assert_eq!(worker.in_flight_len(), 3);

    let deadline = Instant::now() + Duration::from_secs(2);
    while worker.in_flight_len() > 0 && Instant::now() < deadline {
        let _ = drain_render_results(&mut worker);
        thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(worker.in_flight_len(), 0);
    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn render_worker_rejects_duplicate_key_while_inflight() {
    let file = unique_temp_path("render_worker_dedupe.pdf");
    fs::write(&file, build_pdf(&["p1", "p2"])).expect("test pdf should be created");
    let doc = Arc::new(PdfDoc::open(&file).expect("pdf should open"));
    let mut worker = spawn_worker(Arc::clone(&doc), 3);
    let key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

    assert!(worker.enqueue(render_task(doc.as_ref(), 0, WorkClass::CriticalCurrent, 1)));
    assert!(worker.has_in_flight(&key));
    assert!(!worker.enqueue(render_task(doc.as_ref(), 0, WorkClass::DirectionalLead, 1)));

    let deadline = Instant::now() + Duration::from_secs(2);
    while worker.in_flight_len() > 0 && Instant::now() < deadline {
        let _ = drain_render_results(&mut worker);
        thread::sleep(Duration::from_millis(5));
    }

    fs::remove_file(&file).expect("test pdf should be removed");
}

fn render_task(doc: &dyn PdfBackend, page: usize, class: WorkClass, generation: u64) -> RenderTask {
    RenderTask {
        doc_id: doc.doc_id(),
        page,
        scale: 1.0,
        class,
        generation,
        reason: "test-task",
    }
}

fn spawn_worker(doc: Arc<PdfDoc>, worker_threads: usize) -> RenderWorker {
    let doc: SharedPdfBackend = doc;
    RenderWorker::spawn(doc, worker_threads)
}

fn drain_render_results(worker: &mut RenderWorker) -> Vec<RenderedPageKey> {
    let mut completed = Vec::new();
    loop {
        let Some(event) = worker.try_recv_result_event() else {
            break;
        };
        if let Some(result) = worker.accept_result_event(event) {
            completed.push(result.key);
        }
    }
    completed
}
