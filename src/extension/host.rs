use std::sync::Arc;

use crate::app::AppState;
use crate::backend::SharedPdfBackend;
use crate::event::AppEvent;
use crate::highlight::HighlightOverlaySnapshot;
use crate::history::{HistoryCommandPort, HistoryExtension, HistoryState};
use crate::input::{AppInputEvent, InputHookResult};
use crate::outline::{OutlineCommandPort, OutlineExtension, OutlineState, OutlineUiSnapshot};
use crate::search::{
    SearchCommandPort, SearchEvent, SearchExtension, SearchRuntime, SearchUiSnapshot,
};

use super::traits::Extension;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionUiSnapshot {
    pub search: SearchUiSnapshot,
    pub outline: OutlineUiSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionRenderSnapshot {
    pub highlight_overlay: HighlightOverlaySnapshot,
}

pub(crate) struct ExtensionCommandPorts<'a> {
    pub search: SearchCommandPort<'a>,
    pub history: HistoryCommandPort<'a>,
    pub outline: OutlineCommandPort<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExtensionWorkerEvent {
    Search(SearchEvent),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExtensionEventOutcome {
    pub changed: bool,
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

    pub(crate) fn command_ports(&mut self) -> ExtensionCommandPorts<'_> {
        ExtensionCommandPorts {
            search: SearchCommandPort::new(&mut self.search),
            history: HistoryCommandPort::new(&mut self.history),
            outline: OutlineCommandPort::new(&mut self.outline),
        }
    }

    pub(crate) fn search(&self) -> &SearchRuntime {
        &self.search
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

    pub(crate) fn start_workers(
        &mut self,
        event_tx: tokio::sync::mpsc::UnboundedSender<ExtensionWorkerEvent>,
    ) {
        self.search.start_worker(event_tx);
    }

    pub(crate) fn handle_worker_events(
        &mut self,
        events: Vec<ExtensionWorkerEvent>,
        app: &mut AppState,
    ) -> ExtensionEventOutcome {
        let mut changed = false;
        for event in events {
            match event {
                ExtensionWorkerEvent::Search(event) => {
                    changed |= self.search.handle_worker_event(app, event);
                }
            }
        }
        ExtensionEventOutcome { changed }
    }

    pub fn on_document_opened(&mut self, pdf: SharedPdfBackend) {
        self.search.prewarm(pdf);
    }

    pub fn on_document_reloaded(&mut self, app: &mut AppState, pdf: SharedPdfBackend) {
        SearchExtension::on_document_reloaded(&mut self.search, app, Arc::clone(&pdf));
        HistoryExtension::on_document_reloaded(&mut self.history, app, Arc::clone(&pdf));
        OutlineExtension::on_document_reloaded(&mut self.outline, app, pdf);
    }

    pub fn on_visible_pages_changed(
        &mut self,
        pdf: SharedPdfBackend,
        visible_pages: [Option<usize>; 2],
    ) {
        self.search.prewarm(Arc::clone(&pdf));
        self.search.resolve_priority_geometry(pdf, visible_pages);
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
            search: self.search.ui_snapshot(),
            outline: self.outline.ui_snapshot(),
        }
    }

    pub fn render_snapshot(&self, visible_pages: [Option<usize>; 2]) -> ExtensionRenderSnapshot {
        ExtensionRenderSnapshot {
            highlight_overlay: self
                .search
                .highlight_overlay_for_visible_pages(visible_pages),
        }
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
    use crate::command::{CommandOutcome, SearchMatcherKind};
    use crate::event::{AppEvent, NavReason, PageGotoKind};

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
        fn extract_text_page(&self, _page: usize) -> crate::error::AppResult<TextPage> {
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

    fn test_extension_host() -> ExtensionHost {
        let mut host = ExtensionHost::new();
        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        host.start_workers(event_tx);
        host
    }

    #[test]
    fn status_bar_segments_is_empty_without_active_extensions() {
        let host = test_extension_host();
        let app = crate::app::AppState::default();
        assert!(host.status_bar_segments(&app).is_empty());
    }

    #[test]
    fn status_bar_segments_includes_search_when_query_is_active() {
        let mut host = test_extension_host();
        let mut app = crate::app::AppState::default();
        let pdf = StubPdf::new(4);

        host.command_ports()
            .search
            .submit(
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
        let mut host = test_extension_host();
        let mut app = crate::app::AppState::default();
        let pdf = Arc::new(StubPdf::new(4)) as SharedPdfBackend;

        host.command_ports()
            .search
            .submit(
                &mut app,
                Arc::clone(&pdf),
                "needle".to_string(),
                SearchMatcherKind::ContainsInsensitive,
            )
            .expect("submit-search should succeed");
        assert!(host.ui_snapshot().search.active);

        let canceled = host
            .command_ports()
            .search
            .cancel(pdf)
            .expect("cancel should succeed");
        assert!(canceled);
        assert!(!host.ui_snapshot().search.active);
    }

    #[test]
    fn document_reload_preserves_active_search() {
        let mut host = test_extension_host();
        let mut app = crate::app::AppState::default();
        let first = Arc::new(StubPdf::new(4)) as SharedPdfBackend;
        let second = Arc::new(StubPdf::new(2)) as SharedPdfBackend;

        host.command_ports()
            .search
            .submit(
                &mut app,
                first,
                "needle".to_string(),
                SearchMatcherKind::ContainsSensitive,
            )
            .expect("submit-search should succeed");

        host.on_document_reloaded(&mut app, second);

        assert!(host.ui_snapshot().search.active);
        assert_eq!(host.search().query(), "needle");
        assert_eq!(
            host.search().matcher(),
            SearchMatcherKind::ContainsSensitive
        );
        assert_eq!(host.status_bar_segments(&app), vec!["SEARCH 0 hits"]);
    }

    #[test]
    fn document_reload_resets_history_navigation() {
        let mut host = test_extension_host();
        let mut app = crate::app::AppState {
            current_page: 3,
            ..crate::app::AppState::default()
        };
        host.handle_event(
            &AppEvent::PageChanged {
                from: 0,
                to: 3,
                reason: NavReason::PageGoto(PageGotoKind::Specific),
            },
            &mut app,
        );

        host.on_document_reloaded(&mut app, Arc::new(StubPdf::new(4)) as SharedPdfBackend);
        let (outcome, notice) = host.command_ports().history.back(&mut app, 4);

        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, crate::app::NoticeAction::Clear);
        assert_eq!(app.current_page, 3);
    }
}
