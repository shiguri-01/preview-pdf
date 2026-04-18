use std::sync::Arc;

use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};
use tokio::task::JoinHandle;

use crate::backend::{PdfRect, SharedPdfBackend, TextGlyph, TextPage};
use crate::error::{AppError, AppResult};

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
    Failed {
        generation: u64,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchOccurrence {
    pub rects: Vec<PdfRect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPageHit {
    pub page: usize,
    pub occurrences: Vec<SearchOccurrence>,
}

pub trait SearchMatcher: Send + Sync {
    fn prepare_query(&self, raw_query: &str) -> String;
    fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool;
    fn locate_matches(&self, page: &TextPage, prepared_query: &str) -> Vec<SearchOccurrence>;
}

#[derive(Clone)]
struct SearchJob {
    generation: u64,
    pdf: SharedPdfBackend,
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

fn worker_main(
    mut request_rx: UnboundedReceiver<WorkerRequest>,
    event_tx: UnboundedSender<SearchEvent>,
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

        match run_job(job, &mut request_rx, &event_tx, &mut pending) {
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

    let total_pages = doc.page_count();

    let mut hits = Vec::new();
    let mut highlight_unavailable = false;
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
            let occurrences = match doc.extract_positioned_text(page) {
                Ok(text_page) => {
                    if text_page.dropped_glyphs > 0 {
                        highlight_unavailable = true;
                        Vec::new()
                    } else {
                        let occurrences = job.matcher.locate_matches(&text_page, &query);
                        if occurrences.is_empty() {
                            highlight_unavailable = true;
                        }
                        occurrences
                    }
                }
                Err(_) => {
                    highlight_unavailable = true;
                    Vec::new()
                }
            };
            hits.push(SearchPageHit { page, occurrences });
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

pub(crate) fn locate_occurrences(
    glyphs: &[TextGlyph],
    prepared_query: &str,
    case_sensitive: bool,
) -> Vec<SearchOccurrence> {
    let direct = locate_occurrences_with_strategy(glyphs, prepared_query, case_sensitive, false);
    if !direct.is_empty() {
        return direct;
    }
    locate_occurrences_with_strategy(glyphs, prepared_query, case_sensitive, true)
}

fn locate_occurrences_with_strategy(
    glyphs: &[TextGlyph],
    prepared_query: &str,
    case_sensitive: bool,
    ignore_whitespace: bool,
) -> Vec<SearchOccurrence> {
    if prepared_query.is_empty() {
        return Vec::new();
    }

    let mut search_text = String::new();
    let mut char_map = Vec::new();
    for (glyph_index, glyph) in glyphs.iter().enumerate() {
        if ignore_whitespace && glyph.ch.is_whitespace() {
            continue;
        }
        let normalized = normalize_char(glyph.ch, case_sensitive);
        if !ignore_whitespace || !normalized.is_whitespace() {
            search_text.push(normalized);
            char_map.push(glyph_index);
        }
    }

    if search_text.is_empty() {
        return Vec::new();
    }

    let query_text = if ignore_whitespace {
        prepared_query
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
    } else {
        prepared_query.to_string()
    };
    if query_text.is_empty() {
        return Vec::new();
    }

    let search_chars: Vec<char> = search_text.chars().collect();
    let query_chars: Vec<char> = query_text.chars().collect();
    let query_len = query_chars.len();
    if query_len == 0 || query_len > search_chars.len() {
        return Vec::new();
    }

    let mut occurrences = Vec::new();
    let mut cursor = 0;
    while cursor + query_len <= search_chars.len() {
        if search_chars[cursor..cursor + query_len] == query_chars[..] {
            let glyph_start = char_map[cursor];
            let glyph_end = char_map[cursor + query_len - 1];
            let rects = merge_occurrence_rects(&glyphs[glyph_start..=glyph_end]);
            if !rects.is_empty() {
                occurrences.push(SearchOccurrence { rects });
            }
            cursor += query_len;
        } else {
            cursor += 1;
        }
    }

    occurrences
}

fn merge_occurrence_rects(glyphs: &[TextGlyph]) -> Vec<PdfRect> {
    let glyphs: Vec<&TextGlyph> = glyphs
        .iter()
        .filter(|glyph| !glyph.ch.is_whitespace())
        .collect();
    if glyphs.is_empty() {
        return Vec::new();
    }

    let merge_axis = infer_merge_axis(&glyphs);
    let median_width = median_rect_extent(&glyphs, RectExtent::Width);
    let median_height = median_rect_extent(&glyphs, RectExtent::Height);
    let mut rects = Vec::new();
    let mut current = glyphs[0].bbox;

    for glyph in glyphs.iter().skip(1) {
        if belongs_to_run(current, glyph.bbox, merge_axis, median_width, median_height) {
            current = union_rects(current, glyph.bbox);
        } else {
            rects.push(current);
            current = glyph.bbox;
        }
    }

    rects.push(current);
    rects
}

// Choose the screen-space axis used to merge highlight rectangles.
// This is not a writing-mode detector: rotated horizontal text may still merge
// on the vertical axis if that better matches the glyph layout on the page.
fn infer_merge_axis(glyphs: &[&TextGlyph]) -> MergeAxis {
    let mut horizontal_score = 0.0f32;
    let mut vertical_score = 0.0f32;

    for pair in glyphs.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        horizontal_score +=
            overlap_ratio_1d(left.bbox.y0, left.bbox.y1, right.bbox.y0, right.bbox.y1);
        vertical_score +=
            overlap_ratio_1d(left.bbox.x0, left.bbox.x1, right.bbox.x0, right.bbox.x1);
    }

    if vertical_score > horizontal_score {
        MergeAxis::Vertical
    } else {
        MergeAxis::Horizontal
    }
}

fn belongs_to_run(
    current: PdfRect,
    next: PdfRect,
    merge_axis: MergeAxis,
    median_width: f32,
    median_height: f32,
) -> bool {
    match merge_axis {
        MergeAxis::Horizontal => {
            let same_band = overlap_ratio_1d(current.y0, current.y1, next.y0, next.y1) >= 0.45
                || center_distance(current.y0, current.y1, next.y0, next.y1)
                    <= median_height * 0.35;
            let gap_ok =
                interval_gap(current.x0, current.x1, next.x0, next.x1) <= median_width * 4.0;
            same_band && gap_ok
        }
        MergeAxis::Vertical => {
            let same_band = overlap_ratio_1d(current.x0, current.x1, next.x0, next.x1) >= 0.45
                || center_distance(current.x0, current.x1, next.x0, next.x1) <= median_width * 0.35;
            let gap_ok =
                interval_gap(current.y0, current.y1, next.y0, next.y1) <= median_height * 4.0;
            same_band && gap_ok
        }
    }
}

fn overlap_ratio_1d(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    let overlap = (a1.min(b1) - a0.max(b0)).max(0.0);
    let min_extent = (a1 - a0).abs().min((b1 - b0).abs()).max(1e-3);
    overlap / min_extent
}

fn center_distance(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (((a0 + a1) * 0.5) - ((b0 + b1) * 0.5)).abs()
}

fn interval_gap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    if b0 > a1 {
        b0 - a1
    } else if a0 > b1 {
        a0 - b1
    } else {
        0.0
    }
}

fn union_rects(left: PdfRect, right: PdfRect) -> PdfRect {
    PdfRect {
        x0: left.x0.min(right.x0),
        y0: left.y0.min(right.y0),
        x1: left.x1.max(right.x1),
        y1: left.y1.max(right.y1),
    }
}

fn median_rect_extent(glyphs: &[&TextGlyph], extent: RectExtent) -> f32 {
    let mut values: Vec<f32> = glyphs
        .iter()
        .map(|glyph| match extent {
            RectExtent::Width => glyph.bbox.width(),
            RectExtent::Height => glyph.bbox.height(),
        })
        .filter(|value| *value > 0.0)
        .collect();
    if values.is_empty() {
        return 1.0;
    }

    values.sort_by(|left, right| left.total_cmp(right));
    values[values.len() / 2]
}

fn normalize_char(ch: char, case_sensitive: bool) -> char {
    if case_sensitive {
        ch
    } else {
        ch.to_lowercase().next().unwrap_or(ch)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RectExtent {
    Width,
    Height,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::{
        SearchEngine, SearchEvent, SearchMatcher, SearchOccurrence, SearchPageHit,
        locate_occurrences, merge_occurrence_rects,
    };
    use crate::backend::open_default_backend;
    use crate::backend::{OutlineNode, PdfBackend, PdfRect, RgbaFrame, TextGlyph, TextPage};
    use crate::error::{AppError, AppResult};

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

            prepared_page
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
                .contains(
                    &prepared_query
                        .chars()
                        .filter(|ch| !ch.is_whitespace())
                        .collect::<String>(),
                )
        }

        fn locate_matches(&self, page: &TextPage, prepared_query: &str) -> Vec<SearchOccurrence> {
            locate_occurrences(&page.glyphs, prepared_query, self.case_sensitive)
        }
    }

    struct SearchOnlyStubPdf {
        path: PathBuf,
        text: String,
    }

    impl SearchOnlyStubPdf {
        fn new(text: &str) -> Self {
            Self {
                path: PathBuf::from("search-only.pdf"),
                text: text.to_string(),
            }
        }
    }

    impl PdfBackend for SearchOnlyStubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            42
        }

        fn page_count(&self) -> usize {
            1
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            Ok((100.0, 100.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::unsupported("not needed in search test"))
        }

        fn extract_text(&self, _page: usize) -> AppResult<String> {
            Ok(self.text.clone())
        }

        fn extract_positioned_text(&self, _page: usize) -> AppResult<TextPage> {
            Err(AppError::unsupported("positioned text unavailable"))
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Err(AppError::unsupported("not needed in search test"))
        }
    }

    struct SearchPositionedStubPdf {
        path: PathBuf,
        text: String,
        positioned_text: TextPage,
    }

    impl SearchPositionedStubPdf {
        fn new(text: &str, positioned_text: TextPage) -> Self {
            Self {
                path: PathBuf::from("search-positioned.pdf"),
                text: text.to_string(),
                positioned_text,
            }
        }
    }

    impl PdfBackend for SearchPositionedStubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            43
        }

        fn page_count(&self) -> usize {
            1
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            Ok((100.0, 100.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::unsupported("not needed in search test"))
        }

        fn extract_text(&self, _page: usize) -> AppResult<String> {
            Ok(self.text.clone())
        }

        fn extract_positioned_text(&self, _page: usize) -> AppResult<TextPage> {
            Ok(self.positioned_text.clone())
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Err(AppError::unsupported("not needed in search test"))
        }
    }

    #[test]
    fn submit_returns_incrementing_generation() {
        let file = unique_temp_path("generation.pdf");
        fs::write(&file, build_pdf(&["one"])).expect("test file should be created");

        let mut engine = SearchEngine::new();
        let pdf = open_default_backend(&file).expect("pdf should open");
        let gen1 = engine
            .submit(
                Arc::clone(&pdf),
                "one",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("first submit should succeed");
        let gen2 = engine
            .submit(
                pdf,
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
        let pdf = open_default_backend(&file).expect("pdf should open");
        let running_generation = engine
            .submit(
                Arc::clone(&pdf),
                "one",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");
        let cancel_generation = engine.cancel(pdf).expect("cancel should succeed");

        assert_eq!(cancel_generation, running_generation + 1);
        let (hits, _) = wait_for_completed_hits(&mut engine, cancel_generation);
        assert!(hits.is_empty());

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn search_finds_hits_case_insensitively() {
        let file = unique_temp_path("hits.pdf");
        fs::write(&file, build_pdf(&["Alpha", "BETA alpha", "gamma"]))
            .expect("test file should be created");

        let mut engine = SearchEngine::new();
        let pdf = open_default_backend(&file).expect("pdf should open");
        let generation = engine
            .submit(
                pdf,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let (hits, _) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hit_pages(&hits), vec![0, 1]);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn search_can_use_case_sensitive_matcher() {
        let file = unique_temp_path("hits_sensitive.pdf");
        fs::write(&file, build_pdf(&["Alpha", "alpha", "ALPHA"]))
            .expect("test file should be created");

        let mut engine = SearchEngine::new();
        let pdf = open_default_backend(&file).expect("pdf should open");
        let generation = engine
            .submit(
                pdf,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: true,
                }),
            )
            .expect("submit should succeed");

        let (hits, _) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hit_pages(&hits), vec![1]);

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
        let pdf = open_default_backend(&file).expect("pdf should open");
        let generation = engine
            .submit(
                pdf,
                "hello world",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let (hits, _) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hit_pages(&hits), vec![0]);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn search_keeps_hit_pages_when_positioned_text_is_unavailable() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchOnlyStubPdf::new("alpha beta"));
        let generation = engine
            .submit(
                pdf,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let (hits, highlight_unavailable) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page, 0);
        assert!(hits[0].occurrences.is_empty());
        assert!(highlight_unavailable);
    }

    #[test]
    fn search_marks_highlight_unavailable_when_match_has_no_geometry() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "alpha beta",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: Vec::new(),
                dropped_glyphs: 0,
            },
        ));
        let generation = engine
            .submit(
                pdf,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let (hits, highlight_unavailable) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].occurrences.is_empty());
        assert!(highlight_unavailable);
    }

    #[test]
    fn search_treats_dropped_positioned_glyphs_as_highlight_failure() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "alpha beta",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: vec![glyph('a', 0.0, 0.0, 10.0, 10.0)],
                dropped_glyphs: 1,
            },
        ));
        let generation = engine
            .submit(
                pdf,
                "alpha",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let (hits, highlight_unavailable) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].occurrences.is_empty());
        assert!(highlight_unavailable);
    }

    #[test]
    fn merge_occurrence_rects_merges_same_line_with_spaces() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('b', 20.0, 20.0, 28.0, 32.0),
            glyph(' ', 29.0, 20.0, 33.0, 32.0),
            glyph('c', 34.0, 20.0, 42.0, 32.0),
            glyph('d', 44.0, 20.0, 52.0, 32.0),
        ];

        let rects = merge_occurrence_rects(&glyphs);

        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 10.0,
                y0: 20.0,
                x1: 52.0,
                y1: 32.0
            }
        );
    }

    #[test]
    fn merge_occurrence_rects_splits_wrapped_horizontal_lines() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('b', 20.0, 20.0, 28.0, 32.0),
            glyph('c', 30.0, 20.0, 38.0, 32.0),
            glyph('d', 10.0, 36.0, 18.0, 48.0),
            glyph('e', 20.0, 36.0, 28.0, 48.0),
            glyph('f', 30.0, 36.0, 38.0, 48.0),
        ];

        let rects = merge_occurrence_rects(&glyphs);

        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 10.0,
                y0: 20.0,
                x1: 38.0,
                y1: 32.0
            }
        );
        assert_eq!(
            rects[1],
            PdfRect {
                x0: 10.0,
                y0: 36.0,
                x1: 38.0,
                y1: 48.0
            }
        );
    }

    #[test]
    fn merge_occurrence_rects_merges_vertical_column() {
        let glyphs = vec![
            glyph('縦', 80.0, 10.0, 92.0, 22.0),
            glyph('書', 80.0, 24.0, 92.0, 36.0),
            glyph('き', 80.0, 38.0, 92.0, 50.0),
        ];

        let rects = merge_occurrence_rects(&glyphs);

        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 80.0,
                y0: 10.0,
                x1: 92.0,
                y1: 50.0
            }
        );
    }

    #[test]
    fn merge_occurrence_rects_splits_wrapped_vertical_columns() {
        let glyphs = vec![
            glyph('縦', 80.0, 10.0, 92.0, 22.0),
            glyph('書', 80.0, 24.0, 92.0, 36.0),
            glyph('き', 80.0, 38.0, 92.0, 50.0),
            glyph('折', 62.0, 10.0, 74.0, 22.0),
            glyph('返', 62.0, 24.0, 74.0, 36.0),
            glyph('し', 62.0, 38.0, 74.0, 50.0),
        ];

        let rects = merge_occurrence_rects(&glyphs);

        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            PdfRect {
                x0: 80.0,
                y0: 10.0,
                x1: 92.0,
                y1: 50.0
            }
        );
        assert_eq!(
            rects[1],
            PdfRect {
                x0: 62.0,
                y0: 10.0,
                x1: 74.0,
                y1: 50.0
            }
        );
    }

    fn wait_for_completed_hits(
        engine: &mut SearchEngine,
        generation: u64,
    ) -> (Vec<SearchPageHit>, bool) {
        let timeout = Duration::from_secs(3);
        let start = Instant::now();

        loop {
            for event in engine.drain_events() {
                if let SearchEvent::Completed {
                    generation: event_generation,
                    hits,
                    highlight_unavailable,
                } = event
                    && event_generation == generation
                {
                    return (hits, highlight_unavailable);
                }
            }

            assert!(
                start.elapsed() <= timeout,
                "timed out waiting for search completion"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn hit_pages(hits: &[SearchPageHit]) -> Vec<usize> {
        hits.iter().map(|hit| hit.page).collect()
    }

    fn glyph(ch: char, x0: f32, y0: f32, x1: f32, y1: f32) -> TextGlyph {
        TextGlyph {
            ch,
            bbox: PdfRect { x0, y0, x1, y1 },
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
