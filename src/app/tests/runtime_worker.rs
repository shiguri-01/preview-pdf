use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::backend::test_support::{build_pdf, unique_temp_path};
use crate::backend::{PdfBackend, PdfDoc, SharedPdfBackend};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::RenderTask;
use crate::render::worker::RenderWorker;
use crate::work::WorkClass;

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
