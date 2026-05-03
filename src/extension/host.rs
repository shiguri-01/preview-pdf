use std::collections::VecDeque;
use std::sync::Arc;

use crate::app::{AppState, NoticeAction, PaletteRequest};
use crate::backend::SharedPdfBackend;
use crate::command::{CommandOutcome, SearchMatcherKind};
use crate::error::AppResult;
use crate::event::AppEvent;
use crate::highlight::HighlightOverlaySnapshot;
use crate::history::{HistoryExtension, HistoryState};
use crate::input::{AppInputEvent, InputHookResult};
use crate::outline::{OutlineExtension, OutlinePaletteEntry, OutlineState};
use crate::search::{SearchExtension, SearchPaletteEntry, SearchRuntime};

use super::traits::Extension;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionUiSnapshot {
    pub search_active: bool,
    pub search_results_entries: Arc<[SearchPaletteEntry]>,
    pub outline_entries: Arc<[OutlinePaletteEntry]>,
}

impl ExtensionUiSnapshot {
    pub fn with_search_active(search_active: bool) -> Self {
        Self {
            search_active,
            search_results_entries: Arc::from([]),
            outline_entries: Arc::from([]),
        }
    }
}

pub struct ExtensionHost {
    search: SearchRuntime,
    history: HistoryState,
    outline: OutlineState,
}

impl ExtensionHost {
    pub fn new() -> Self {
        Self {
            search: SearchExtension::init_state(),
            history: HistoryExtension::init_state(),
            outline: OutlineExtension::init_state(),
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
        OutlineExtension::handle_event(&mut self.outline, event, app);
    }

    pub fn drain_background(&mut self, app: &mut AppState) -> bool {
        let search_changed = SearchExtension::drain_background(&mut self.search, app);
        let history_changed = HistoryExtension::on_background(&mut self.history, app);
        search_changed || history_changed
    }

    pub fn open_search_palette(
        &mut self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        self.search.open_palette(app, palette_requests)
    }

    pub fn submit_search(
        &mut self,
        app: &mut AppState,
        pdf: SharedPdfBackend,
        query: String,
        matcher: SearchMatcherKind,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.search.submit(app, pdf, query, matcher)
    }

    pub fn prewarm_search_text(&mut self, pdf: SharedPdfBackend) {
        self.search.prewarm(pdf);
    }

    pub fn open_search_results_palette(
        &mut self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        self.search.open_results_palette(app, palette_requests)
    }

    pub fn search_result_goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.search.goto_result(app, page_count, page)
    }

    pub fn cancel_search(&mut self, pdf: SharedPdfBackend) -> AppResult<bool> {
        self.search.cancel(pdf)
    }

    pub fn next_search_hit(&mut self, app: &mut AppState) -> (CommandOutcome, NoticeAction) {
        self.search.next_hit(app)
    }

    pub fn prev_search_hit(&mut self, app: &mut AppState) -> (CommandOutcome, NoticeAction) {
        self.search.prev_hit(app)
    }

    pub fn history_back(
        &mut self,
        app: &mut AppState,
        page_count: usize,
    ) -> (CommandOutcome, NoticeAction) {
        self.history.back(app, page_count)
    }

    pub fn history_forward(
        &mut self,
        app: &mut AppState,
        page_count: usize,
    ) -> (CommandOutcome, NoticeAction) {
        self.history.forward(app, page_count)
    }

    pub fn history_goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.history.goto(app, page_count, page)
    }

    pub fn open_history_palette(
        &self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> (CommandOutcome, NoticeAction) {
        self.history.open_palette(app, palette_requests)
    }

    pub fn open_outline_palette(
        &mut self,
        pdf: SharedPdfBackend,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.outline.open_palette(pdf, palette_requests)
    }

    pub fn outline_goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.outline.goto(app, page_count, page)
    }

    pub fn search_query(&self) -> &str {
        self.search.query()
    }

    pub fn search_matcher(&self) -> SearchMatcherKind {
        self.search.matcher()
    }

    pub fn status_bar_segments(&self, app: &AppState) -> Vec<String> {
        let mut segments = Vec::new();
        if let Some(segment) = SearchExtension::status_bar_segment(&self.search, app)
            && !segment.is_empty()
        {
            segments.push(segment);
        }
        if let Some(segment) = HistoryExtension::status_bar_segment(&self.history, app)
            && !segment.is_empty()
        {
            segments.push(segment);
        }
        segments
    }

    pub fn ui_snapshot(&self) -> ExtensionUiSnapshot {
        ExtensionUiSnapshot {
            search_active: self.search.is_active(),
            search_results_entries: self.search.palette_entries(),
            outline_entries: self.outline.palette_entries(),
        }
    }

    pub fn highlight_overlay_for(
        &self,
        visible_pages: [Option<usize>; 2],
    ) -> HighlightOverlaySnapshot {
        self.search
            .highlight_overlay_for_visible_pages(visible_pages)
    }
}

impl Default for ExtensionHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::backend::{PdfBackend, RgbaFrame, SharedPdfBackend, TextPage};
    use crate::command::SearchMatcherKind;

    use super::ExtensionHost;

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
            1
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

    #[test]
    fn status_bar_segments_is_empty_without_active_extensions() {
        let host = ExtensionHost::default();
        let app = crate::app::AppState::default();
        assert!(host.status_bar_segments(&app).is_empty());
    }

    #[test]
    fn status_bar_segments_includes_search_when_query_is_active() {
        let mut host = ExtensionHost::default();
        let mut app = crate::app::AppState::default();
        let pdf = StubPdf::new(4);

        host.submit_search(
            &mut app,
            Arc::new(pdf) as SharedPdfBackend,
            "needle".to_string(),
            SearchMatcherKind::ContainsInsensitive,
        )
        .expect("submit-search should succeed");

        let segments = host.status_bar_segments(&app);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "SEARCH 0 hits");
    }

    #[test]
    fn cancel_search_clears_active_query() {
        let mut host = ExtensionHost::default();
        let mut app = crate::app::AppState::default();
        let pdf = Arc::new(StubPdf::new(4)) as SharedPdfBackend;

        host.submit_search(
            &mut app,
            Arc::clone(&pdf),
            "needle".to_string(),
            SearchMatcherKind::ContainsInsensitive,
        )
        .expect("submit-search should succeed");
        assert!(host.ui_snapshot().search_active);

        let canceled = host
            .cancel_search(pdf)
            .expect("cancel-search should succeed");
        assert!(canceled);
        assert!(!host.ui_snapshot().search_active);
    }
}
