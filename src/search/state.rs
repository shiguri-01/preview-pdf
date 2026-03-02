use std::collections::VecDeque;
use std::sync::Arc;

use crossterm::event::KeyCode;

use crate::app::{AppState, PaletteRequest, SearchUiState};
use crate::backend::PdfBackend;
use crate::command::{ActionId, Command, CommandOutcome, SearchMatcherKind};
use crate::error::AppResult;
use crate::input::{AppInputEvent, InputHookResult};
use crate::palette::PaletteKind;

use super::engine::{SearchEngine, SearchEvent, SearchMatcher};

#[derive(Debug, Clone)]
pub struct SearchState {
    query: String,
    matcher: SearchMatcherKind,
    generation: u64,
    in_progress: bool,
    scanned_pages: usize,
    total_pages: usize,
    hits_found: usize,
    hits: Vec<usize>,
    current_hit: Option<usize>,
    last_error: Option<String>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            matcher: SearchMatcherKind::ContainsInsensitive,
            generation: 0,
            in_progress: false,
            scanned_pages: 0,
            total_pages: 0,
            hits_found: 0,
            hits: Vec::new(),
            current_hit: None,
            last_error: None,
        }
    }
}

impl SearchState {
    pub fn open_palette(
        &mut self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> CommandOutcome {
        let seed = if self.query.is_empty() {
            None
        } else {
            Some(self.query.clone())
        };
        palette_requests.push_back(PaletteRequest::Open {
            kind: PaletteKind::Search,
            seed,
        });
        app.status.last_action_id = Some(ActionId::Search);
        app.status.message = "opening search palette".to_string();
        CommandOutcome::Applied
    }

    pub fn submit(
        &mut self,
        app: &mut AppState,
        pdf: &dyn PdfBackend,
        search_engine: &mut SearchEngine,
        query: String,
        matcher: SearchMatcherKind,
    ) -> AppResult<CommandOutcome> {
        app.status.last_action_id = Some(ActionId::SubmitSearch);
        self.query = query;
        self.matcher = matcher;

        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.generation = search_engine.cancel(pdf.path())?;
            self.query.clear();
            self.clear_results();
            self.sync_ui_state(app);
            app.status.message = "search query is empty".to_string();
            return Ok(CommandOutcome::Noop);
        }

        let matcher = matcher_for_kind(self.matcher);
        let generation = search_engine.submit(pdf.path(), query.clone(), matcher)?;

        self.query = query;
        self.generation = generation;
        self.in_progress = true;
        self.scanned_pages = 0;
        self.total_pages = pdf.page_count();
        self.hits_found = 0;
        self.hits.clear();
        self.current_hit = None;
        self.last_error = None;
        self.sync_ui_state(app);

        app.status.message = format!("search started ({})", self.matcher.id());
        Ok(CommandOutcome::Applied)
    }

    pub fn next_hit(&mut self, app: &mut AppState) -> CommandOutcome {
        self.move_hit(app, true)
    }

    pub fn prev_hit(&mut self, app: &mut AppState) -> CommandOutcome {
        self.move_hit(app, false)
    }

    pub fn cancel(
        &mut self,
        app: &mut AppState,
        pdf: &dyn PdfBackend,
        search_engine: &mut SearchEngine,
    ) -> AppResult<bool> {
        if self.query.is_empty() {
            return Ok(false);
        }

        self.generation = search_engine.cancel(pdf.path())?;
        self.query.clear();
        self.clear_results();
        self.sync_ui_state(app);
        Ok(true)
    }

    pub fn on_input(&mut self, event: AppInputEvent, app: &mut AppState) -> InputHookResult {
        let AppInputEvent::Key(key) = event;
        let _ = app;
        if key.code == KeyCode::Char('/') {
            let seed = if self.query.is_empty() {
                None
            } else {
                Some(self.query.clone())
            };
            return InputHookResult::EmitCommand(Command::OpenPalette {
                kind: PaletteKind::Search,
                seed,
            });
        }

        InputHookResult::Ignored
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
                    self.scanned_pages = snapshot.scanned_pages;
                    self.total_pages = snapshot.total_pages;
                    self.hits_found = snapshot.hit_pages;
                    self.in_progress = true;
                    app.status.last_action_id = Some(ActionId::SearchProgress);
                    app.status.message = format!(
                        "searching... {}/{} pages ({} hits)",
                        snapshot.scanned_pages, snapshot.total_pages, snapshot.hit_pages
                    );
                    changed = true;
                }
                SearchEvent::Completed { generation, hits } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.in_progress = false;
                    self.scanned_pages = self.total_pages.max(self.scanned_pages);
                    self.hits_found = hits.len();
                    self.current_hit = None;
                    self.hits = hits;
                    app.status.last_action_id = Some(ActionId::SearchComplete);
                    app.status.message = format!("search complete ({} hits)", self.hits.len());
                    changed = true;
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
                    app.status.last_action_id = Some(ActionId::SearchFailed);
                    app.status.message = format!("search failed: {message}");
                    changed = true;
                }
            }
        }
        if changed {
            self.sync_ui_state(app);
        }
        changed
    }

    pub fn matcher(&self) -> SearchMatcherKind {
        self.matcher
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn status_bar_segment(&self) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }

        if let Some(current_hit) = self.current_hit {
            return Some(format!("SEARCH {}/{}", current_hit + 1, self.hits_found));
        }

        Some(format!("SEARCH {} hits", self.hits_found))
    }

    fn move_hit(&mut self, app: &mut AppState, forward: bool) -> CommandOutcome {
        app.status.last_action_id = Some(if forward {
            ActionId::NextSearchHit
        } else {
            ActionId::PrevSearchHit
        });

        if self.hits.is_empty() {
            app.status.message = if self.in_progress {
                "search is still in progress".to_string()
            } else {
                "no search hits available".to_string()
            };
            self.sync_ui_state(app);
            return CommandOutcome::Noop;
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
        app.current_page = self.hits[next_index];
        app.status.message = format!(
            "search hit {}/{} (page {})",
            next_index + 1,
            self.hits.len(),
            app.current_page + 1
        );
        self.sync_ui_state(app);
        CommandOutcome::Applied
    }

    fn clear_results(&mut self) {
        self.in_progress = false;
        self.scanned_pages = 0;
        self.total_pages = 0;
        self.hits_found = 0;
        self.hits.clear();
        self.current_hit = None;
        self.last_error = None;
    }

    fn sync_ui_state(&self, app: &mut AppState) {
        app.search_ui = SearchUiState {
            active: !self.query.is_empty(),
            in_progress: self.in_progress,
            scanned_pages: self.scanned_pages,
            total_pages: self.total_pages,
            hits_found: self.hits_found,
            current_hit: self.current_hit,
        };
    }
}

fn matcher_for_kind(kind: SearchMatcherKind) -> Arc<dyn SearchMatcher> {
    Arc::new(ContainsMatcher {
        case_sensitive: kind == SearchMatcherKind::ContainsSensitive,
    })
}

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::AppState;
    use crate::backend::{PdfBackend, RgbaFrame};
    use crate::command::{CommandOutcome, SearchMatcherKind};
    use crate::search::engine::SearchEngine;

    use super::SearchState;
    use crate::input::{AppInputEvent, InputHookResult};
    use crate::palette::PaletteKind;

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
    }

    #[test]
    fn slash_key_opens_search_palette() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let result = state.on_input(
            AppInputEvent::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
            &mut app,
        );
        assert!(matches!(
            result,
            InputHookResult::EmitCommand(crate::command::Command::OpenPalette {
                kind: PaletteKind::Search,
                ..
            })
        ));
    }

    #[test]
    fn non_search_key_is_ignored() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let result = state.on_input(
            AppInputEvent::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            &mut app,
        );
        assert_eq!(result, InputHookResult::Ignored);
    }

    #[test]
    fn submit_search_marks_search_ui_active() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = StubPdf::new(5);
        let mut engine = SearchEngine::new();

        let outcome = state
            .submit(
                &mut app,
                &pdf,
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");

        assert_eq!(outcome, CommandOutcome::Applied);
        assert!(app.search_ui.active);
        assert!(app.search_ui.in_progress);
        assert_eq!(app.search_ui.total_pages, 5);
        assert_eq!(
            state.status_bar_segment(),
            Some("SEARCH 0 hits".to_string())
        );
    }

    #[test]
    fn submit_empty_query_clears_search_ui() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = StubPdf::new(2);
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                &pdf,
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");
        assert!(app.search_ui.active);

        let outcome = state
            .submit(
                &mut app,
                &pdf,
                &mut engine,
                "   ".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("empty submit should succeed");

        assert_eq!(outcome, CommandOutcome::Noop);
        assert!(!app.search_ui.active);
        assert!(!app.search_ui.in_progress);
        assert_eq!(state.status_bar_segment(), None);
    }

    #[test]
    fn cancel_clears_active_search_state() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = StubPdf::new(2);
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                &pdf,
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");
        assert!(app.search_ui.active);

        let canceled = state
            .cancel(&mut app, &pdf, &mut engine)
            .expect("cancel should succeed");
        assert!(canceled);
        assert!(!app.search_ui.active);
        assert_eq!(state.status_bar_segment(), None);
    }

    #[test]
    fn status_bar_segment_only_shows_position_after_hit_selection() {
        let state = SearchState {
            query: "needle".to_string(),
            hits_found: 3,
            current_hit: None,
            ..SearchState::default()
        };
        assert_eq!(
            state.status_bar_segment(),
            Some("SEARCH 3 hits".to_string())
        );

        let state = SearchState {
            query: "needle".to_string(),
            hits_found: 3,
            current_hit: Some(1),
            ..SearchState::default()
        };
        assert_eq!(state.status_bar_segment(), Some("SEARCH 2/3".to_string()));
    }

    #[test]
    fn on_background_ignores_synthetic_events_after_cancel() {
        let mut state = SearchState::default();
        let mut app = AppState::default();
        let pdf = StubPdf::new(2);
        let mut engine = SearchEngine::new();

        state
            .submit(
                &mut app,
                &pdf,
                &mut engine,
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit should succeed");
        app.status.message = "search canceled".to_string();
        state
            .cancel(&mut app, &pdf, &mut engine)
            .expect("cancel should succeed");

        for _ in 0..20 {
            let changed = state.on_background(&mut app, &mut engine);
            assert!(!changed);
            assert_eq!(app.status.message, "search canceled");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
