use std::collections::HashSet;
use std::mem::size_of;
use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::backend::{SharedPdfBackend, TextGlyph, TextPage};

use super::engine::{SearchEvent, SearchPageHit, SearchSnapshot};
use super::matcher::{SearchMatcher, apply_hit_snippet, occurrence_highlight_unavailable};

#[derive(Clone)]
pub(crate) struct SearchJob {
    pub(crate) generation: u64,
    pub(crate) pdf: SharedPdfBackend,
    pub(crate) query: String,
    pub(crate) matcher: Arc<dyn SearchMatcher>,
}

#[derive(Clone)]
pub(crate) struct PrewarmJob {
    pub(crate) pdf: SharedPdfBackend,
}

#[derive(Clone)]
pub(crate) struct GeometryJob {
    pub(crate) generation: u64,
    pub(crate) pdf: SharedPdfBackend,
    pub(crate) query: String,
    pub(crate) matcher: Arc<dyn SearchMatcher>,
    pub(crate) pages: Vec<usize>,
    pub(crate) priority: GeometryPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeometryPriority {
    High,
    Background,
}

pub(crate) enum WorkerRequest {
    Query(SearchJob),
    Prewarm(PrewarmJob),
    ResolveGeometry(GeometryJob),
    Shutdown,
}

enum WorkerControl {
    Continue,
    Shutdown,
}

enum PrewarmControl {
    Finished,
    Interrupted(PrewarmJob),
    Shutdown,
}

#[derive(Default)]
struct PendingWorkerWork {
    query: Option<SearchJob>,
    geometry: Option<GeometryJob>,
    prewarm: Option<PrewarmJob>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SearchPageCacheKey {
    doc_id: u64,
    page: usize,
}

struct SearchTextPage {
    text: Arc<str>,
    estimated_bytes: usize,
}

struct SearchTextCache {
    pages: LruCache<SearchPageCacheKey, SearchTextPage>,
    memory_budget_bytes: usize,
    memory_bytes: usize,
}

struct SearchGeometryCache {
    pages: LruCache<SearchPageCacheKey, CachedTextPage>,
    memory_budget_bytes: usize,
    memory_bytes: usize,
}

#[cfg(test)]
type SearchPageCache = SearchGeometryCache;

struct CachedTextPage {
    page: Arc<TextPage>,
    estimated_bytes: usize,
}

impl SearchTextCache {
    const DEFAULT_MAX_ENTRIES: usize = 16_384;
    const DEFAULT_MEMORY_BUDGET_BYTES: usize = 48 * 1024 * 1024;

    fn new() -> Self {
        Self::with_limits(Self::DEFAULT_MAX_ENTRIES, Self::DEFAULT_MEMORY_BUDGET_BYTES)
    }

    fn with_limits(max_entries: usize, memory_budget_bytes: usize) -> Self {
        let max_entries =
            NonZeroUsize::new(max_entries).expect("search page cache entries must be non-zero");
        Self {
            pages: LruCache::new(max_entries),
            memory_budget_bytes,
            memory_bytes: 0,
        }
    }

    fn get(&mut self, doc_id: u64, page: usize) -> Option<Arc<str>> {
        self.pages
            .get(&SearchPageCacheKey { doc_id, page })
            .map(|cached| Arc::clone(&cached.text))
    }

    fn insert(&mut self, doc_id: u64, page: usize, text: Arc<str>) {
        let estimated_bytes = estimate_search_text_bytes(&text);
        let key = SearchPageCacheKey { doc_id, page };
        if let Some(replaced) = self.pages.pop(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(replaced.estimated_bytes);
        }
        if let Some((_key, removed)) = self.pages.push(
            key,
            SearchTextPage {
                text,
                estimated_bytes,
            },
        ) {
            self.memory_bytes = self.memory_bytes.saturating_sub(removed.estimated_bytes);
        }
        self.memory_bytes = self.memory_bytes.saturating_add(estimated_bytes);
        self.enforce_memory_budget();
    }

    fn try_insert_without_eviction(&mut self, doc_id: u64, page: usize, text: Arc<str>) -> bool {
        let estimated_bytes = estimate_search_text_bytes(&text);
        if self.memory_bytes.saturating_add(estimated_bytes) > self.memory_budget_bytes {
            return false;
        }

        self.insert(doc_id, page, text);
        true
    }

    fn enforce_memory_budget(&mut self) {
        while self.memory_bytes > self.memory_budget_bytes {
            let Some((_key, evicted)) = self.pages.pop_lru() else {
                self.memory_bytes = 0;
                break;
            };
            self.memory_bytes = self.memory_bytes.saturating_sub(evicted.estimated_bytes);
        }
    }
}

impl SearchGeometryCache {
    const DEFAULT_MAX_ENTRIES: usize = 16_384;
    const DEFAULT_MEMORY_BUDGET_BYTES: usize = 16 * 1024 * 1024;

    fn new() -> Self {
        Self::with_limits(Self::DEFAULT_MAX_ENTRIES, Self::DEFAULT_MEMORY_BUDGET_BYTES)
    }

    fn with_limits(max_entries: usize, memory_budget_bytes: usize) -> Self {
        let max_entries =
            NonZeroUsize::new(max_entries).expect("search geometry cache entries must be non-zero");
        Self {
            pages: LruCache::new(max_entries),
            memory_budget_bytes,
            memory_bytes: 0,
        }
    }

    fn get(&mut self, doc_id: u64, page: usize) -> Option<Arc<TextPage>> {
        self.pages
            .get(&SearchPageCacheKey { doc_id, page })
            .map(|cached| Arc::clone(&cached.page))
    }

    fn insert(&mut self, doc_id: u64, page: usize, text_page: Arc<TextPage>) {
        let estimated_bytes = estimate_text_page_bytes(&text_page);
        let key = SearchPageCacheKey { doc_id, page };
        if let Some(replaced) = self.pages.pop(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(replaced.estimated_bytes);
        }
        if let Some((_key, removed)) = self.pages.push(
            key,
            CachedTextPage {
                page: text_page,
                estimated_bytes,
            },
        ) {
            self.memory_bytes = self.memory_bytes.saturating_sub(removed.estimated_bytes);
        }
        self.memory_bytes = self.memory_bytes.saturating_add(estimated_bytes);
        self.enforce_memory_budget();
    }

    fn try_insert_without_eviction(
        &mut self,
        doc_id: u64,
        page: usize,
        text_page: Arc<TextPage>,
    ) -> bool {
        let estimated_bytes = estimate_text_page_bytes(&text_page);
        if self.memory_bytes.saturating_add(estimated_bytes) > self.memory_budget_bytes {
            return false;
        }

        self.insert(doc_id, page, text_page);
        true
    }

    fn enforce_memory_budget(&mut self) {
        while self.memory_bytes > self.memory_budget_bytes {
            let Some((_key, evicted)) = self.pages.pop_lru() else {
                self.memory_bytes = 0;
                break;
            };
            self.memory_bytes = self.memory_bytes.saturating_sub(evicted.estimated_bytes);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pages.len()
    }

    #[cfg(test)]
    fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }
}

#[cfg(test)]
pub(crate) fn estimate_search_text_bytes(text: &Arc<str>) -> usize {
    size_of::<Arc<str>>() + text.len()
}

#[cfg(not(test))]
fn estimate_search_text_bytes(text: &Arc<str>) -> usize {
    size_of::<Arc<str>>() + text.len()
}

fn estimate_text_page_bytes(text_page: &TextPage) -> usize {
    size_of::<TextPage>() + text_page.glyphs.len() * size_of::<TextGlyph>()
}

pub(crate) fn worker_main(
    mut request_rx: UnboundedReceiver<WorkerRequest>,
    event_tx: UnboundedSender<SearchEvent>,
) {
    let mut pending = PendingWorkerWork::default();
    let mut text_cache = SearchTextCache::new();
    let mut geometry_cache = SearchGeometryCache::new();
    let mut prewarm_finished_doc_ids = HashSet::new();

    loop {
        let job = match pending.query.take() {
            Some(job) => job,
            None if pending.geometry.is_some() => {
                let job = pending
                    .geometry
                    .take()
                    .expect("pending geometry checked above");
                match run_geometry_job(
                    job,
                    &mut request_rx,
                    &event_tx,
                    &mut pending,
                    &mut text_cache,
                    &mut geometry_cache,
                ) {
                    WorkerControl::Continue => {}
                    WorkerControl::Shutdown => break,
                }
                continue;
            }
            None if pending.prewarm.is_some() => {
                let job = pending
                    .prewarm
                    .take()
                    .expect("pending prewarm checked above");
                match run_prewarm_job(
                    job,
                    &mut request_rx,
                    &mut pending,
                    &mut text_cache,
                    &mut geometry_cache,
                    &mut prewarm_finished_doc_ids,
                ) {
                    PrewarmControl::Finished => {}
                    PrewarmControl::Interrupted(job) => pending.prewarm = Some(job),
                    PrewarmControl::Shutdown => break,
                }
                continue;
            }
            None => match wait_for_job(&mut request_rx) {
                Some(WorkerWork::Query(job)) => job,
                Some(WorkerWork::Geometry(job)) => {
                    match run_geometry_job(
                        job,
                        &mut request_rx,
                        &event_tx,
                        &mut pending,
                        &mut text_cache,
                        &mut geometry_cache,
                    ) {
                        WorkerControl::Continue => {}
                        WorkerControl::Shutdown => break,
                    }
                    continue;
                }
                Some(WorkerWork::Prewarm(job)) => {
                    match run_prewarm_job(
                        job,
                        &mut request_rx,
                        &mut pending,
                        &mut text_cache,
                        &mut geometry_cache,
                        &mut prewarm_finished_doc_ids,
                    ) {
                        PrewarmControl::Finished => {}
                        PrewarmControl::Interrupted(job) => pending.prewarm = Some(job),
                        PrewarmControl::Shutdown => break,
                    }
                    continue;
                }
                None => break,
            },
        };

        match run_job(
            job,
            &mut request_rx,
            &event_tx,
            &mut pending,
            &mut text_cache,
            &mut geometry_cache,
        ) {
            WorkerControl::Continue => {}
            WorkerControl::Shutdown => break,
        }
    }
}

enum WorkerWork {
    Query(SearchJob),
    Prewarm(PrewarmJob),
    Geometry(GeometryJob),
}

fn wait_for_job(request_rx: &mut UnboundedReceiver<WorkerRequest>) -> Option<WorkerWork> {
    match request_rx.blocking_recv() {
        Some(WorkerRequest::Query(job)) => Some(WorkerWork::Query(job)),
        Some(WorkerRequest::Prewarm(job)) => Some(WorkerWork::Prewarm(job)),
        Some(WorkerRequest::ResolveGeometry(job)) => Some(WorkerWork::Geometry(job)),
        Some(WorkerRequest::Shutdown) | None => None,
    }
}

fn run_prewarm_job(
    job: PrewarmJob,
    request_rx: &mut UnboundedReceiver<WorkerRequest>,
    pending: &mut PendingWorkerWork,
    text_cache: &mut SearchTextCache,
    geometry_cache: &mut SearchGeometryCache,
    prewarm_finished_doc_ids: &mut HashSet<u64>,
) -> PrewarmControl {
    let doc = job.pdf.clone();
    let total_pages = doc.page_count();
    if total_pages == 0 {
        return PrewarmControl::Finished;
    }

    let doc_id = doc.doc_id();
    if prewarm_finished_doc_ids.contains(&doc_id) {
        return PrewarmControl::Finished;
    }

    for page in 0..total_pages {
        match flush_requests(request_rx, pending) {
            WorkerControl::Continue => {
                if pending.query.is_some()
                    || pending
                        .geometry
                        .as_ref()
                        .is_some_and(|job| job.priority == GeometryPriority::High)
                {
                    return PrewarmControl::Interrupted(job);
                }
            }
            WorkerControl::Shutdown => return PrewarmControl::Shutdown,
        }
        if text_cache.get(doc_id, page).is_some() {
            continue;
        }
        if let Ok(text_page) = doc.extract_positioned_text(page) {
            let text_page = Arc::new(text_page);
            let text: Arc<str> = Arc::from(text_page.extracted_text());
            if !text_cache.try_insert_without_eviction(doc_id, page, text) {
                prewarm_finished_doc_ids.insert(doc_id);
                break;
            }
            geometry_cache.try_insert_without_eviction(doc_id, page, text_page);
        }
    }

    prewarm_finished_doc_ids.insert(doc_id);
    PrewarmControl::Finished
}

fn run_job(
    job: SearchJob,
    request_rx: &mut UnboundedReceiver<WorkerRequest>,
    event_tx: &UnboundedSender<SearchEvent>,
    pending: &mut PendingWorkerWork,
    text_cache: &mut SearchTextCache,
    geometry_cache: &mut SearchGeometryCache,
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
            highlight_unavailable: false,
        });
        return WorkerControl::Continue;
    }

    let doc = job.pdf;
    let doc_id = doc.doc_id();
    let total_pages = doc.page_count();

    let mut hits = Vec::new();
    let mut highlight_unavailable = false;
    for page in 0..total_pages {
        match flush_requests(request_rx, pending) {
            WorkerControl::Continue => {
                if pending.query.is_some() {
                    return WorkerControl::Continue;
                }
            }
            WorkerControl::Shutdown => return WorkerControl::Shutdown,
        }

        let page_text = match text_cache.get(doc_id, page) {
            Some(text) => Ok(text),
            None => doc.extract_positioned_text(page).map(|text_page| {
                let text_page = Arc::new(text_page);
                let text: Arc<str> = Arc::from(text_page.extracted_text());
                text_cache.try_insert_without_eviction(doc_id, page, Arc::clone(&text));
                geometry_cache.try_insert_without_eviction(doc_id, page, text_page);
                text
            }),
        };

        match page_text {
            Ok(text) => {
                let occurrences = match geometry_cache.get(doc_id, page) {
                    Some(text_page) => {
                        let mut occurrences =
                            job.matcher.locate_matches(text_page.as_ref(), &query);
                        for occurrence in &mut occurrences {
                            if occurrence_highlight_unavailable(occurrence, &text_page.glyphs) {
                                highlight_unavailable = true;
                            }
                            apply_hit_snippet(occurrence, &text_page.glyphs);
                        }
                        occurrences
                    }
                    None => job.matcher.locate_text_matches(&text, &query),
                };
                if !occurrences.is_empty() {
                    hits.push(SearchPageHit { page, occurrences });
                }
            }
            Err(_) => {
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
                    hits.push(SearchPageHit {
                        page,
                        occurrences: Vec::new(),
                    });
                    highlight_unavailable = true;
                }
            }
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
        highlight_unavailable,
    });
    WorkerControl::Continue
}

fn run_geometry_job(
    job: GeometryJob,
    request_rx: &mut UnboundedReceiver<WorkerRequest>,
    event_tx: &UnboundedSender<SearchEvent>,
    pending: &mut PendingWorkerWork,
    text_cache: &mut SearchTextCache,
    geometry_cache: &mut SearchGeometryCache,
) -> WorkerControl {
    let query = job.matcher.prepare_query(job.query.trim());
    if query.is_empty() {
        return WorkerControl::Continue;
    }

    let doc = job.pdf;
    let doc_id = doc.doc_id();
    let total_pages = doc.page_count();
    let mut seen = HashSet::new();

    for page in job.pages.into_iter().filter(|page| *page < total_pages) {
        if !seen.insert(page) {
            continue;
        }
        match flush_requests(request_rx, pending) {
            WorkerControl::Continue => {
                if pending.query.is_some()
                    || pending
                        .geometry
                        .as_ref()
                        .is_some_and(|queued| queued.priority == GeometryPriority::High)
                {
                    return WorkerControl::Continue;
                }
            }
            WorkerControl::Shutdown => return WorkerControl::Shutdown,
        }

        let positioned_text = match geometry_cache.get(doc_id, page) {
            Some(text_page) => Ok(text_page),
            None => doc.extract_positioned_text(page).map(|text_page| {
                let text_page = Arc::new(text_page);
                if text_cache.get(doc_id, page).is_none() {
                    let text: Arc<str> = Arc::from(text_page.extracted_text());
                    text_cache.try_insert_without_eviction(doc_id, page, text);
                }
                geometry_cache.insert(doc_id, page, Arc::clone(&text_page));
                text_page
            }),
        };

        let Ok(text_page) = positioned_text else {
            continue;
        };

        let mut occurrences = job.matcher.locate_matches(text_page.as_ref(), &query);
        let mut highlight_unavailable = false;
        for occurrence in &mut occurrences {
            if occurrence_highlight_unavailable(occurrence, &text_page.glyphs) {
                highlight_unavailable = true;
            }
            apply_hit_snippet(occurrence, &text_page.glyphs);
        }
        let _ = event_tx.send(SearchEvent::GeometryResolved {
            generation: job.generation,
            page,
            occurrences,
            highlight_unavailable,
        });
    }

    WorkerControl::Continue
}

fn flush_requests(
    request_rx: &mut UnboundedReceiver<WorkerRequest>,
    pending: &mut PendingWorkerWork,
) -> WorkerControl {
    loop {
        match request_rx.try_recv() {
            Ok(WorkerRequest::Query(job)) => pending.query = Some(job),
            Ok(WorkerRequest::Prewarm(job)) => pending.prewarm = Some(job),
            Ok(WorkerRequest::ResolveGeometry(job)) => {
                if job.priority == GeometryPriority::High
                    || pending
                        .geometry
                        .as_ref()
                        .is_none_or(|queued| queued.priority == GeometryPriority::Background)
                {
                    pending.geometry = Some(job);
                }
            }
            Ok(WorkerRequest::Shutdown) => return WorkerControl::Shutdown,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return WorkerControl::Shutdown,
        }
    }

    WorkerControl::Continue
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use tokio::sync::mpsc::unbounded_channel;

    use super::{
        PendingWorkerWork, PrewarmControl, PrewarmJob, SearchGeometryCache, SearchJob,
        SearchPageCache, SearchTextCache, WorkerRequest, estimate_search_text_bytes,
        estimate_text_page_bytes, run_prewarm_job,
    };
    use crate::backend::{OutlineNode, PdfBackend, PdfRect, RgbaFrame, TextGlyph, TextPage};
    use crate::command::SearchMatcherKind;
    use crate::error::{AppError, AppResult};
    use crate::search::matcher::matcher_for_kind;

    struct CountingPositionedStubPdf {
        path: PathBuf,
        doc_id: u64,
        pages: Vec<TextPage>,
        positioned_calls: Mutex<Vec<usize>>,
    }

    impl CountingPositionedStubPdf {
        fn new(doc_id: u64, pages: Vec<TextPage>) -> Self {
            let page_count = pages.len();
            Self {
                path: PathBuf::from(format!("counting-positioned-{doc_id}.pdf")),
                doc_id,
                pages,
                positioned_calls: Mutex::new(vec![0; page_count]),
            }
        }

        fn positioned_calls(&self) -> Vec<usize> {
            self.positioned_calls
                .lock()
                .expect("positioned calls lock should not be poisoned")
                .clone()
        }
    }

    impl PdfBackend for CountingPositionedStubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            self.doc_id
        }

        fn page_count(&self) -> usize {
            self.pages.len()
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            Ok((100.0, 100.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::unsupported("not needed in search test"))
        }

        fn extract_text(&self, page: usize) -> AppResult<String> {
            Ok(self.pages[page].extracted_text())
        }

        fn extract_positioned_text(&self, page: usize) -> AppResult<TextPage> {
            let mut calls = self
                .positioned_calls
                .lock()
                .expect("positioned calls lock should not be poisoned");
            calls[page] += 1;
            Ok(self.pages[page].clone())
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Err(AppError::unsupported("not needed in search test"))
        }
    }

    #[test]
    fn search_page_cache_hits_after_insert() {
        let mut cache = SearchPageCache::with_limits(4, usize::MAX);
        let page = text_page("alpha");

        cache.insert(1, 0, Arc::new(page.clone()));

        assert_eq!(cache.get(1, 0).as_deref(), Some(&page));
    }

    #[test]
    fn search_page_cache_evicts_least_recently_used_over_max_entries() {
        let mut cache = SearchPageCache::with_limits(2, usize::MAX);
        cache.insert(1, 0, Arc::new(text_page("alpha")));
        cache.insert(1, 1, Arc::new(text_page("beta")));
        assert!(cache.get(1, 0).is_some());

        cache.insert(1, 2, Arc::new(text_page("gamma")));

        assert!(cache.get(1, 0).is_some());
        assert!(cache.get(1, 1).is_none());
        assert!(cache.get(1, 2).is_some());
    }

    #[test]
    fn search_page_cache_evicts_over_memory_budget() {
        let small = text_page("a");
        let large = text_page("abcdef");
        let budget = estimate_text_page_bytes(&small) + estimate_text_page_bytes(&large) - 1;
        let mut cache = SearchPageCache::with_limits(4, budget);
        cache.insert(1, 0, Arc::new(small));

        cache.insert(1, 1, Arc::new(large));

        assert_eq!(cache.len(), 1);
        assert!(cache.get(1, 0).is_none());
        assert!(cache.get(1, 1).is_some());
        assert!(cache.memory_bytes() <= budget);
    }

    #[test]
    fn search_page_cache_insert_without_eviction_stops_at_memory_budget() {
        let first = Arc::new(text_page("alpha"));
        let second = Arc::new(text_page("beta gamma"));
        let budget = estimate_text_page_bytes(&first);
        let mut cache = SearchPageCache::with_limits(4, budget);

        assert!(cache.try_insert_without_eviction(1, 0, Arc::clone(&first)));
        assert!(!cache.try_insert_without_eviction(1, 1, Arc::clone(&second)));

        assert!(cache.get(1, 0).is_some());
        assert!(cache.get(1, 1).is_none());
        assert_eq!(cache.len(), 1);
        assert!(cache.memory_bytes() <= budget);
    }

    #[test]
    fn search_page_cache_separates_documents() {
        let mut cache = SearchPageCache::with_limits(4, usize::MAX);
        cache.insert(1, 0, Arc::new(text_page("alpha")));
        cache.insert(2, 0, Arc::new(text_page("beta")));

        assert_eq!(
            cache
                .get(1, 0)
                .expect("doc 1 page should exist")
                .extracted_text(),
            "alpha"
        );
        assert_eq!(
            cache
                .get(2, 0)
                .expect("doc 2 page should exist")
                .extracted_text(),
            "beta"
        );
    }

    #[test]
    fn search_page_cache_reinsertion_replaces_memory_accounting() {
        let mut cache = SearchPageCache::with_limits(4, usize::MAX);
        let replacement = text_page("abcde");
        let expected_bytes = estimate_text_page_bytes(&replacement);

        cache.insert(1, 0, Arc::new(text_page("a")));
        cache.insert(1, 0, Arc::new(replacement));

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.memory_bytes(), expected_bytes);
    }

    #[test]
    fn search_page_cache_reinsertion_does_not_trigger_budget_eviction() {
        let page = Arc::new(text_page("a"));
        let expected_bytes = estimate_text_page_bytes(&page);
        let mut cache = SearchPageCache::with_limits(4, expected_bytes);

        cache.insert(1, 0, Arc::clone(&page));
        cache.insert(1, 0, Arc::clone(&page));

        assert_eq!(cache.len(), 1);
        assert!(cache.get(1, 0).is_some());
        assert_eq!(cache.memory_bytes(), expected_bytes);
    }

    #[test]
    fn prewarm_does_not_retry_after_memory_budget_is_reached() {
        let first_page = text_page("alpha");
        let second_page = text_page("beta gamma");
        let budget = estimate_search_text_bytes(&Arc::from(first_page.extracted_text()));
        let pdf = Arc::new(CountingPositionedStubPdf::new(
            302,
            vec![first_page, second_page],
        ));
        let (_request_tx, request_rx) = unbounded_channel();
        let mut request_rx = request_rx;
        let mut pending = PendingWorkerWork::default();
        let mut text_cache = SearchTextCache::with_limits(4, budget);
        let mut geometry_cache = SearchGeometryCache::with_limits(4, usize::MAX);
        let mut prewarm_finished_doc_ids = HashSet::new();

        assert!(matches!(
            run_prewarm_job(
                PrewarmJob { pdf: pdf.clone() },
                &mut request_rx,
                &mut pending,
                &mut text_cache,
                &mut geometry_cache,
                &mut prewarm_finished_doc_ids,
            ),
            PrewarmControl::Finished
        ));
        assert_eq!(pdf.positioned_calls(), vec![1, 1]);

        assert!(matches!(
            run_prewarm_job(
                PrewarmJob { pdf: pdf.clone() },
                &mut request_rx,
                &mut pending,
                &mut text_cache,
                &mut geometry_cache,
                &mut prewarm_finished_doc_ids,
            ),
            PrewarmControl::Finished
        ));
        assert_eq!(pdf.positioned_calls(), vec![1, 1]);
    }

    #[test]
    fn interrupted_prewarm_can_resume_after_priority_work() {
        let pdf = Arc::new(CountingPositionedStubPdf::new(
            303,
            vec![text_page("alpha"), text_page("beta")],
        ));
        let (request_tx, request_rx) = unbounded_channel();
        let mut request_rx = request_rx;
        let mut pending = PendingWorkerWork::default();
        let mut text_cache = SearchTextCache::with_limits(4, usize::MAX);
        let mut geometry_cache = SearchGeometryCache::with_limits(4, usize::MAX);
        let mut prewarm_finished_doc_ids = HashSet::new();

        request_tx
            .send(WorkerRequest::Query(SearchJob {
                generation: 1,
                pdf: pdf.clone(),
                query: "alpha".to_string(),
                matcher: matcher_for_kind(SearchMatcherKind::ContainsInsensitive),
            }))
            .expect("query request should be queued");

        let interrupted = match run_prewarm_job(
            PrewarmJob { pdf: pdf.clone() },
            &mut request_rx,
            &mut pending,
            &mut text_cache,
            &mut geometry_cache,
            &mut prewarm_finished_doc_ids,
        ) {
            PrewarmControl::Interrupted(job) => job,
            PrewarmControl::Finished => panic!("prewarm should be interrupted"),
            PrewarmControl::Shutdown => panic!("worker should not shut down"),
        };

        assert!(pending.query.is_some());
        assert_eq!(pdf.positioned_calls(), vec![0, 0]);

        pending.query = None;
        assert!(matches!(
            run_prewarm_job(
                interrupted,
                &mut request_rx,
                &mut pending,
                &mut text_cache,
                &mut geometry_cache,
                &mut prewarm_finished_doc_ids,
            ),
            PrewarmControl::Finished
        ));
        assert_eq!(pdf.positioned_calls(), vec![1, 1]);
    }

    fn text_page(text: &str) -> TextPage {
        TextPage {
            width_pt: 100.0,
            height_pt: 100.0,
            glyphs: text
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    let x0 = index as f32 * 10.0;
                    glyph(ch, x0, 0.0, x0 + 10.0, 10.0)
                })
                .collect(),
            dropped_glyphs: 0,
        }
    }

    fn glyph(ch: char, x0: f32, y0: f32, x1: f32, y1: f32) -> TextGlyph {
        TextGlyph {
            ch,
            bbox: Some(PdfRect { x0, y0, x1, y1 }),
        }
    }
}
