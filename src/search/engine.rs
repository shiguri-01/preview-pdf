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
    pub glyph_start: usize,
    pub glyph_end: usize,
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

pub trait SearchMatcher: Send + Sync {
    fn prepare_query(&self, raw_query: &str) -> String;
    fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool;
    fn locate_matches(&self, page: &TextPage, prepared_query: &str) -> Vec<SearchOccurrence>;
}

pub(crate) fn prepare_contains_query(raw_query: &str, case_sensitive: bool) -> String {
    normalize_text_for_search(raw_query, case_sensitive, false)
}

pub(crate) fn page_matches_contains(
    page_text: &str,
    prepared_query: &str,
    case_sensitive: bool,
) -> bool {
    let prepared_page = normalize_text_for_search(page_text, case_sensitive, false);
    if prepared_page.contains(prepared_query) {
        return true;
    }

    let whitespace_insensitive_page = normalize_text_for_search(page_text, case_sensitive, true);
    let whitespace_insensitive_query = normalize_text_for_search(prepared_query, true, true);
    whitespace_insensitive_page.contains(&whitespace_insensitive_query)
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

        match doc.extract_positioned_text(page) {
            Ok(text_page) => {
                let mut occurrences = job.matcher.locate_matches(&text_page, &query);
                // Keep `extract_text` out of the primary search path: once positioned text is
                // available, the glyph stream is the searchable source of truth.
                if !occurrences.is_empty() {
                    for occurrence in &mut occurrences {
                        if occurrence_highlight_unavailable(occurrence, &text_page.glyphs) {
                            highlight_unavailable = true;
                        }
                        apply_hit_snippet(occurrence, &text_page.glyphs);
                    }
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

fn occurrence_highlight_unavailable(occurrence: &SearchOccurrence, glyphs: &[TextGlyph]) -> bool {
    if occurrence.rects.is_empty() {
        return true;
    }

    let Some(slice) = glyphs.get(occurrence.glyph_start..=occurrence.glyph_end) else {
        return true;
    };

    slice
        .iter()
        .any(|glyph| !glyph.ch.is_whitespace() && glyph.bbox.is_none())
}

pub(crate) fn locate_occurrences(
    glyphs: &[TextGlyph],
    prepared_query: &str,
    case_sensitive: bool,
) -> Vec<SearchOccurrence> {
    let mut occurrences =
        locate_occurrences_with_strategy(glyphs, prepared_query, case_sensitive, false);

    // `SearchOccurrence` dedups by `(glyph_start, glyph_end)` because
    // locate_occurrences_with_strategy/merge_occurrence_rects are pure for a glyph slice; keep it
    // that way and avoid adding path-dependent fields that would make equal ranges diverge.
    for occurrence in locate_occurrences_with_strategy(glyphs, prepared_query, case_sensitive, true)
    {
        let duplicate = occurrences.iter().any(|existing| {
            existing.glyph_start == occurrence.glyph_start
                && existing.glyph_end == occurrence.glyph_end
        });
        if !duplicate {
            occurrences.push(occurrence);
        }
    }

    occurrences
}

struct SnippetPresentation {
    text: String,
    match_start: Option<usize>,
    match_end: Option<usize>,
}

fn apply_hit_snippet(occurrence: &mut SearchOccurrence, glyphs: &[TextGlyph]) {
    let snippet = build_hit_snippet(glyphs, occurrence.glyph_start, occurrence.glyph_end);
    occurrence.snippet = snippet.text;
    occurrence.snippet_match_start = snippet.match_start;
    occurrence.snippet_match_end = snippet.match_end;
}

fn build_hit_snippet(
    glyphs: &[TextGlyph],
    glyph_start: usize,
    glyph_end: usize,
) -> SnippetPresentation {
    const CONTEXT_CHARS: usize = 16;

    if glyphs.is_empty() || glyph_start >= glyphs.len() || glyph_end < glyph_start {
        return SnippetPresentation {
            text: String::new(),
            match_start: None,
            match_end: None,
        };
    }

    let glyph_end = glyph_end.min(glyphs.len() - 1);
    let context_start = glyph_start.saturating_sub(CONTEXT_CHARS);
    let context_end = glyph_end
        .saturating_add(CONTEXT_CHARS)
        .saturating_add(1)
        .min(glyphs.len());

    let before = glyphs[context_start..glyph_start]
        .iter()
        .map(|glyph| glyph.ch)
        .collect::<String>();
    let matched = glyphs[glyph_start..=glyph_end]
        .iter()
        .map(|glyph| glyph.ch)
        .collect::<String>();
    let after = glyphs[glyph_end + 1..context_end]
        .iter()
        .map(|glyph| glyph.ch)
        .collect::<String>();

    let mut snippet = String::new();
    let mut match_start = None;
    let mut match_end = None;
    if context_start > 0 {
        snippet.push('…');
    }
    snippet.push_str(&before);
    if !matched.is_empty() {
        match_start = Some(snippet.len());
    }
    snippet.push_str(&matched);
    if !matched.is_empty() {
        match_end = Some(snippet.len());
    }
    snippet.push_str(&after);
    if context_end < glyphs.len() {
        snippet.push('…');
    }

    SnippetPresentation {
        text: snippet,
        match_start,
        match_end,
    }
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

    let (search_text, char_map) =
        normalize_glyphs_for_search(glyphs, case_sensitive, ignore_whitespace);
    if search_text.is_empty() {
        return Vec::new();
    }

    // `prepared_query` already has case handling applied by prepare_query/prepare_contains_query.
    let query_text = normalize_text_for_search(prepared_query, true, ignore_whitespace);
    if query_text.is_empty() {
        return Vec::new();
    }

    if query_text.len() > search_text.len() {
        return Vec::new();
    }

    let char_byte_offsets: Vec<usize> = search_text
        .char_indices()
        .map(|(offset, _)| offset)
        .collect();
    let query_char_len = query_text.chars().count();
    if query_char_len == 0 || query_char_len > char_map.len() {
        return Vec::new();
    }

    let mut occurrences = Vec::new();
    let mut cursor_byte = 0;
    // Matches are intentionally non-overlapping "find in page" hits: after matching
    // `query_text`, advance by its byte length so overlapping occurrences are skipped by design.
    while cursor_byte <= search_text.len() {
        let Some(relative_match_byte) = search_text[cursor_byte..].find(&query_text) else {
            break;
        };
        let match_byte = cursor_byte + relative_match_byte;
        let match_char_start = char_byte_offsets.binary_search(&match_byte);
        debug_assert!(
            match_char_start.is_ok(),
            "str::find returned a non-character-boundary offset"
        );
        let match_char_start =
            match_char_start.expect("str::find returned a non-character-boundary offset");
        let glyph_start = char_map[match_char_start];
        let glyph_end = char_map[match_char_start + query_char_len - 1];
        let rects = merge_occurrence_rects(&glyphs[glyph_start..=glyph_end]);
        occurrences.push(SearchOccurrence {
            glyph_start,
            glyph_end,
            rects,
            snippet: String::new(),
            snippet_match_start: None,
            snippet_match_end: None,
        });
        cursor_byte = match_byte + query_text.len();
    }

    occurrences
}

fn normalize_glyphs_for_search(
    glyphs: &[TextGlyph],
    case_sensitive: bool,
    ignore_whitespace: bool,
) -> (String, Vec<usize>) {
    let mut search_text = String::new();
    let mut char_map = Vec::new();

    for (glyph_index, glyph) in glyphs.iter().enumerate() {
        if ignore_whitespace && glyph.ch.is_whitespace() {
            continue;
        }
        push_normalized_chars(glyph.ch, case_sensitive, |normalized| {
            if !ignore_whitespace || !normalized.is_whitespace() {
                search_text.push(normalized);
                char_map.push(glyph_index);
            }
        });
    }

    (search_text, char_map)
}

fn merge_occurrence_rects(glyphs: &[TextGlyph]) -> Vec<PdfRect> {
    let glyphs: Vec<HighlightGlyph> = glyphs
        .iter()
        .filter_map(|glyph| {
            if glyph.ch.is_whitespace() {
                None
            } else {
                glyph.bbox.map(|bbox| HighlightGlyph { bbox })
            }
        })
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
        let bbox = glyph.bbox;
        if belongs_to_run(current, bbox, merge_axis, median_width, median_height) {
            current = union_rects(current, bbox);
        } else {
            rects.push(current);
            current = bbox;
        }
    }

    rects.push(current);
    rects
}

#[derive(Debug, Clone, Copy)]
struct HighlightGlyph {
    bbox: PdfRect,
}

// Choose the screen-space axis used to merge highlight rectangles.
// This is not a writing-mode detector: rotated horizontal text may still merge
// on the vertical axis if that better matches the glyph layout on the page.
fn infer_merge_axis(glyphs: &[HighlightGlyph]) -> MergeAxis {
    let mut horizontal_score = 0.0f32;
    let mut vertical_score = 0.0f32;

    for pair in glyphs.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        let left_bbox = left.bbox;
        let right_bbox = right.bbox;
        horizontal_score +=
            overlap_ratio_1d(left_bbox.y0, left_bbox.y1, right_bbox.y0, right_bbox.y1);
        vertical_score +=
            overlap_ratio_1d(left_bbox.x0, left_bbox.x1, right_bbox.x0, right_bbox.x1);
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

fn median_rect_extent(glyphs: &[HighlightGlyph], extent: RectExtent) -> f32 {
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

fn push_normalized_chars(ch: char, case_sensitive: bool, mut push: impl FnMut(char)) {
    if case_sensitive {
        push(ch);
    } else {
        for normalized in ch.to_lowercase() {
            push(normalized);
        }
    }
}

fn normalize_text_for_search(text: &str, case_sensitive: bool, ignore_whitespace: bool) -> String {
    let mut normalized_text = String::with_capacity(text.len());
    for ch in text.chars() {
        if ignore_whitespace && ch.is_whitespace() {
            continue;
        }
        push_normalized_chars(ch, case_sensitive, |normalized| {
            if !ignore_whitespace || !normalized.is_whitespace() {
                normalized_text.push(normalized);
            }
        });
    }
    normalized_text
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
        locate_occurrences, merge_occurrence_rects, page_matches_contains, prepare_contains_query,
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
            prepare_contains_query(raw_query, self.case_sensitive)
        }

        fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool {
            page_matches_contains(page_text, prepared_query, self.case_sensitive)
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
    fn search_uses_positioned_text_even_when_extract_text_does_not_match() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: vec![
                    glyph('a', 0.0, 0.0, 10.0, 10.0),
                    glyph('l', 10.0, 0.0, 20.0, 10.0),
                    glyph('p', 20.0, 0.0, 30.0, 10.0),
                    glyph('h', 30.0, 0.0, 40.0, 10.0),
                    glyph('a', 40.0, 0.0, 50.0, 10.0),
                ],
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
        assert_eq!(hits[0].occurrences.len(), 1);
        assert!(!highlight_unavailable);
    }

    #[test]
    fn search_keeps_occurrence_when_match_has_no_geometry() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "alpha beta",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: vec![
                    glyph_without_bbox('a'),
                    glyph_without_bbox('l'),
                    glyph_without_bbox('p'),
                    glyph_without_bbox('h'),
                    glyph_without_bbox('a'),
                ],
                dropped_glyphs: 5,
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
        assert_eq!(hits[0].occurrences.len(), 1);
        assert!(hits[0].occurrences[0].rects.is_empty());
        assert_eq!(hits[0].occurrences[0].snippet, "alpha");
        assert!(highlight_unavailable);
    }

    #[test]
    fn search_marks_highlight_unavailable_when_matched_glyph_lacks_geometry() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "alpha beta",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: vec![
                    glyph('a', 0.0, 0.0, 10.0, 10.0),
                    glyph('l', 10.0, 0.0, 20.0, 10.0),
                    glyph_without_bbox('p'),
                    glyph('h', 30.0, 0.0, 40.0, 10.0),
                    glyph('a', 40.0, 0.0, 50.0, 10.0),
                ],
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
        assert_eq!(hits[0].occurrences.len(), 1);
        assert_eq!(hits[0].occurrences[0].rects.len(), 1);
        assert!(highlight_unavailable);
    }

    #[test]
    fn search_ignores_dropped_glyphs_when_matched_glyphs_have_geometry() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "alpha beta",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: vec![
                    glyph('a', 0.0, 0.0, 10.0, 10.0),
                    glyph('l', 10.0, 0.0, 20.0, 10.0),
                    glyph('p', 20.0, 0.0, 30.0, 10.0),
                    glyph('h', 30.0, 0.0, 40.0, 10.0),
                    glyph('a', 40.0, 0.0, 50.0, 10.0),
                ],
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
        assert_eq!(hits[0].occurrences.len(), 1);
        assert!(!highlight_unavailable);
    }

    #[test]
    fn search_uses_glyph_order_without_coordinate_newline_inference() {
        let mut engine = SearchEngine::new();
        let pdf: Arc<dyn PdfBackend> = Arc::new(SearchPositionedStubPdf::new(
            "",
            TextPage {
                width_pt: 100.0,
                height_pt: 100.0,
                glyphs: vec![
                    glyph('縦', 80.0, 10.0, 92.0, 22.0),
                    glyph('書', 80.0, 24.0, 92.0, 36.0),
                    glyph('き', 80.0, 38.0, 92.0, 50.0),
                ],
                dropped_glyphs: 0,
            },
        ));
        let generation = engine
            .submit(
                pdf,
                "縦書き",
                Arc::new(ContainsMatcher {
                    case_sensitive: false,
                }),
            )
            .expect("submit should succeed");

        let (hits, highlight_unavailable) = wait_for_completed_hits(&mut engine, generation);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].occurrences.len(), 1);
        assert!(!highlight_unavailable);
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

    #[test]
    fn locate_occurrences_keeps_direct_and_whitespace_insensitive_matches() {
        let glyphs = vec![
            glyph('f', 10.0, 20.0, 18.0, 32.0),
            glyph('o', 20.0, 20.0, 28.0, 32.0),
            glyph('o', 30.0, 20.0, 38.0, 32.0),
            glyph('b', 40.0, 20.0, 48.0, 32.0),
            glyph('a', 50.0, 20.0, 58.0, 32.0),
            glyph('r', 60.0, 20.0, 68.0, 32.0),
            glyph(' ', 70.0, 20.0, 74.0, 32.0),
            glyph('f', 80.0, 20.0, 88.0, 32.0),
            glyph('o', 90.0, 20.0, 98.0, 32.0),
            glyph('o', 100.0, 20.0, 108.0, 32.0),
            glyph(' ', 110.0, 20.0, 114.0, 32.0),
            glyph('b', 120.0, 20.0, 128.0, 32.0),
            glyph('a', 130.0, 20.0, 138.0, 32.0),
            glyph('r', 140.0, 20.0, 148.0, 32.0),
        ];

        let occurrences = locate_occurrences(&glyphs, "foobar", false);

        assert_eq!(occurrences.len(), 2);
        assert_eq!(occurrences[0].glyph_start, 0);
        assert_eq!(occurrences[0].glyph_end, 5);
        assert_eq!(occurrences[1].glyph_start, 7);
        assert_eq!(occurrences[1].glyph_end, 13);
    }

    #[test]
    fn locate_occurrences_skips_overlapping_matches() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('a', 20.0, 20.0, 28.0, 32.0),
            glyph('a', 30.0, 20.0, 38.0, 32.0),
        ];

        let occurrences = locate_occurrences(&glyphs, "aa", true);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].glyph_start, 0);
        assert_eq!(occurrences[0].glyph_end, 1);
    }

    #[test]
    fn locate_occurrences_maps_multibyte_byte_offsets_to_char_positions() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('β', 20.0, 20.0, 28.0, 32.0),
            glyph('a', 30.0, 20.0, 38.0, 32.0),
            glyph('β', 40.0, 20.0, 48.0, 32.0),
        ];

        let occurrences = locate_occurrences(&glyphs, "βa", true);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].glyph_start, 1);
        assert_eq!(occurrences[0].glyph_end, 2);
    }

    #[test]
    fn locate_occurrences_preserves_multi_char_lowercase_expansion() {
        let glyphs = vec![glyph('İ', 10.0, 20.0, 18.0, 32.0)];

        let occurrences = locate_occurrences(&glyphs, "i\u{307}", false);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].glyph_start, 0);
        assert_eq!(occurrences[0].glyph_end, 0);
    }

    #[test]
    fn normalize_text_for_search_preserves_search_semantics() {
        assert_eq!(
            super::normalize_text_for_search("İ", false, false),
            "i\u{307}"
        );
        assert_eq!(super::normalize_text_for_search("İ", true, false), "İ");
        assert_eq!(
            super::normalize_text_for_search("A \tİ\nB", false, true),
            "ai\u{307}b"
        );
    }

    #[test]
    #[ignore = "manual timing aid for search normalization changes"]
    fn normalize_text_for_search_perf() {
        let sample = "Alpha βeta İSTANBUL  \tfoo\nbar ".repeat(20_000);
        let started = Instant::now();
        let mut bytes = 0;

        for _ in 0..100 {
            bytes += super::normalize_text_for_search(&sample, false, true).len();
        }

        eprintln!(
            "normalize_text_for_search_perf: {:?}, output bytes={bytes}",
            started.elapsed()
        );
    }

    #[test]
    fn apply_hit_snippet_uses_original_glyph_boundaries_after_case_fold_expansion() {
        let glyphs = vec![
            glyph('İ', 10.0, 20.0, 18.0, 32.0),
            glyph('x', 20.0, 20.0, 28.0, 32.0),
        ];
        let mut occurrence = SearchOccurrence {
            glyph_start: 0,
            glyph_end: 0,
            rects: vec![PdfRect {
                x0: 10.0,
                y0: 20.0,
                x1: 18.0,
                y1: 32.0,
            }],
            snippet: String::new(),
            snippet_match_start: None,
            snippet_match_end: None,
        };

        super::apply_hit_snippet(&mut occurrence, &glyphs);

        assert_eq!(occurrence.snippet, "İx");
        assert_eq!(occurrence.snippet_match_start, Some(0));
        assert_eq!(occurrence.snippet_match_end, Some('İ'.len_utf8()));
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
            bbox: Some(PdfRect { x0, y0, x1, y1 }),
        }
    }

    fn glyph_without_bbox(ch: char) -> TextGlyph {
        TextGlyph { ch, bbox: None }
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
