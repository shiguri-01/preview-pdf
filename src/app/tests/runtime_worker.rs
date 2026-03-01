use std::fs;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::super::runtime::RenderRuntime;
use crate::backend::{PdfDoc, RgbaFrame};
use crate::error::AppResult;
use crate::perf::PerfStats;
use crate::presenter::{ImagePresenter, PanOffset, PresenterCaps, Viewport};
use crate::render::cache::RenderedPageKey;
use crate::render::prefetch::PrefetchClass;
use crate::render::scheduler::{NavDirection, NavIntent, RenderPriority, RenderTask};
use crate::render::worker::RenderWorker;

#[derive(Default)]
struct TestPresenter {
    prepare_calls: usize,
    prefetch_calls: usize,
    render_calls: usize,
    stats: PerfStats,
}

impl ImagePresenter for TestPresenter {
    fn prepare(
        &mut self,
        _cache_key: RenderedPageKey,
        _frame: &RgbaFrame,
        _viewport: Viewport,
        _pan: PanOffset,
        _generation: u64,
    ) -> AppResult<()> {
        self.prepare_calls += 1;
        self.stats.record_convert(Duration::from_millis(4));
        self.stats.set_l2_hit_rate(0.5);
        Ok(())
    }

    fn render(&mut self, _frame: &mut ratatui::Frame<'_>, _area: Rect) -> AppResult<bool> {
        self.render_calls += 1;
        self.stats.record_blit(Duration::from_millis(2));
        Ok(true)
    }

    fn prefetch_encode(
        &mut self,
        _cache_key: RenderedPageKey,
        _frame: &RgbaFrame,
        _viewport: Viewport,
        _pan: PanOffset,
        _class: PrefetchClass,
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
        )
        .expect("second prepare should succeed");
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    terminal
        .draw(|frame| {
            presenter
                .render(frame, Rect::new(0, 0, 80, 24))
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
            None,
            false,
            PrefetchClass::DirectionalLead,
            1,
        )
        .expect("prefetch from cache should succeed");
    assert!(prefetched);
    assert_eq!(presenter.prefetch_calls, 1);
}

#[test]
fn render_worker_accepts_up_to_three_inflight_tasks() {
    let file = unique_temp_path("render_worker_parallel.pdf");
    fs::write(&file, build_pdf(&["p1", "p2", "p3", "p4"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");
    let mut worker = spawn_worker(file.clone(), doc.doc_id(), 3);

    assert!(worker.enqueue(render_task(&doc, 0, RenderPriority::CriticalCurrent, 1)));
    assert!(worker.enqueue(render_task(&doc, 1, RenderPriority::DirectionalLead, 1)));
    assert!(worker.enqueue(render_task(&doc, 2, RenderPriority::Background, 1)));
    assert!(!worker.enqueue(render_task(&doc, 3, RenderPriority::Background, 1)));
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
    let doc = PdfDoc::open(&file).expect("pdf should open");
    let mut worker = spawn_worker(file.clone(), doc.doc_id(), 3);
    let key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

    assert!(worker.enqueue(render_task(&doc, 0, RenderPriority::CriticalCurrent, 1)));
    assert!(worker.has_in_flight(&key));
    assert!(!worker.enqueue(render_task(&doc, 0, RenderPriority::DirectionalLead, 1)));

    let deadline = Instant::now() + Duration::from_secs(2);
    while worker.in_flight_len() > 0 && Instant::now() < deadline {
        let _ = drain_render_results(&mut worker);
        thread::sleep(Duration::from_millis(5));
    }

    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn current_enqueue_preempts_prefetch_when_worker_is_full() {
    let file = unique_temp_path("render_worker_preempt_current.pdf");
    fs::write(&file, build_pdf(&["p1", "p2"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");
    let mut worker = spawn_worker(file.clone(), doc.doc_id(), 1);
    let old_key = RenderedPageKey::new(doc.doc_id(), 1, 1.0);
    let current_key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

    assert!(worker.enqueue(render_task(&doc, 1, RenderPriority::Background, 1)));
    let (enqueued, preempted) = worker.enqueue_current_with_preemption(
        render_task(&doc, 0, RenderPriority::CriticalCurrent, 2),
        2,
        current_key,
    );
    assert!(!enqueued);
    assert_eq!(preempted, 1);
    assert!(worker.has_in_flight(&old_key));
    assert!(!worker.has_in_flight(&current_key));

    let mut completed = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while worker.in_flight_len() > 0 && Instant::now() < deadline {
        completed.extend(drain_render_results(&mut worker));
        thread::sleep(Duration::from_millis(5));
    }

    assert!(!completed.contains(&old_key));
    assert!(worker.enqueue(render_task(&doc, 0, RenderPriority::CriticalCurrent, 2)));
    let deadline = Instant::now() + Duration::from_secs(2);
    while worker.in_flight_len() > 0 && Instant::now() < deadline {
        completed.extend(drain_render_results(&mut worker));
        thread::sleep(Duration::from_millis(5));
    }
    assert!(completed.contains(&current_key));
    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn enqueue_current_with_preemption_does_not_exceed_inflight_limit() {
    let file = unique_temp_path("render_worker_preempt_limit.pdf");
    fs::write(&file, build_pdf(&["p1", "p2"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");
    let mut worker = spawn_worker(file.clone(), doc.doc_id(), 1);
    let keep_key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

    assert!(worker.enqueue(render_task(&doc, 1, RenderPriority::Background, 1)));
    for _ in 0..8 {
        let _ = worker.enqueue_current_with_preemption(
            render_task(&doc, 0, RenderPriority::CriticalCurrent, 2),
            2,
            keep_key,
        );
        assert_eq!(worker.in_flight_len(), 1);
    }

    fs::remove_file(&file).expect("test pdf should be removed");
}

#[test]
fn cancel_stale_prefetch_drops_results_for_old_generation_prefetch() {
    let file = unique_temp_path("render_worker_cancel_stale.pdf");
    fs::write(&file, build_pdf(&["p1", "p2", "p3", "p4"])).expect("test pdf should be created");
    let doc = PdfDoc::open(&file).expect("pdf should open");
    let mut worker = spawn_worker(file.clone(), doc.doc_id(), 4);
    let current_key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

    assert!(worker.enqueue(render_task(&doc, 0, RenderPriority::CriticalCurrent, 1)));
    assert!(worker.enqueue(render_task(&doc, 1, RenderPriority::DirectionalLead, 1)));
    assert!(worker.enqueue(render_task(&doc, 2, RenderPriority::Background, 1)));
    assert!(worker.enqueue(render_task(&doc, 3, RenderPriority::GuardReverse, 1)));

    let canceled = worker.cancel_stale_prefetch_except(2, Some(current_key));
    assert_eq!(canceled, 2);

    let mut completed_keys = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while worker.in_flight_len() > 0 && Instant::now() < deadline {
        completed_keys.extend(drain_render_results(&mut worker));
        thread::sleep(Duration::from_millis(5));
    }

    assert!(completed_keys.contains(&RenderedPageKey::new(doc.doc_id(), 0, 1.0)));
    assert!(completed_keys.contains(&RenderedPageKey::new(doc.doc_id(), 3, 1.0)));
    assert!(!completed_keys.contains(&RenderedPageKey::new(doc.doc_id(), 1, 1.0)));
    assert!(!completed_keys.contains(&RenderedPageKey::new(doc.doc_id(), 2, 1.0)));
    fs::remove_file(&file).expect("test pdf should be removed");
}

fn render_task(doc: &PdfDoc, page: usize, priority: RenderPriority, generation: u64) -> RenderTask {
    RenderTask {
        doc_id: doc.doc_id(),
        page,
        scale: 1.0,
        priority,
        generation,
        reason: "test-task",
    }
}

fn spawn_worker(path: PathBuf, doc_id: u64, worker_threads: usize) -> RenderWorker {
    RenderWorker::spawn(path, doc_id, worker_threads)
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

fn unique_temp_path(suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    let mut path = std::env::temp_dir();
    path.push(format!("pvf_{suffix}_{}_{}", process::id(), nanos));
    path
}

fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
    let page_texts = if page_texts.is_empty() {
        vec![""]
    } else {
        page_texts.to_vec()
    };

    let page_count = page_texts.len();
    let page_ids: Vec<usize> = (0..page_count).map(|i| 4 + i * 2).collect();

    let mut objects = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());

    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

    for (index, text) in page_texts.iter().enumerate() {
        let content_id = 5 + index * 2;
        let escaped = escape_literal_string(text);
        let stream = format!("BT /F1 14 Tf 36 260 Td ({escaped}) Tj ET");

        let page_obj = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
        );
        let content_obj = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        );

        objects.push(page_obj);
        objects.push(content_obj);
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let mut offsets = Vec::new();
    offsets.push(0_usize);
    for (index, object) in objects.iter().enumerate() {
        let object_id = index + 1;
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{object_id} 0 obj\n{object}\nendobj\n").as_bytes());
    }

    let xref_start = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }

    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_start
        )
        .as_bytes(),
    );

    bytes
}

fn escape_literal_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }

    out
}
