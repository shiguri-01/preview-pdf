use std::collections::VecDeque;
use std::sync::Arc;

use crate::app::{AppState, NoticeAction, PaletteRequest};
use crate::backend::SharedPdfBackend;
use crate::command::{CommandOutcome, SearchMatcherKind};
use crate::error::AppResult;
use crate::highlight::{HighlightOverlaySnapshot, HighlightSource, HighlightSpan, HighlightStyle};
use crate::palette::{PaletteKind, PaletteOpenPayload};

use super::engine::{SearchEngine, SearchEvent, SearchPageHit};
use super::matcher::matcher_for_kind;

#[derive(Default)]
pub struct SearchRuntime {
    state: SearchState,
    engine: SearchEngine,
}

impl SearchRuntime {
    pub fn with_engine(engine: SearchEngine) -> Self {
        Self {
            state: SearchState::default(),
            engine,
        }
    }

    pub fn open_palette(
        &mut self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        self.state.open_palette(app, palette_requests)
    }

    pub fn submit(
        &mut self,
        app: &mut AppState,
        pdf: SharedPdfBackend,
        query: String,
        matcher: SearchMatcherKind,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.state
            .submit(app, pdf, &mut self.engine, query, matcher)
    }

    pub fn open_results_palette(
        &mut self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        self.state.open_results_palette(app, palette_requests)
    }

    pub fn goto_result(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.state.goto_result(app, page_count, page)
    }

    pub fn cancel(&mut self, pdf: SharedPdfBackend) -> AppResult<bool> {
        self.state.cancel(pdf, &mut self.engine)
    }

    pub fn next_hit(&mut self, app: &mut AppState) -> (CommandOutcome, NoticeAction) {
        self.state.next_hit(app)
    }

    pub fn prev_hit(&mut self, app: &mut AppState) -> (CommandOutcome, NoticeAction) {
        self.state.prev_hit(app)
    }

    pub fn on_background(&mut self, app: &mut AppState) -> bool {
        self.state.on_background(app, &mut self.engine)
    }

    pub fn prewarm(&mut self, pdf: SharedPdfBackend) {
        self.engine.prewarm(pdf);
    }

    pub fn resolve_priority_geometry(
        &mut self,
        pdf: SharedPdfBackend,
        visible_pages: [Option<usize>; 2],
    ) {
        if self.state.query.is_empty() {
            return;
        }
        let pages = self.state.geometry_priority_pages(visible_pages, false);
        let matcher = matcher_for_kind(self.state.matcher);
        self.engine.resolve_geometry(
            pdf,
            self.state.generation,
            self.state.query.clone(),
            matcher,
            pages,
            true,
        );
    }

    pub fn matcher(&self) -> SearchMatcherKind {
        self.state.matcher()
    }

    pub fn query(&self) -> &str {
        self.state.query()
    }

    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    pub fn status_bar_segment(&self) -> Option<String> {
        self.state.status_bar_segment()
    }

    pub fn palette_entries(&self) -> Arc<[SearchPaletteEntry]> {
        self.state.palette_entries()
    }

    pub fn highlight_overlay_for_visible_pages(
        &self,
        visible_pages: [Option<usize>; 2],
    ) -> HighlightOverlaySnapshot {
        self.state
            .highlight_overlay_for_visible_pages(visible_pages)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPaletteEntry {
    pub index: usize,
    pub page: usize,
    pub snippet: String,
    pub snippet_match_start: Option<usize>,
    pub snippet_match_end: Option<usize>,
}

#[derive(Clone)]
pub struct SearchState {
    query: String,
    matcher: SearchMatcherKind,
    generation: u64,
    in_progress: bool,
    /// Number of scanned pages observed so far while scanning.
    /// This is progress-oriented and may change before completion.
    scanned_pages_progress: usize,
    total_pages: usize,
    /// Number of matched pages observed so far while scanning.
    /// This is progress-oriented and may change before completion.
    hit_pages_progress: usize,
    /// Final matched page list. Occurrence rects may be filled after completion by geometry events.
    hits: Vec<SearchPageHit>,
    /// Final matched occurrences for the results palette, ordered by page/match order.
    palette_entries: Arc<[SearchPaletteEntry]>,
    current_hit: Option<usize>,
    last_error: Option<String>,
    active_pdf: Option<SharedPdfBackend>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            matcher: SearchMatcherKind::ContainsInsensitive,
            generation: 0,
            in_progress: false,
            scanned_pages_progress: 0,
            total_pages: 0,
            hit_pages_progress: 0,
            hits: Vec::new(),
            palette_entries: Arc::from([]),
            current_hit: None,
            last_error: None,
            active_pdf: None,
        }
    }
}

impl SearchState {
    const HIGHLIGHT_UNAVAILABLE_NOTICE: &str = "some search highlights are unavailable";

    pub fn open_palette(
        &mut self,
        _app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        let payload = if self.query.is_empty() {
            None
        } else {
            Some(PaletteOpenPayload::Search {
                query: self.query.clone(),
                matcher: self.matcher,
            })
        };
        palette_requests.push_back(PaletteRequest::Open {
            kind: PaletteKind::Search,
            payload,
        });
        (CommandOutcome::Applied, NoticeAction::Clear)
    }

    pub fn open_results_palette(
        &mut self,
        _app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        if self.query.is_empty() {
            return (CommandOutcome::Noop, NoticeAction::Clear);
        }
        palette_requests.push_back(PaletteRequest::Open {
            kind: PaletteKind::SearchResults,
            payload: None,
        });
        (CommandOutcome::Applied, NoticeAction::Clear)
    }

    pub fn submit(
        &mut self,
        _app: &mut AppState,
        pdf: SharedPdfBackend,
        search_engine: &mut SearchEngine,
        query: String,
        matcher: SearchMatcherKind,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.query = query;
        self.matcher = matcher;

        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.generation = search_engine.cancel(Arc::clone(&pdf))?;
            self.query.clear();
            self.clear_results();
            return Ok((CommandOutcome::Noop, NoticeAction::Clear));
        }

        let matcher = matcher_for_kind(self.matcher);
        let generation = search_engine.submit(Arc::clone(&pdf), query.clone(), matcher)?;

        self.query = query;
        self.generation = generation;
        self.in_progress = true;
        self.scanned_pages_progress = 0;
        self.total_pages = pdf.page_count();
        self.hit_pages_progress = 0;
        self.hits.clear();
        self.palette_entries = Arc::from([]);
        self.current_hit = None;
        self.last_error = None;
        self.active_pdf = Some(Arc::clone(&pdf));
        Ok((CommandOutcome::Applied, NoticeAction::Clear))
    }

    pub fn next_hit(&mut self, app: &mut AppState) -> (CommandOutcome, NoticeAction) {
        self.move_hit(app, true)
    }

    pub fn prev_hit(&mut self, app: &mut AppState) -> (CommandOutcome, NoticeAction) {
        self.move_hit(app, false)
    }

    pub fn goto_result(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        if page < 1 {
            return Err(crate::error::AppError::invalid_argument(
                "page number must be >= 1",
            ));
        }
        if page > page_count {
            return Err(crate::error::AppError::page_out_of_range(page, page_count));
        }
        let hit_page = page - 1;
        let target = app.normalize_page_for_layout(hit_page, page_count);
        if app.current_page == target {
            self.current_hit = self.hits.iter().position(|hit| hit.page == hit_page);
            return Ok((CommandOutcome::Noop, NoticeAction::Clear));
        }
        app.current_page = target;
        self.current_hit = self.hits.iter().position(|hit| hit.page == hit_page);
        Ok((CommandOutcome::Applied, NoticeAction::Clear))
    }

    pub fn cancel(
        &mut self,
        pdf: SharedPdfBackend,
        search_engine: &mut SearchEngine,
    ) -> AppResult<bool> {
        if self.query.is_empty() {
            return Ok(false);
        }

        self.generation = search_engine.cancel(pdf)?;
        self.query.clear();
        self.clear_results();
        Ok(true)
    }

    pub fn on_background(&mut self, app: &mut AppState, search_engine: &mut SearchEngine) -> bool {
        let events = search_engine.drain_events();
        if events.is_empty() {
            return false;
        }
        // If search is inactive (e.g. canceled), drain pending worker events without
        // changing state/message. This avoids "search complete (0 hits)" flash after cancel.
        if self.query.is_empty() {
            return false;
        }

        let mut changed = false;
        for event in events {
            match event {
                SearchEvent::Snapshot(snapshot) => {
                    if snapshot.generation != self.generation {
                        continue;
                    }
                    self.scanned_pages_progress = snapshot.scanned_pages;
                    self.total_pages = snapshot.total_pages;
                    self.hit_pages_progress = snapshot.hit_pages;
                    self.in_progress = true;
                    changed = true;
                }
                SearchEvent::Completed {
                    generation,
                    hits,
                    highlight_unavailable,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.in_progress = false;
                    self.scanned_pages_progress = self.total_pages.max(self.scanned_pages_progress);
                    self.hit_pages_progress = hits.len();
                    self.current_hit = None;
                    self.palette_entries = build_palette_entries(&hits);
                    self.hits = hits;
                    let pages = self.geometry_priority_pages([Some(app.current_page), None], true);
                    if !pages.is_empty()
                        && let Some(pdf) = &self.active_pdf
                    {
                        let matcher = matcher_for_kind(self.matcher);
                        search_engine.resolve_geometry(
                            Arc::clone(pdf),
                            self.generation,
                            self.query.clone(),
                            matcher,
                            pages,
                            false,
                        );
                    }
                    if highlight_unavailable {
                        app.apply_notice_action(NoticeAction::warning(
                            Self::HIGHLIGHT_UNAVAILABLE_NOTICE,
                        ));
                    }
                    changed = true;
                }
                SearchEvent::GeometryResolved {
                    generation,
                    page,
                    occurrences,
                    highlight_unavailable,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    if let Some(hit) = self.hits.iter_mut().find(|hit| hit.page == page) {
                        hit.occurrences = occurrences;
                        self.palette_entries = build_palette_entries(&self.hits);
                        if highlight_unavailable {
                            app.apply_notice_action(NoticeAction::warning(
                                Self::HIGHLIGHT_UNAVAILABLE_NOTICE,
                            ));
                        }
                        changed = true;
                    }
                }
                SearchEvent::Failed {
                    generation,
                    message,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.in_progress = false;
                    self.last_error = Some(message.clone());
                    app.apply_notice_action(NoticeAction::error(format!(
                        "search failed: {message}"
                    )));
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn matcher(&self) -> SearchMatcherKind {
        self.matcher
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn in_progress(&self) -> bool {
        self.in_progress
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    pub fn status_bar_segment(&self) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }

        if let Some(current_hit) = self.current_hit {
            return Some(format!(
                "SEARCH {}/{}",
                current_hit + 1,
                self.hit_pages_progress
            ));
        }

        Some(format!("SEARCH {} hits", self.hit_pages_progress))
    }

    pub fn palette_entries(&self) -> Arc<[SearchPaletteEntry]> {
        Arc::clone(&self.palette_entries)
    }

    fn geometry_priority_pages(
        &self,
        visible_pages: [Option<usize>; 2],
        include_all_hits: bool,
    ) -> Vec<usize> {
        let mut pages = Vec::new();
        for page in visible_pages.into_iter().flatten() {
            push_unique_page(&mut pages, page);
        }

        if let Some(current_hit) = self.current_hit {
            if let Some(hit) = self.hits.get(current_hit) {
                push_unique_page(&mut pages, hit.page);
            }
            if let Some(hit) = self.hits.get((current_hit + 1) % self.hits.len().max(1)) {
                push_unique_page(&mut pages, hit.page);
            }
            if !self.hits.is_empty() {
                let prev = if current_hit == 0 {
                    self.hits.len() - 1
                } else {
                    current_hit - 1
                };
                if let Some(hit) = self.hits.get(prev) {
                    push_unique_page(&mut pages, hit.page);
                }
            }
        }
        if include_all_hits {
            for hit in &self.hits {
                push_unique_page(&mut pages, hit.page);
            }
        }
        pages
    }

    fn move_hit(&mut self, app: &mut AppState, forward: bool) -> (CommandOutcome, NoticeAction) {
        if self.hits.is_empty() {
            // Without an active search context, hit navigation does not need extra feedback.
            // Once a search has started, reflect its current state instead of looking like a
            // broken keybinding.
            let notice = if self.in_progress {
                if app.notice.is_some() {
                    NoticeAction::Keep
                } else {
                    NoticeAction::warning("searching...")
                }
            } else if self.last_error.is_some() {
                NoticeAction::Keep
            } else {
                NoticeAction::Clear
            };
            return (CommandOutcome::Noop, notice);
        }

        let next_index = if forward {
            match self.current_hit {
                Some(idx) => (idx + 1) % self.hits.len(),
                None => 0,
            }
        } else {
            match self.current_hit {
                Some(0) | None => self.hits.len() - 1,
                Some(idx) => idx - 1,
            }
        };

        self.current_hit = Some(next_index);
        let hit_page = self.hits[next_index].page;
        let page_count = self.total_pages.max(hit_page + 1);
        app.current_page = app.normalize_page_for_layout(hit_page, page_count);
        (CommandOutcome::Applied, NoticeAction::Clear)
    }

    pub fn highlight_overlay_for_visible_pages(
        &self,
        visible_pages: [Option<usize>; 2],
    ) -> HighlightOverlaySnapshot {
        if self.query.is_empty() {
            return HighlightOverlaySnapshot::default();
        }

        let spans = self
            .hits
            .iter()
            .filter(|hit| visible_pages.contains(&Some(hit.page)))
            .flat_map(|hit| {
                hit.occurrences.iter().filter_map(move |occurrence| {
                    (!occurrence.rects.is_empty()).then_some(HighlightSpan {
                        source: HighlightSource::Search,
                        page: hit.page,
                        rects: occurrence.rects.clone(),
                        style: HighlightStyle::SEARCH_HIT,
                    })
                })
            })
            .collect();
        HighlightOverlaySnapshot::new(spans)
    }

    fn clear_results(&mut self) {
        self.in_progress = false;
        self.scanned_pages_progress = 0;
        self.total_pages = 0;
        self.hit_pages_progress = 0;
        self.hits.clear();
        self.palette_entries = Arc::from([]);
        self.current_hit = None;
        self.last_error = None;
        self.active_pdf = None;
    }
}

fn push_unique_page(pages: &mut Vec<usize>, page: usize) {
    if !pages.contains(&page) {
        pages.push(page);
    }
}

fn build_palette_entries(hits: &[SearchPageHit]) -> Arc<[SearchPaletteEntry]> {
    Arc::from(
        hits.iter()
            .flat_map(|hit| {
                hit.occurrences
                    .iter()
                    .map(move |occurrence| SearchPaletteEntry {
                        index: 0,
                        page: hit.page,
                        snippet: occurrence.snippet.clone(),
                        snippet_match_start: occurrence.snippet_match_start,
                        snippet_match_end: occurrence.snippet_match_end,
                    })
            })
            .enumerate()
            .map(|(idx, mut entry)| {
                entry.index = idx + 1;
                entry
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::app::{AppState, NoticeAction, NoticeLevel, PageLayoutMode, PaletteRequest};
    use crate::backend::{PdfBackend, RgbaFrame, SharedPdfBackend, TextPage};
    use crate::command::{CommandOutcome, SearchMatcherKind};
    use crate::error::AppError;
    use crate::palette::{PaletteKind, PaletteOpenPayload};
    use crate::search::engine::{SearchEngine, SearchPageHit};

    use super::SearchState;

    struct StubPdf {
        path: PathBuf,
        page_count: usize,
    }

    impl StubPdf {
        fn new(page_count: usize) -> Self {
            Self {
                path: PathBuf::from("stub.pdf"),
                page_count,
            }
        }
    }

    impl PdfBackend for StubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            9
        }

        fn page_count(&self) -> usize {
            self.page_count
        }

        fn page_dimensions(&self, _page: usize) -> crate::error::AppResult<(f32, f32)> {
            Ok((612.0, 792.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> crate::error::AppResult<RgbaFrame> {
            Ok(RgbaFrame {
                width: 1,
                height: 1,
                pixels: vec![0, 0, 0, 0].into(),
            })
        }

        fn extract_text(&self, _page: usize) -> crate::error::AppResult<String> {
            Ok(String::new())
        }

        fn extract_positioned_text(&self, _page: usize) -> crate::error::AppResult<TextPage> {
            Ok(TextPage {
                width_pt: 612.0,
                height_pt: 792.0,
                glyphs: Vec::new(),
                dropped_glyphs: 0,
            })
        }

        fn extract_outline(&self) -> crate::error::AppResult<Vec<crate::backend::OutlineNode>> {
            Ok(Vec::new())
        }
    }

    struct HighlightUnavailableStubPdf {
        path: PathBuf,
        text: String,
    }

    impl HighlightUnavailableStubPdf {
        fn new(text: &str) -> Self {
            Self {
                path: PathBuf::from("highlight-unavailable.pdf"),
                text: text.to_string(),
            }
        }
    }

    impl PdfBackend for HighlightUnavailableStubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            10
        }

        fn page_count(&self) -> usize {
            1
        }

        fn page_dimensions(&self, _page: usize) -> crate::error::AppResult<(f32, f32)> {
            Ok((612.0, 792.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> crate::error::AppResult<RgbaFrame> {
            Ok(RgbaFrame {
                width: 1,
                height: 1,
                pixels: vec![0, 0, 0, 0].into(),
            })
        }

        fn extract_text(&self, _page: usize) -> crate::error::AppResult<String> {
            Ok(self.text.clone())
        }

        fn extract_positioned_text(&self, _page: usize) -> crate::error::AppResult<TextPage> {
            Err(AppError::unsupported("positioned text unavailable"))
        }

        fn extract_outline(&self) -> crate::error::AppResult<Vec<crate::backend::OutlineNode>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn submit_search_marks_search_active() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(5)) as SharedPdfBackend;
        let mut engine = SearchEngine::new();

        let (outcome, _) = state
            .submit(
                &mut app,
                Arc::clone(&pdf),
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");

        assert_eq!(outcome, CommandOutcome::Applied);
        assert!(state.is_active());
        assert!(state.in_progress());
        assert_eq!(state.total_pages(), 5);
        assert_eq!(
            state.status_bar_segment(),
            Some("SEARCH 0 hits".to_string())
        );
    }

    #[test]
    fn open_palette_includes_query_and_matcher_in_payload() {
        let mut state = SearchState {
            query: "needle".to_string(),
            matcher: SearchMatcherKind::ContainsSensitive,
            ..SearchState::default()
        };
        let mut app = AppState::default();
        let mut requests = VecDeque::new();

        let (outcome, notice) = state.open_palette(&mut app, &mut requests);

        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(
            requests.pop_front(),
            Some(PaletteRequest::Open {
                kind: PaletteKind::Search,
                payload: Some(PaletteOpenPayload::Search {
                    query: "needle".to_string(),
                    matcher: SearchMatcherKind::ContainsSensitive,
                }),
            })
        );
    }

    #[test]
    fn open_results_palette_requires_active_search() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let mut requests = VecDeque::new();

        let (outcome, notice) = state.open_results_palette(&mut app, &mut requests);

        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, NoticeAction::Clear);
        assert!(requests.is_empty());
    }

    #[test]
    fn open_results_palette_is_available_with_zero_hits_when_search_is_active() {
        let mut state = SearchState {
            query: "needle".to_string(),
            ..SearchState::default()
        };
        let mut app = AppState::default();
        let mut requests = VecDeque::new();

        let (outcome, notice) = state.open_results_palette(&mut app, &mut requests);

        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(
            requests.pop_front(),
            Some(PaletteRequest::Open {
                kind: PaletteKind::SearchResults,
                payload: None,
            })
        );
    }

    #[test]
    fn submit_empty_query_clears_search_active() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(2)) as SharedPdfBackend;
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                Arc::clone(&pdf),
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");
        assert!(state.is_active());

        let (outcome, _) = state
            .submit(
                &mut app,
                Arc::clone(&pdf),
                &mut engine,
                "   ".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("empty submit should succeed");

        assert_eq!(outcome, CommandOutcome::Noop);
        assert!(!state.is_active());
        assert!(!state.in_progress());
        assert_eq!(state.status_bar_segment(), None);
    }

    #[test]
    fn cancel_clears_active_search_state() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(2)) as SharedPdfBackend;
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                Arc::clone(&pdf),
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");
        assert!(state.is_active());

        let canceled = state
            .cancel(Arc::clone(&pdf), &mut engine)
            .expect("cancel should succeed");
        assert!(canceled);
        assert!(!state.is_active());
        assert_eq!(state.status_bar_segment(), None);
    }

    #[test]
    fn goto_result_moves_to_selected_page() {
        let mut state = SearchState {
            query: "needle".to_string(),
            hits: vec![page_hit(1), page_hit(4)],
            ..SearchState::default()
        };
        let mut app = AppState::default();

        let (outcome, notice) = state
            .goto_result(&mut app, 10, 5)
            .expect("goto result should succeed");

        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(app.current_page, 4);
    }

    #[test]
    fn goto_result_keeps_right_page_hit_selected_in_spread_layout() {
        let mut state = SearchState {
            query: "needle".to_string(),
            hits: vec![page_hit(5)],
            hit_pages_progress: 1,
            ..SearchState::default()
        };
        let mut app = AppState {
            page_layout_mode: PageLayoutMode::Spread,
            ..AppState::default()
        };

        let (outcome, notice) = state
            .goto_result(&mut app, 10, 6)
            .expect("goto result should succeed");

        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(app.current_page, 4);
        assert_eq!(state.current_hit, Some(0));
        assert_eq!(state.status_bar_segment(), Some("SEARCH 1/1".to_string()));
    }

    #[test]
    fn status_bar_segment_only_shows_position_after_hit_selection() {
        let state = SearchState {
            query: "needle".to_string(),
            hit_pages_progress: 3,
            current_hit: None,
            ..SearchState::default()
        };
        assert_eq!(
            state.status_bar_segment(),
            Some("SEARCH 3 hits".to_string())
        );

        let state = SearchState {
            query: "needle".to_string(),
            hit_pages_progress: 3,
            current_hit: Some(1),
            ..SearchState::default()
        };
        assert_eq!(state.status_bar_segment(), Some("SEARCH 2/3".to_string()));
    }

    #[test]
    fn on_background_ignores_synthetic_events_after_cancel() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(2)) as SharedPdfBackend;
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                Arc::clone(&pdf),
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");
        app.clear_notice();
        state
            .cancel(Arc::clone(&pdf), &mut engine)
            .expect("cancel should succeed");

        for _ in 0..20 {
            let changed = state.on_background(&mut app, &mut engine);
            assert!(!changed);
            assert_eq!(app.notice, None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn on_background_warns_when_highlight_is_unavailable() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = Arc::new(HighlightUnavailableStubPdf::new("alpha beta")) as SharedPdfBackend;
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                Arc::clone(&pdf),
                &mut engine,
                "alpha".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");

        let timeout = std::time::Duration::from_secs(3);
        let start = std::time::Instant::now();
        while state.in_progress() {
            let _ = state.on_background(&mut app, &mut engine);
            assert!(
                start.elapsed() <= timeout,
                "timed out waiting for search completion"
            );
            if state.in_progress() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        assert_eq!(
            state.status_bar_segment(),
            Some("SEARCH 1 hits".to_string())
        );
        let notice = app.notice.expect("warning notice should be set");
        assert_eq!(notice.level, NoticeLevel::Warning);
        assert_eq!(notice.message, SearchState::HIGHLIGHT_UNAVAILABLE_NOTICE);
    }

    #[test]
    fn next_hit_keeps_active_error_notice_when_no_hits_exist() {
        let mut state = SearchState {
            query: "needle".to_string(),
            last_error: Some("backend failed".to_string()),
            ..SearchState::default()
        };
        let mut app = AppState::default();
        app.set_error_notice("search failed: backend failed");

        let (outcome, notice) = state.next_hit(&mut app);

        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, NoticeAction::Keep);
        assert_eq!(
            app.notice.expect("existing notice should stay").message,
            "search failed: backend failed"
        );
    }

    fn page_hit(page: usize) -> SearchPageHit {
        SearchPageHit {
            page,
            occurrences: Vec::new(),
        }
    }

    #[test]
    fn next_hit_keeps_progress_notice_while_search_is_still_running() {
        let mut state = SearchState {
            query: "needle".to_string(),
            in_progress: true,
            ..SearchState::default()
        };
        let mut app = AppState::default();
        app.set_warning_notice("searching...");

        let (outcome, notice) = state.next_hit(&mut app);

        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, NoticeAction::Keep);
        assert_eq!(
            app.notice.expect("progress notice should stay").message,
            "searching..."
        );
    }

    #[test]
    fn next_hit_shows_progress_notice_when_search_is_running_without_notice() {
        let mut state = SearchState {
            query: "needle".to_string(),
            in_progress: true,
            ..SearchState::default()
        };
        let mut app = AppState::default();

        let (outcome, notice) = state.next_hit(&mut app);

        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, NoticeAction::warning("searching..."));
        assert!(app.notice.is_none());
    }

    #[test]
    fn next_hit_stays_silent_when_no_search_is_active() {
        let mut state = SearchState::default();
        let mut app = AppState::default();

        let (outcome, notice) = state.next_hit(&mut app);

        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, NoticeAction::Clear);
        assert!(app.notice.is_none());
    }
}
