use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};
use tokio::task::JoinHandle;

use crate::backend::{PdfBackend, open_default_backend};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSnapshot {
    pub generation: u64,
    pub scanned_pages: usize,
    pub total_pages: usize,
    pub hit_pages: usize,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEvent {
    Snapshot(SearchSnapshot),
    Completed { generation: u64, hits: Vec<usize> },
    Failed { generation: u64, message: String },
}

pub trait SearchMatcher: Send + Sync {
    fn prepare_query(&self, raw_query: &str) -> String;
    fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool;
}

pub trait SearchPdfLoader: Send + Sync {
    fn load(&self, path: &Path) -> AppResult<Box<dyn PdfBackend>>;
}

#[derive(Debug, Default)]
pub struct HayroSearchPdfLoader;

impl SearchPdfLoader for HayroSearchPdfLoader {
    fn load(&self, path: &Path) -> AppResult<Box<dyn PdfBackend>> {
        open_default_backend(path)
    }
}

#[derive(Clone)]
struct SearchJob {
    generation: u64,
    pdf_path: PathBuf,
    query: String,
    matcher: Arc<dyn SearchMatcher>,
}

enum WorkerRequest {
    Query(SearchJob),
    Shutdown,
}

enum WorkerControl {
    Continue,
    Shutdown,
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
        Self::new_with_loader(Arc::new(HayroSearchPdfLoader))
    }

    pub fn new_with_loader(loader: Arc<dyn SearchPdfLoader>) -> Self {
        let (request_tx, request_rx) = unbounded_channel();
        let (event_tx, event_rx) = unbounded_channel();
        let runtime = SearchWorkerRuntime::new();
        let worker = runtime.spawn_blocking(move || worker_main(request_rx, event_tx, loader));

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
        pdf_path: &Path,
        query: impl Into<String>,
        matcher: Arc<dyn SearchMatcher>,
    ) -> AppResult<u64> {
        self.next_generation = self.next_generation.saturating_add(1);

        let generation = self.next_generation;
        let job = SearchJob {
            generation,
            pdf_path: pdf_path.to_path_buf(),
            query: query.into(),
            matcher,
        };

        self.request_tx
            .send(WorkerRequest::Query(job))
            .map_err(|_| AppError::unsupported("search worker is not available"))?;

        Ok(generation)
    }

    pub fn cancel(&mut self, pdf_path: &Path) -> AppResult<u64> {
        self.submit(pdf_path, String::new(), Arc::new(CancelMatcher))
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
}

impl Drop for SearchEngine {
    fn drop(&mut self) {
        let _ = self.request_tx.send(WorkerRequest::Shutdown);
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

fn worker_main(
    mut request_rx: UnboundedReceiver<WorkerRequest>,
    event_tx: UnboundedSender<SearchEvent>,
    loader: Arc<dyn SearchPdfLoader>,
) {
    let mut pending: Option<SearchJob> = None;

    loop {
        let job = match pending.take() {
            Some(job) => job,
            None => match wait_for_job(&mut request_rx) {
                Some(job) => job,
                None => break,
            },
        };

        match run_job(
            job,
            &mut request_rx,
            &event_tx,
            &mut pending,
            loader.as_ref(),
        ) {
            WorkerControl::Continue => {}
            WorkerControl::Shutdown => break,
        }
    }
}

fn wait_for_job(request_rx: &mut UnboundedReceiver<WorkerRequest>) -> Option<SearchJob> {
    match request_rx.blocking_recv() {
        Some(WorkerRequest::Query(job)) => Some(job),
        Some(WorkerRequest::Shutdown) | None => None,
    }
}

fn run_job(
    job: SearchJob,
    request_rx: &mut UnboundedReceiver<WorkerRequest>,
    event_tx: &UnboundedSender<SearchEvent>,
    pending: &mut Option<SearchJob>,
    loader: &dyn SearchPdfLoader,
) -> WorkerControl {
    let query = job.matcher.prepare_query(job.query.trim());
    if query.is_empty() {
        let snapshot = SearchSnapshot {
            generation: job.generation,
            scanned_pages: 0,
            total_pages: 0,
            hit_pages: 0,
            done: true,
        };
        let _ = event_tx.send(SearchEvent::Snapshot(snapshot));
        let _ = event_tx.send(SearchEvent::Completed {
            generation: job.generation,
            hits: Vec::new(),
        });
        return WorkerControl::Continue;
    }

    let doc = match loader.load(&job.pdf_path) {
        Ok(doc) => doc,
        Err(err) => {
            let _ = event_tx.send(SearchEvent::Failed {
                generation: job.generation,
                message: err.to_string(),
            });
            return WorkerControl::Continue;
        }
    };

    let total_pages = doc.page_count();

    let mut hits = Vec::new();
    for page in 0..total_pages {
        match flush_requests(request_rx, pending) {
            WorkerControl::Continue => {
                if pending.is_some() {
                    return WorkerControl::Continue;
                }
            }
            WorkerControl::Shutdown => return WorkerControl::Shutdown,
        }

        let text = match doc.extract_text(page) {
            Ok(text) => text,
            Err(err) => {
                let _ = event_tx.send(SearchEvent::Failed {
                    generation: job.generation,
                    message: err.to_string(),
                });
                return WorkerControl::Continue;
            }
        };

        if job.matcher.matches_page(&text, &query) {
            hits.push(page);
        }

        let scanned_pages = page + 1;
        let snapshot = SearchSnapshot {
            generation: job.generation,
            scanned_pages,
            total_pages,
            hit_pages: hits.len(),
            done: scanned_pages == total_pages,
        };
        let _ = event_tx.send(SearchEvent::Snapshot(snapshot));
    }

    let _ = event_tx.send(SearchEvent::Completed {
        generation: job.generation,
        hits,
    });
    WorkerControl::Continue
}

fn flush_requests(
    request_rx: &mut UnboundedReceiver<WorkerRequest>,
    pending: &mut Option<SearchJob>,
) -> WorkerControl {
    loop {
        match request_rx.try_recv() {
            Ok(WorkerRequest::Query(job)) => *pending = Some(job),
            Ok(WorkerRequest::Shutdown) => return WorkerControl::Shutdown,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return WorkerControl::Shutdown,
        }
    }

    WorkerControl::Continue
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::{SearchEngine, SearchEvent, SearchMatcher};

    #[derive(Debug)]
    struct ContainsMatcher {
        case_sensitive: bool,
    }

    impl SearchMatcher for ContainsMatcher {
        fn prepare_query(&self, raw_query: &str) -> String {
            if self.case_sensitive {
                raw_query.to_string()
            } else {
                raw_query.to_lowercase()
            }
        }

        fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool {
            let prepared_page = if self.case_sensitive {
                page_text.to_string()
            } else {
                page_text.to_lowercase()
            };

            if prepared_page.contains(prepared_query) {
                return true;
            }

            remove_whitespace(&prepared_page).contains(&remove_whitespace(prepared_query))
        }
    }

    fn remove_whitespace(input: &str) -> String {
        input.chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    #[test]
    fn submit_returns_incrementing_generation() {
        let file = unique_temp_path("generation.pdf");
        fs::write(&file, build_pdf(&["one"])).expect("test file should be created");

        let mut engine = SearchEngine::new();
        let gen1 = engine
            .submit(
                &file,
                "one",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("first submit should succeed");
        let gen2 = engine
            .submit(
                &file,
                "two",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("second submit should succeed");

        assert_eq!(gen1, 1);
        assert_eq!(gen2, 2);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn cancel_enqueues_empty_query_with_new_generation() {
        let file = unique_temp_path("cancel.pdf");
        fs::write(&file, build_pdf(&["one", "two", "three"])).expect("test file should be created");

        let mut engine = SearchEngine::new();
        let running_generation = engine
            .submit(
                &file,
                "one",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");
        let cancel_generation = engine.cancel(&file).expect("cancel should succeed");

        assert_eq!(cancel_generation, running_generation + 1);
        let hits = wait_for_completed_hits(&mut engine, cancel_generation);
        assert!(hits.is_empty());

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn search_finds_hits_case_insensitively() {
        let file = unique_temp_path("hits.pdf");
        fs::write(&file, build_pdf(&["Alpha", "BETA alpha", "gamma"]))
            .expect("test file should be created");

        let mut engine = SearchEngine::new();
        let generation = engine
            .submit(
                &file,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let hits = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits, vec![0, 1]);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn search_can_use_case_sensitive_matcher() {
        let file = unique_temp_path("hits_sensitive.pdf");
        fs::write(&file, build_pdf(&["Alpha", "alpha", "ALPHA"]))
            .expect("test file should be created");

        let mut engine = SearchEngine::new();
        let generation = engine
            .submit(
                &file,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: true,
                }),
            )
            .expect("submit should succeed");

        let hits = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits, vec![1]);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn search_matches_phrase_when_extraction_omits_tj_space() {
        let file = unique_temp_path("hits_tj_gap.pdf");
        fs::write(
            &file,
            build_pdf_with_raw_streams(&["BT /F1 14 Tf 36 260 Td [(hello) -220 (world)] TJ ET"]),
        )
        .expect("test file should be created");

        let mut engine = SearchEngine::new();
        let generation = engine
            .submit(
                &file,
                "hello world",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let hits = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits, vec![0]);

        fs::remove_file(&file).expect("test file should be removed");
    }

    fn wait_for_completed_hits(engine: &mut SearchEngine, generation: u64) -> Vec<usize> {
        let timeout = Duration::from_secs(3);
        let start = Instant::now();

        loop {
            for event in engine.drain_events() {
                if let SearchEvent::Completed {
                    generation: event_generation,
                    hits,
                } = event
                    && event_generation == generation
                {
                    return hits;
                }
            }

            assert!(
                start.elapsed() <= timeout,
                "timed out waiting for search completion"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        let mut path = std::env::temp_dir();
        path.push(format!("pvf_search_{suffix}_{}_{}", process::id(), nanos));
        path
    }

    fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
        let page_texts = if page_texts.is_empty() {
            vec!["".to_string()]
        } else {
            page_texts
                .iter()
                .map(|text| {
                    let escaped = escape_literal_string(text);
                    format!("BT /F1 14 Tf 36 260 Td ({escaped}) Tj ET")
                })
                .collect()
        };

        build_pdf_from_streams(&page_texts)
    }

    fn build_pdf_with_raw_streams(page_streams: &[&str]) -> Vec<u8> {
        let page_streams = if page_streams.is_empty() {
            vec!["".to_string()]
        } else {
            page_streams
                .iter()
                .map(|stream| (*stream).to_string())
                .collect()
        };

        build_pdf_from_streams(&page_streams)
    }

    fn build_pdf_from_streams(page_streams: &[String]) -> Vec<u8> {
        let page_count = page_streams.len();
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

        for (index, stream) in page_streams.iter().enumerate() {
            let content_id = 5 + index * 2;

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
