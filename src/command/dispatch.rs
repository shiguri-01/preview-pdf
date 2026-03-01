use std::collections::VecDeque;

use crate::app::{AppState, Mode, PaletteRequest};
use crate::backend::PdfBackend;
use crate::error::AppResult;
use crate::extension::{AppEvent, ExtensionHost, NavReason};

use super::core::{
    first_page, goto_page, last_page, next_page, prev_page, set_debug_status_visible, set_zoom,
    set_zoom_with_id,
};
use super::types::{ActionId, Command, CommandOutcome};

const ZOOM_STEP: f32 = 0.1;

#[derive(Debug, Clone)]
pub struct CommandDispatchResult {
    pub outcome: CommandOutcome,
    pub emitted_events: Vec<AppEvent>,
}

pub fn dispatch(
    app: &mut AppState,
    cmd: Command,
    pdf: &mut dyn PdfBackend,
    extension_host: &mut ExtensionHost,
    palette_requests: &mut VecDeque<PaletteRequest>,
) -> AppResult<CommandDispatchResult> {
    let previous_page = app.current_page;
    let prev_mode = app.mode;
    let dispatched_command = cmd.clone();
    let action_id = dispatched_command.action_id();
    let page_count = pdf.page_count();

    let outcome = match cmd {
        Command::NextPage => next_page(app, page_count),
        Command::PrevPage => prev_page(app, page_count),
        Command::FirstPage => first_page(app, page_count),
        Command::LastPage => last_page(app, page_count),
        Command::GotoPage { page } => goto_page(app, page_count, page),
        Command::SetZoom { value } => set_zoom(app, value),
        Command::ZoomIn => set_zoom_with_id(app, app.zoom + ZOOM_STEP, ActionId::ZoomIn),
        Command::ZoomOut => set_zoom_with_id(app, app.zoom - ZOOM_STEP, ActionId::ZoomOut),
        Command::Scroll { dx, dy } => {
            app.scroll_x = app.scroll_x.saturating_add(dx);
            app.scroll_y = app.scroll_y.saturating_add(dy);
            app.status.last_action_id = Some(ActionId::Scroll);
            app.status.message = format!("scrolled to ({}, {})", app.scroll_x, app.scroll_y);
            Ok(CommandOutcome::Applied)
        }
        Command::DebugStatusShow => set_debug_status_visible(app, true, ActionId::DebugStatusShow),
        Command::DebugStatusHide => set_debug_status_visible(app, false, ActionId::DebugStatusHide),
        Command::DebugStatusToggle => {
            let visible = !app.debug_status_visible;
            set_debug_status_visible(app, visible, ActionId::DebugStatusToggle)
        }
        Command::OpenPalette { kind, seed } => {
            palette_requests.push_back(PaletteRequest::Open { kind, seed });
            app.status.last_action_id = Some(ActionId::OpenPalette);
            app.status.message = "opening palette".to_string();
            Ok(CommandOutcome::Applied)
        }
        Command::ClosePalette => {
            palette_requests.push_back(PaletteRequest::Close);
            app.status.last_action_id = Some(ActionId::ClosePalette);
            app.status.message = "closing palette".to_string();
            Ok(CommandOutcome::Applied)
        }
        Command::OpenSearch => Ok(extension_host.open_search_palette(app, palette_requests)),
        Command::SubmitSearch { query, matcher } => {
            extension_host.submit_search(app, pdf, query, matcher)
        }
        Command::NextSearchHit => Ok(extension_host.next_search_hit(app)),
        Command::PrevSearchHit => Ok(extension_host.prev_search_hit(app)),
        Command::HistoryBack => Ok(extension_host.history_back(app)),
        Command::HistoryForward => Ok(extension_host.history_forward(app)),
        Command::HistoryGoto { page } => extension_host.history_goto(app, page_count, page),
        Command::OpenHistory => Ok(extension_host.open_history_palette(app, palette_requests)),
        Command::Cancel => {
            if app.mode == Mode::Palette {
                palette_requests.push_back(PaletteRequest::Close);
            } else {
                app.mode = Mode::Normal;
            }
            app.status.last_action_id = Some(ActionId::Cancel);
            app.status.message = "canceled current mode".to_string();
            Ok(CommandOutcome::Applied)
        }
        Command::Quit => {
            app.status.last_action_id = Some(ActionId::Quit);
            app.status.message = "quit requested".to_string();
            Ok(CommandOutcome::QuitRequested)
        }
    }?;

    let mut emitted_events = collect_transition_events(
        app,
        extension_host,
        previous_page,
        prev_mode,
        &dispatched_command,
    );
    emitted_events.push(AppEvent::CommandExecuted {
        id: action_id,
        outcome,
    });

    Ok(CommandDispatchResult {
        outcome,
        emitted_events,
    })
}

pub fn drain_background_events(app: &mut AppState, extension_host: &mut ExtensionHost) -> bool {
    extension_host.drain_background(app)
}

fn collect_transition_events(
    app: &mut AppState,
    extension_host: &ExtensionHost,
    prev_page: usize,
    prev_mode: Mode,
    command: &Command,
) -> Vec<AppEvent> {
    let mut events = Vec::new();
    if app.current_page != prev_page {
        events.push(AppEvent::PageChanged {
            from: prev_page,
            to: app.current_page,
            reason: derive_nav_reason(command, extension_host),
        });
    }

    if app.mode != prev_mode {
        events.push(AppEvent::ModeChanged {
            from: prev_mode,
            to: app.mode,
        });
    }
    events
}

fn derive_nav_reason(command: &Command, extension_host: &ExtensionHost) -> NavReason {
    match command {
        Command::NextPage | Command::PrevPage => NavReason::Step,
        Command::FirstPage | Command::LastPage | Command::GotoPage { .. } => NavReason::Jump,
        Command::SubmitSearch { query, .. } => NavReason::Search(query.clone()),
        Command::NextSearchHit | Command::PrevSearchHit => {
            NavReason::Search(extension_host.search_query().to_string())
        }
        Command::HistoryBack | Command::HistoryForward | Command::HistoryGoto { .. } => {
            NavReason::History
        }
        _ => NavReason::Jump,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};

    use crate::app::AppState;
    use crate::backend::{PdfBackend, RgbaFrame};
    use crate::command::{ActionId, Command, CommandOutcome};
    use crate::extension::{AppEvent, ExtensionHost, NavReason};

    use super::dispatch;

    struct StubPdf {
        path: PathBuf,
        doc_id: u64,
        page_count: usize,
    }

    impl StubPdf {
        fn new(page_count: usize) -> Self {
            Self {
                path: PathBuf::from("stub.pdf"),
                doc_id: 7,
                page_count,
            }
        }
    }

    impl PdfBackend for StubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            self.doc_id
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
                pixels: vec![0; 4].into(),
            })
        }

        fn extract_text(&self, _page: usize) -> crate::error::AppResult<String> {
            Ok(String::new())
        }
    }

    #[test]
    fn dispatch_next_page_emits_page_changed_and_command_executed() {
        let mut app = AppState::default();
        let mut pdf = StubPdf::new(3);
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::NextPage,
            &mut pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 2);
        assert!(matches!(
            result.emitted_events[0],
            AppEvent::PageChanged {
                from: 0,
                to: 1,
                reason: NavReason::Step
            }
        ));
        assert!(matches!(
            result.emitted_events[1],
            AppEvent::CommandExecuted {
                id: ActionId::NextPage,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_open_palette_emits_command_executed_only() {
        let mut app = AppState::default();
        let mut pdf = StubPdf::new(3);
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::ClosePalette,
            &mut pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 1);
        assert!(matches!(
            result.emitted_events[0],
            AppEvent::CommandExecuted {
                id: ActionId::ClosePalette,
                outcome: CommandOutcome::Applied
            }
        ));
    }
}
