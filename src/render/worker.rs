use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flume::{Receiver, Sender};
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::backend::{RgbaFrame, SharedPdfBackend};
use crate::error::{AppError, AppResult};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::RenderTask;
use crate::work::WorkClass;

enum RenderWorkerRequest {
    Task {
        task_id: u64,
        task: RenderTask,
        enqueued_at: Instant,
    },
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct RenderWorkerResult {
    pub(crate) key: RenderedPageKey,
    pub(crate) class: WorkClass,
    pub(crate) generation: u64,
    pub(crate) result: AppResult<RgbaFrame>,
    pub(crate) queue_wait: Duration,
    pub(crate) elapsed: Duration,
}

#[derive(Debug)]
pub(crate) struct RenderResultEvent {
    pub(crate) task_id: u64,
    pub(crate) key: RenderedPageKey,
    pub(crate) class: WorkClass,
    pub(crate) generation: u64,
    pub(crate) result: AppResult<RgbaFrame>,
    pub(crate) queue_wait: Duration,
    pub(crate) elapsed: Duration,
}

pub(crate) struct RenderWorker {
    request_tx: Sender<RenderWorkerRequest>,
    result_rx: UnboundedReceiver<RenderResultEvent>,
    in_flight: HashMap<RenderedPageKey, InFlightTask>,
    _runtime: RenderWorkerRuntime,
    workers: Vec<JoinHandle<()>>,
    worker_threads: usize,
    next_task_id: u64,
}

struct RenderWorkerRuntime {
    _owned: Option<Runtime>,
    handle: Handle,
}

impl RenderWorkerRuntime {
    fn new() -> Self {
        if let Ok(handle) = Handle::try_current() {
            return Self {
                _owned: None,
                handle,
            };
        }

        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("pvf-render")
            .build()
            .expect("render runtime should initialize");
        let handle = runtime.handle().clone();
        Self {
            _owned: Some(runtime),
            handle,
        }
    }

    fn spawn_blocking<F>(&self, task: F) -> JoinHandle<()>
    where
        F: FnOnce() + Send + 'static,
    {
        self.handle.spawn_blocking(task)
    }
}

#[derive(Debug, Clone, Copy)]
struct InFlightTask {
    task_id: u64,
    class: WorkClass,
    generation: u64,
    canceled: bool,
}

impl RenderWorker {
    pub(crate) fn spawn(pdf: SharedPdfBackend, worker_threads: usize) -> Self {
        let (request_tx, request_rx) = flume::unbounded();
        let (result_tx, result_rx) = unbounded_channel();
        let runtime = RenderWorkerRuntime::new();
        let worker_threads = worker_threads.max(1);
        let mut workers = Vec::with_capacity(worker_threads);
        for _ in 0..worker_threads {
            let request_rx = request_rx.clone();
            let pdf = Arc::clone(&pdf);
            let result_tx = result_tx.clone();
            let worker =
                runtime.spawn_blocking(move || render_worker_main(pdf, request_rx, result_tx));
            workers.push(worker);
        }

        Self {
            request_tx,
            result_rx,
            in_flight: HashMap::new(),
            _runtime: runtime,
            workers,
            worker_threads,
            next_task_id: 1,
        }
    }

    pub(crate) fn enqueue(&mut self, task: RenderTask) -> bool {
        let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
        if self.in_flight.contains_key(&key) || self.in_flight.len() >= self.worker_threads {
            return false;
        }
        let class = task.class;
        let generation = task.generation;
        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);

        if self
            .request_tx
            .send(RenderWorkerRequest::Task {
                task_id,
                task,
                enqueued_at: Instant::now(),
            })
            .is_err()
        {
            return false;
        }
        self.in_flight.insert(
            key,
            InFlightTask {
                task_id,
                class,
                generation,
                canceled: false,
            },
        );
        true
    }

    pub(crate) fn has_in_flight(&self, key: &RenderedPageKey) -> bool {
        self.in_flight.contains_key(key)
    }

    pub(crate) fn available_slots(&self) -> usize {
        self.worker_threads.saturating_sub(self.in_flight.len())
    }

    pub(crate) fn enqueue_current_with_preemption(
        &mut self,
        task: RenderTask,
        current_generation: u64,
        keep_keys: &[RenderedPageKey],
    ) -> (bool, usize) {
        let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
        if self.in_flight.contains_key(&key) {
            return (true, 0);
        }

        if self.in_flight.len() >= self.worker_threads {
            let Some(victim_key) = self.select_preemptable_prefetch(current_generation, keep_keys)
            else {
                return (false, 0);
            };
            let marked = usize::from(self.preempt_inflight(victim_key));
            return (false, marked);
        }
        (self.enqueue(task), 0)
    }

    fn select_preemptable_prefetch(
        &self,
        current_generation: u64,
        keep_keys: &[RenderedPageKey],
    ) -> Option<RenderedPageKey> {
        self.in_flight
            .iter()
            .filter_map(|(key, entry)| {
                if keep_keys.contains(key) {
                    return None;
                }
                let rank = entry
                    .class
                    .preempt_rank(current_generation, entry.generation)?;
                Some(((rank.0, rank.1, rank.2, entry.task_id), *key))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, key)| key)
    }

    fn preempt_inflight(&mut self, key: RenderedPageKey) -> bool {
        let Some(entry) = self.in_flight.get_mut(&key) else {
            return false;
        };
        if entry.canceled {
            return false;
        }
        entry.canceled = true;
        true
    }

    pub(crate) fn cancel_stale_prefetch_except(
        &mut self,
        generation: u64,
        keep_keys: &[RenderedPageKey],
    ) -> usize {
        let mut canceled = 0;
        for (key, entry) in &mut self.in_flight {
            let stale = entry.generation < generation;
            let should_keep = keep_keys.contains(key);
            let prefetch = entry.class.is_prefetch();
            if stale && prefetch && !should_keep && !entry.canceled {
                entry.canceled = true;
                canceled += 1;
            }
        }
        canceled
    }

    pub(crate) fn accept_result_event(
        &mut self,
        result: RenderResultEvent,
    ) -> Option<RenderWorkerResult> {
        let entry = self.in_flight.remove(&result.key)?;
        if entry.task_id != result.task_id || entry.canceled {
            return None;
        }

        Some(RenderWorkerResult {
            key: result.key,
            class: result.class,
            generation: result.generation,
            result: result.result,
            queue_wait: result.queue_wait,
            elapsed: result.elapsed,
        })
    }

    pub(crate) fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    pub(crate) async fn recv_result_event(&mut self) -> Option<RenderResultEvent> {
        self.result_rx.recv().await
    }

    pub(crate) async fn recv_result(&mut self) -> Option<RenderWorkerResult> {
        while let Some(event) = self.recv_result_event().await {
            if let Some(result) = self.accept_result_event(event) {
                return Some(result);
            }
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn try_recv_result_event(&mut self) -> Option<RenderResultEvent> {
        self.result_rx.try_recv().ok()
    }

    fn shutdown(&mut self) {
        for _ in 0..self.worker_threads {
            let _ = self.request_tx.send(RenderWorkerRequest::Shutdown);
        }
        while let Some(worker) = self.workers.pop() {
            worker.abort();
        }
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn render_worker_main(
    doc: SharedPdfBackend,
    request_rx: Receiver<RenderWorkerRequest>,
    result_tx: UnboundedSender<RenderResultEvent>,
) {
    loop {
        let request = request_rx.recv();
        let request = match request {
            Ok(request) => request,
            Err(_) => break,
        };

        match request {
            RenderWorkerRequest::Task {
                task_id,
                task,
                enqueued_at,
            } => {
                let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
                let started = Instant::now();
                let result = if doc.doc_id() != task.doc_id {
                    Err(AppError::invalid_argument(
                        "render task does not match active document",
                    ))
                } else {
                    doc.render_page(task.page, task.scale)
                        .map_err(|err| AppError::pdf_render(task.page, err))
                };

                let event = RenderResultEvent {
                    task_id,
                    key,
                    class: task.class,
                    generation: task.generation,
                    result,
                    queue_wait: started.saturating_duration_since(enqueued_at),
                    elapsed: started.elapsed(),
                };

                let _ = result_tx.send(event);
            }
            RenderWorkerRequest::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::RenderWorker;
    use crate::backend::{PdfBackend, PdfDoc, SharedPdfBackend};
    use crate::render::cache::RenderedPageKey;
    use crate::render::scheduler::RenderTask;
    use crate::work::WorkClass;

    #[test]
    fn current_enqueue_preempts_prefetch_when_worker_is_full() {
        let file = unique_temp_path("render_worker_preempt_current.pdf");
        fs::write(&file, build_pdf(&["p1", "p2"])).expect("test pdf should be created");
        let doc = Arc::new(PdfDoc::open(&file).expect("pdf should open"));
        let mut worker = spawn_worker(Arc::clone(&doc), 1);
        let old_key = RenderedPageKey::new(doc.doc_id(), 1, 1.0);
        let current_key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

        assert!(worker.enqueue(render_task(doc.as_ref(), 1, WorkClass::Background, 1)));
        let (enqueued, preempted) = worker.enqueue_current_with_preemption(
            render_task(doc.as_ref(), 0, WorkClass::CriticalCurrent, 2),
            2,
            &[current_key],
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
        assert!(worker.enqueue(render_task(doc.as_ref(), 0, WorkClass::CriticalCurrent, 2)));
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
        let doc = Arc::new(PdfDoc::open(&file).expect("pdf should open"));
        let mut worker = spawn_worker(Arc::clone(&doc), 1);
        let keep_key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

        assert!(worker.enqueue(render_task(doc.as_ref(), 1, WorkClass::Background, 1)));
        for _ in 0..8 {
            let _ = worker.enqueue_current_with_preemption(
                render_task(doc.as_ref(), 0, WorkClass::CriticalCurrent, 2),
                2,
                &[keep_key],
            );
            assert_eq!(worker.in_flight_len(), 1);
        }

        fs::remove_file(&file).expect("test pdf should be removed");
    }

    #[test]
    fn cancel_stale_prefetch_drops_results_for_old_generation_prefetch() {
        let file = unique_temp_path("render_worker_cancel_stale.pdf");
        fs::write(&file, build_pdf(&["p1", "p2", "p3", "p4"])).expect("test pdf should be created");
        let doc = Arc::new(PdfDoc::open(&file).expect("pdf should open"));
        let mut worker = spawn_worker(Arc::clone(&doc), 4);
        let current_key = RenderedPageKey::new(doc.doc_id(), 0, 1.0);

        assert!(worker.enqueue(render_task(doc.as_ref(), 0, WorkClass::CriticalCurrent, 1)));
        assert!(worker.enqueue(render_task(doc.as_ref(), 1, WorkClass::DirectionalLead, 1)));
        assert!(worker.enqueue(render_task(doc.as_ref(), 2, WorkClass::Background, 1)));
        assert!(worker.enqueue(render_task(doc.as_ref(), 3, WorkClass::GuardReverse, 1)));

        let canceled = worker.cancel_stale_prefetch_except(2, &[current_key]);
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

    fn render_task(
        doc: &dyn PdfBackend,
        page: usize,
        class: WorkClass,
        generation: u64,
    ) -> RenderTask {
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
}
