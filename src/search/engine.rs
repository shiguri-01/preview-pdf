use std::sync::Arc;

use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};
use tokio::task::JoinHandle;

use crate::backend::{PdfRect, SharedPdfBackend, TextPage};
use crate::error::{AppError, AppResult};

use super::matcher::SearchMatcher;
use super::worker::{
    GeometryJob, GeometryPriority, PrewarmJob, SearchJob, WorkerRequest, worker_main,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSnapshot {
    pub generation: u64,
    pub scanned_pages: usize,
    pub total_pages: usize,
    pub hit_pages: usize,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchEvent {
    Snapshot(SearchSnapshot),
    Completed {
        generation: u64,
        hits: Vec<SearchPageHit>,
        highlight_unavailable: bool,
    },
    GeometryResolved {
        generation: u64,
        page: usize,
        occurrences: Vec<SearchOccurrence>,
        highlight_unavailable: bool,
    },
    Failed {
        generation: u64,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchOccurrence {
    pub match_start: usize,
    pub match_end: usize,
    pub rects: Vec<PdfRect>,
    pub snippet: String,
    pub snippet_match_start: Option<usize>,
    pub snippet_match_end: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPageHit {
    pub page: usize,
    pub occurrences: Vec<SearchOccurrence>,
}

pub struct SearchEngine {
    request_tx: UnboundedSender<WorkerRequest>,
    event_rx: UnboundedReceiver<SearchEvent>,
    next_generation: u64,
    _runtime: SearchWorkerRuntime,
    worker: Option<JoinHandle<()>>,
}

struct SearchWorkerRuntime {
    _owned: Option<Runtime>,
    handle: Handle,
}

impl SearchWorkerRuntime {
    fn new() -> Self {
        if let Ok(handle) = Handle::try_current() {
            return Self {
                _owned: None,
                handle,
            };
        }

        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("pvf-search")
            .build()
            .expect("search runtime should initialize");
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

impl Default for SearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchEngine {
    pub fn new() -> Self {
        let (request_tx, request_rx) = unbounded_channel();
        let (event_tx, event_rx) = unbounded_channel();
        let runtime = SearchWorkerRuntime::new();
        let worker = runtime.spawn_blocking(move || worker_main(request_rx, event_tx));

        Self {
            request_tx,
            event_rx,
            next_generation: 0,
            _runtime: runtime,
            worker: Some(worker),
        }
    }

    pub fn submit(
        &mut self,
        pdf: SharedPdfBackend,
        query: impl Into<String>,
        matcher: Arc<dyn SearchMatcher>,
    ) -> AppResult<u64> {
        self.next_generation = self.next_generation.saturating_add(1);

        let generation = self.next_generation;
        let job = SearchJob {
            generation,
            pdf,
            query: query.into(),
            matcher,
        };

        self.request_tx
            .send(WorkerRequest::Query(job))
            .map_err(|_| AppError::unsupported("search worker is not available"))?;

        Ok(generation)
    }

    pub fn cancel(&mut self, pdf: SharedPdfBackend) -> AppResult<u64> {
        self.submit(pdf, String::new(), Arc::new(CancelMatcher))
    }

    pub fn prewarm(&mut self, pdf: SharedPdfBackend) {
        let _ = self
            .request_tx
            .send(WorkerRequest::Prewarm(PrewarmJob { pdf }));
    }

    pub fn resolve_geometry(
        &mut self,
        pdf: SharedPdfBackend,
        generation: u64,
        query: impl Into<String>,
        matcher: Arc<dyn SearchMatcher>,
        pages: Vec<usize>,
        high_priority: bool,
    ) {
        if pages.is_empty() {
            return;
        }
        let priority = if high_priority {
            GeometryPriority::High
        } else {
            GeometryPriority::Background
        };
        let _ = self
            .request_tx
            .send(WorkerRequest::ResolveGeometry(GeometryJob {
                generation,
                pdf,
                query: query.into(),
                matcher,
                pages,
                priority,
            }));
    }

    pub fn drain_events(&mut self) -> Vec<SearchEvent> {
        let mut drained = Vec::new();

        loop {
            match self.event_rx.try_recv() {
                Ok(event) => drained.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        drained
    }
}

#[derive(Debug)]
struct CancelMatcher;

impl SearchMatcher for CancelMatcher {
    fn prepare_query(&self, _raw_query: &str) -> String {
        String::new()
    }

    fn matches_page(&self, _page_text: &str, _prepared_query: &str) -> bool {
        false
    }

    fn locate_text_matches(
        &self,
        _page_text: &str,
        _prepared_query: &str,
    ) -> Vec<SearchOccurrence> {
        Vec::new()
    }

    fn locate_matches(&self, _page: &TextPage, _prepared_query: &str) -> Vec<SearchOccurrence> {
        Vec::new()
    }
}

impl Drop for SearchEngine {
    fn drop(&mut self) {
        let _ = self.request_tx.send(WorkerRequest::Shutdown);
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}
