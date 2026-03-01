use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::backend::{
    PdfBackend, RgbaFrame, load_default_shared_bytes, open_default_backend_with_shared_bytes,
};
use crate::error::{AppError, AppResult};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::{RenderPriority, RenderTask};

enum RenderWorkerRequest {
    Task { task_id: u64, task: RenderTask },
    Shutdown,
}

pub trait RenderPdfLoader: Send + Sync {
    fn load_shared_bytes(&self, path: &Path) -> AppResult<Arc<Vec<u8>>>;
    fn open_with_shared_bytes(
        &self,
        path: &Path,
        bytes: Arc<Vec<u8>>,
    ) -> AppResult<Box<dyn PdfBackend>>;
}

#[derive(Debug, Default)]
pub struct HayroRenderPdfLoader;

impl RenderPdfLoader for HayroRenderPdfLoader {
    fn load_shared_bytes(&self, path: &Path) -> AppResult<Arc<Vec<u8>>> {
        load_default_shared_bytes(path)
    }

    fn open_with_shared_bytes(
        &self,
        path: &Path,
        bytes: Arc<Vec<u8>>,
    ) -> AppResult<Box<dyn PdfBackend>> {
        open_default_backend_with_shared_bytes(path, bytes)
    }
}

#[derive(Debug)]
pub(crate) struct RenderWorkerResult {
    pub(crate) key: RenderedPageKey,
    pub(crate) priority: RenderPriority,
    pub(crate) generation: u64,
    pub(crate) result: AppResult<RgbaFrame>,
    pub(crate) elapsed: Duration,
}

#[derive(Debug)]
pub(crate) struct RenderResultEvent {
    pub(crate) task_id: u64,
    pub(crate) key: RenderedPageKey,
    pub(crate) priority: RenderPriority,
    pub(crate) generation: u64,
    pub(crate) result: AppResult<RgbaFrame>,
    pub(crate) elapsed: Duration,
}

pub(crate) struct RenderWorker {
    request_tx: UnboundedSender<RenderWorkerRequest>,
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
    priority: RenderPriority,
    generation: u64,
    canceled: bool,
}

impl RenderWorker {
    pub(crate) fn spawn(path: PathBuf, doc_id: u64, worker_threads: usize) -> Self {
        Self::spawn_with_loader(path, doc_id, worker_threads, Arc::new(HayroRenderPdfLoader))
    }

    pub(crate) fn spawn_with_loader(
        path: PathBuf,
        doc_id: u64,
        worker_threads: usize,
        loader: Arc<dyn RenderPdfLoader>,
    ) -> Self {
        let (request_tx, request_rx) = unbounded_channel();
        let (result_tx, result_rx) = unbounded_channel();
        let runtime = RenderWorkerRuntime::new();
        let worker_threads = worker_threads.max(1);
        let request_rx = Arc::new(Mutex::new(request_rx));
        let shared_pdf_bytes = loader
            .load_shared_bytes(&path)
            .map_err(|err| err.to_string());
        let mut workers = Vec::with_capacity(worker_threads);
        for _ in 0..worker_threads {
            let path = path.clone();
            let request_rx = Arc::clone(&request_rx);
            let shared_pdf_bytes = shared_pdf_bytes.clone();
            let loader = Arc::clone(&loader);
            let result_tx = result_tx.clone();
            let worker = runtime.spawn_blocking(move || {
                render_worker_main(
                    path,
                    doc_id,
                    shared_pdf_bytes,
                    request_rx,
                    loader,
                    result_tx,
                )
            });
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
        let priority = task.priority;
        let generation = task.generation;
        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);

        if self
            .request_tx
            .send(RenderWorkerRequest::Task { task_id, task })
            .is_err()
        {
            return false;
        }
        self.in_flight.insert(
            key,
            InFlightTask {
                task_id,
                priority,
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
        keep_key: RenderedPageKey,
    ) -> (bool, usize) {
        let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
        if self.in_flight.contains_key(&key) {
            return (true, 0);
        }

        if self.in_flight.len() >= self.worker_threads {
            let Some(victim_key) = self.select_preemptable_prefetch(current_generation, keep_key)
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
        keep_key: RenderedPageKey,
    ) -> Option<RenderedPageKey> {
        self.in_flight
            .iter()
            .filter_map(|(key, entry)| {
                if *key == keep_key {
                    return None;
                }
                let class_rank = match entry.priority {
                    RenderPriority::Background => 0,
                    RenderPriority::DirectionalLead => 1,
                    _ => return None,
                };
                let stale_rank = if entry.generation < current_generation {
                    0
                } else {
                    1
                };
                Some((
                    (stale_rank, class_rank, entry.generation, entry.task_id),
                    *key,
                ))
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
        keep_key: Option<RenderedPageKey>,
    ) -> usize {
        let mut canceled = 0;
        for (key, entry) in &mut self.in_flight {
            let stale = entry.generation < generation;
            let should_keep = keep_key.is_some_and(|keep| keep == *key);
            let prefetch = matches!(
                entry.priority,
                RenderPriority::DirectionalLead | RenderPriority::Background
            );
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
            priority: result.priority,
            generation: result.generation,
            result: result.result,
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
    path: PathBuf,
    doc_id: u64,
    shared_pdf_bytes: Result<Arc<Vec<u8>>, String>,
    request_rx: Arc<Mutex<UnboundedReceiver<RenderWorkerRequest>>>,
    loader: Arc<dyn RenderPdfLoader>,
    result_tx: UnboundedSender<RenderResultEvent>,
) {
    let doc = match shared_pdf_bytes {
        Ok(bytes) => loader.open_with_shared_bytes(&path, bytes),
        Err(message) => Err(AppError::unsupported(format!(
            "render worker failed to load shared document bytes: {message}"
        ))),
    };
    loop {
        let request = match request_rx.lock() {
            Ok(mut request_rx) => request_rx.blocking_recv(),
            Err(_) => None,
        };
        let request = match request {
            Some(request) => request,
            None => break,
        };

        match request {
            RenderWorkerRequest::Task { task_id, task } => {
                let key = RenderedPageKey::new(task.doc_id, task.page, task.scale);
                let started = Instant::now();
                let result = match &doc {
                    Ok(doc) => {
                        if doc.doc_id() != doc_id || task.doc_id != doc_id {
                            Err(AppError::invalid_argument(
                                "render task does not match active document",
                            ))
                        } else {
                            doc.render_page(task.page, task.scale)
                                .map_err(|err| AppError::pdf_render(task.page, err))
                        }
                    }
                    Err(err) => Err(AppError::unsupported(format!(
                        "render worker failed to open active document: {err}"
                    ))),
                };

                let event = RenderResultEvent {
                    task_id,
                    key,
                    priority: task.priority,
                    generation: task.generation,
                    result,
                    elapsed: started.elapsed(),
                };

                let _ = result_tx.send(event);
            }
            RenderWorkerRequest::Shutdown => break,
        }
    }
}
