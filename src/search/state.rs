use std::collections::VecDeque;
use std::sync::Arc;

use crossterm::event::KeyCode;

use crate::app::{AppState, PaletteRequest};
use crate::backend::PdfBackend;
use crate::command::{ActionId, Command, CommandOutcome, SearchMatcherKind};
use crate::error::AppResult;
use crate::extension::{AppInputEvent, InputHookResult};
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

        app.status.message = format!("search started ({})", self.matcher.id());
        Ok(CommandOutcome::Applied)
    }

    pub fn next_hit(&mut self, app: &mut AppState) -> CommandOutcome {
        self.move_hit(app, true)
    }

    pub fn prev_hit(&mut self, app: &mut AppState) -> CommandOutcome {
        self.move_hit(app, false)
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
        changed
    }

    pub fn matcher(&self) -> SearchMatcherKind {
        self.matcher
    }

    pub fn query(&self) -> &str {
        &self.query
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::AppState;

    use super::SearchState;
    use crate::extension::{AppInputEvent, InputHookResult};
    use crate::palette::PaletteKind;

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
}
