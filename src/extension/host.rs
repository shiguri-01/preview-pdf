use std::collections::VecDeque;

use crate::app::{AppState, PaletteRequest};
use crate::backend::PdfBackend;
use crate::command::{CommandOutcome, SearchMatcherKind};
use crate::error::AppResult;
use crate::history::{HistoryExtension, HistoryState};
use crate::search::engine::SearchEngine;
use crate::search::{SearchExtension, SearchState};

use super::events::AppEvent;
use super::input::{AppInputEvent, InputHookResult};
use super::traits::Extension;

pub struct ExtensionHost {
    search: SearchState,
    history: HistoryState,
    search_engine: SearchEngine,
}

impl ExtensionHost {
    pub fn new() -> Self {
        Self::with_search_engine(SearchEngine::new())
    }

    pub fn with_search_engine(search_engine: SearchEngine) -> Self {
        Self {
            search: SearchExtension::init_state(),
            history: HistoryExtension::init_state(),
            search_engine,
        }
    }

    pub fn handle_input(&mut self, event: AppInputEvent, app: &mut AppState) -> InputHookResult {
        let search_result = SearchExtension::handle_input(&mut self.search, event, app);
        if search_result != InputHookResult::Ignored {
            return search_result;
        }

        let history_result = HistoryExtension::handle_input(&mut self.history, event, app);
        if history_result != InputHookResult::Ignored {
            return history_result;
        }

        InputHookResult::Ignored
    }

    pub fn handle_event(&mut self, event: &AppEvent, app: &mut AppState) {
        SearchExtension::handle_event(&mut self.search, event, app);
        HistoryExtension::handle_event(&mut self.history, event, app);
    }

    pub fn drain_background(&mut self, app: &mut AppState) -> bool {
        let search_changed =
            SearchExtension::drain_background(&mut self.search, app, &mut self.search_engine);
        let history_changed = HistoryExtension::on_background(&mut self.history, app);
        search_changed || history_changed
    }

    pub fn open_search_palette(
        &mut self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> CommandOutcome {
        self.search.open_palette(app, palette_requests)
    }

    pub fn submit_search(
        &mut self,
        app: &mut AppState,
        pdf: &dyn PdfBackend,
        query: String,
        matcher: SearchMatcherKind,
    ) -> AppResult<CommandOutcome> {
        self.search
            .submit(app, pdf, &mut self.search_engine, query, matcher)
    }

    pub fn next_search_hit(&mut self, app: &mut AppState) -> CommandOutcome {
        self.search.next_hit(app)
    }

    pub fn prev_search_hit(&mut self, app: &mut AppState) -> CommandOutcome {
        self.search.prev_hit(app)
    }

    pub fn history_back(&mut self, app: &mut AppState) -> CommandOutcome {
        self.history.back(app)
    }

    pub fn history_forward(&mut self, app: &mut AppState) -> CommandOutcome {
        self.history.forward(app)
    }

    pub fn history_goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<CommandOutcome> {
        self.history.goto(app, page_count, page)
    }

    pub fn open_history_palette(
        &self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> CommandOutcome {
        self.history.open_palette(app, palette_requests)
    }

    pub fn search_query(&self) -> &str {
        self.search.query()
    }

    pub fn search_matcher(&self) -> SearchMatcherKind {
        self.search.matcher()
    }
}

impl Default for ExtensionHost {
    fn default() -> Self {
        Self::new()
    }
}
