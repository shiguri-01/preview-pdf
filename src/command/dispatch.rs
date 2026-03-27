use std::collections::VecDeque;
use std::sync::Arc;

use crate::app::{AppState, Mode, NoticeAction, PaletteRequest};
use crate::backend::SharedPdfBackend;
use crate::error::AppResult;
use crate::event::{AppEvent, GotoKind, HistoryOp, NavReason};
use crate::extension::ExtensionHost;

use super::core::{
    close_help, first_page, goto_page, last_page, next_page, open_help, prev_page,
    set_debug_status_visible, set_page_layout, set_zoom,
};
use super::spec::{CommandConditionContext, rejection_message_for_command};
use super::types::{Command, CommandInvocationSource, CommandOutcome};

const ZOOM_STEP: f32 = 0.1;

#[derive(Debug, Clone)]
pub struct CommandDispatchResult {
    pub outcome: CommandOutcome,
    pub emitted_events: Vec<AppEvent>,
}

fn apply_notice(app: &mut AppState, action: NoticeAction) {
    app.apply_notice_action(action);
}

fn rejection_notice(command: &Command, message: String) -> NoticeAction {
    match command {
        // Search hit navigation is often retried while search state is still settling,
        // so keep the current notice instead of replacing it with a generic rejection.
        Command::NextSearchHit | Command::PrevSearchHit => NoticeAction::Keep,
        _ => NoticeAction::warning(message),
    }
}

pub fn dispatch(
    app: &mut AppState,
    cmd: Command,
    source: CommandInvocationSource,
    pdf: SharedPdfBackend,
    extension_host: &mut ExtensionHost,
    palette_requests: &mut VecDeque<PaletteRequest>,
) -> AppResult<CommandDispatchResult> {
    let action_id = cmd.action_id();
    let extensions = extension_host.ui_snapshot();
    let ctx = CommandConditionContext {
        app,
        extensions: &extensions,
        source,
    };
    if let Some(message) = rejection_message_for_command(&cmd, &ctx) {
        apply_notice(app, rejection_notice(&cmd, message));
        let outcome = CommandOutcome::Noop;
        return Ok(CommandDispatchResult {
            outcome,
            emitted_events: vec![AppEvent::CommandExecuted {
                id: action_id,
                outcome,
            }],
        });
    }

    let previous_page = app.current_page;
    let prev_mode = app.mode;
    let dispatched_command = cmd.clone();
    let page_count = pdf.page_count();

    let (outcome, notice) = match cmd {
        Command::NextPage => next_page(app, page_count),
        Command::PrevPage => prev_page(app, page_count),
        Command::FirstPage => first_page(app, page_count),
        Command::LastPage => last_page(app, page_count),
        Command::GotoPage { page } => goto_page(app, page_count, page),
        Command::SetZoom { value } => set_zoom(app, value),
        Command::ZoomIn => set_zoom(app, app.zoom + ZOOM_STEP),
        Command::ZoomOut => set_zoom(app, app.zoom - ZOOM_STEP),
        Command::Scroll { dx, dy } => {
            app.scroll_x = app.scroll_x.saturating_add(dx);
            app.scroll_y = app.scroll_y.saturating_add(dy);
            Ok((CommandOutcome::Applied, NoticeAction::Clear))
        }
        Command::SetPageLayout { mode, direction } => {
            set_page_layout(app, page_count, mode, direction)
        }
        Command::DebugStatusShow => set_debug_status_visible(app, true),
        Command::DebugStatusHide => set_debug_status_visible(app, false),
        Command::DebugStatusToggle => {
            let visible = !app.debug_status_visible;
            set_debug_status_visible(app, visible)
        }
        Command::OpenPalette { kind, seed } => {
            palette_requests.push_back(PaletteRequest::Open { kind, seed });
            Ok((CommandOutcome::Applied, NoticeAction::Clear))
        }
        Command::ClosePalette => {
            palette_requests.push_back(PaletteRequest::Close);
            Ok((CommandOutcome::Applied, NoticeAction::Clear))
        }
        Command::OpenHelp => open_help(app),
        Command::CloseHelp => close_help(app),
        Command::OpenSearch => Ok(extension_host.open_search_palette(app, palette_requests)),
        Command::SubmitSearch { query, matcher } => {
            extension_host.submit_search(app, Arc::clone(&pdf), query, matcher)
        }
        Command::NextSearchHit => Ok(extension_host.next_search_hit(app)),
        Command::PrevSearchHit => Ok(extension_host.prev_search_hit(app)),
        Command::HistoryBack => Ok(extension_host.history_back(app, page_count)),
        Command::HistoryForward => Ok(extension_host.history_forward(app, page_count)),
        Command::HistoryGoto { page } => extension_host.history_goto(app, page_count, page),
        Command::OpenHistory => Ok(extension_host.open_history_palette(app, palette_requests)),
        Command::OpenOutline => extension_host.open_outline_palette(pdf, palette_requests),
        Command::OutlineGoto { page, .. } => extension_host.outline_goto(app, page_count, page),
        Command::Cancel => {
            if app.mode == Mode::Palette {
                palette_requests.push_back(PaletteRequest::Close);
            } else {
                let _ = extension_host.cancel_search(pdf)?;
            }
            Ok((CommandOutcome::Applied, NoticeAction::Clear))
        }
        Command::Quit => Ok((CommandOutcome::QuitRequested, NoticeAction::Keep)),
    }?;
    apply_notice(app, notice);

    let mut emitted_events = collect_transition_events(
        app,
        extension_host,
        previous_page,
        prev_mode,
        &dispatched_command,
        outcome,
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
    outcome: CommandOutcome,
) -> Vec<AppEvent> {
    let mut events = Vec::new();
    if let Some(reason) = derive_nav_reason(command, extension_host) {
        let should_emit = match reason {
            // Search/outline are intent-driven navigations. Even if layout normalization
            // leaves the visible anchor page unchanged, history still needs the event so the
            // attempted destination is recorded consistently.
            NavReason::Search { .. } | NavReason::Outline { .. } => {
                outcome == CommandOutcome::Applied
            }
            _ => app.current_page != prev_page,
        };
        if should_emit {
            events.push(AppEvent::PageChanged {
                from: prev_page,
                to: app.current_page,
                reason,
            });
        }
    }

    if app.mode != prev_mode {
        events.push(AppEvent::ModeChanged {
            from: prev_mode,
            to: app.mode,
        });
    }
    events
}

fn derive_nav_reason(command: &Command, extension_host: &ExtensionHost) -> Option<NavReason> {
    match command {
        Command::NextPage | Command::PrevPage => Some(NavReason::Step),
        Command::FirstPage => Some(NavReason::Goto(GotoKind::FirstPage)),
        Command::LastPage => Some(NavReason::Goto(GotoKind::LastPage)),
        Command::GotoPage { .. } => Some(NavReason::Goto(GotoKind::SpecificPage)),
        Command::SetPageLayout { .. } => Some(NavReason::LayoutNormalize),
        Command::NextSearchHit | Command::PrevSearchHit => Some(NavReason::Search {
            query: extension_host.search_query().to_string(),
        }),
        Command::HistoryBack => Some(NavReason::History(HistoryOp::Back)),
        Command::HistoryForward => Some(NavReason::History(HistoryOp::Forward)),
        Command::HistoryGoto { .. } => Some(NavReason::History(HistoryOp::Goto)),
        Command::OutlineGoto { title, .. } => Some(NavReason::Outline {
            title: title.clone(),
        }),
        Command::SetZoom { .. }
        | Command::ZoomIn
        | Command::ZoomOut
        | Command::Scroll { .. }
        | Command::DebugStatusShow
        | Command::DebugStatusHide
        | Command::DebugStatusToggle
        | Command::OpenPalette { .. }
        | Command::ClosePalette
        | Command::OpenHelp
        | Command::CloseHelp
        | Command::OpenSearch
        | Command::SubmitSearch { .. }
        | Command::OpenHistory
        | Command::OpenOutline
        | Command::Cancel
        | Command::Quit => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::app::AppState;
    use crate::backend::{PdfBackend, RgbaFrame, SharedPdfBackend};
    use crate::command::{
        ActionId, Command, CommandInvocationSource, CommandOutcome, PageLayoutModeArg,
        SearchMatcherKind,
    };
    use crate::event::{AppEvent, NavReason};
    use crate::extension::ExtensionHost;

    use super::{collect_transition_events, dispatch};

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

        fn extract_outline(&self) -> crate::error::AppResult<Vec<crate::backend::OutlineNode>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn dispatch_next_page_emits_page_changed_and_command_executed() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::NextPage,
            CommandInvocationSource::Keymap,
            pdf,
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
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::ClosePalette,
            CommandInvocationSource::PaletteProvider,
            pdf,
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

    #[test]
    fn dispatch_open_help_changes_mode_and_emits_mode_event() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::OpenHelp,
            CommandInvocationSource::Keymap,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(app.mode, crate::app::Mode::Help);
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 2);
        assert!(matches!(
            result.emitted_events[0],
            AppEvent::ModeChanged {
                from: crate::app::Mode::Normal,
                to: crate::app::Mode::Help
            }
        ));
        assert!(matches!(
            result.emitted_events[1],
            AppEvent::CommandExecuted {
                id: ActionId::Help,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_close_help_returns_to_normal_mode() {
        let mut app = AppState {
            mode: crate::app::Mode::Help,
            help_scroll: 3,
            ..AppState::default()
        };
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::CloseHelp,
            CommandInvocationSource::Keymap,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(app.mode, crate::app::Mode::Normal);
        assert_eq!(app.help_scroll, 0);
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 2);
        assert!(matches!(
            result.emitted_events[0],
            AppEvent::ModeChanged {
                from: crate::app::Mode::Help,
                to: crate::app::Mode::Normal
            }
        ));
        assert!(matches!(
            result.emitted_events[1],
            AppEvent::CommandExecuted {
                id: ActionId::CloseHelp,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_cancel_clears_active_search() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        host.submit_search(
            &mut app,
            Arc::clone(&pdf),
            "needle".to_string(),
            SearchMatcherKind::ContainsInsensitive,
        )
        .expect("submit-search should succeed");
        assert!(host.ui_snapshot().search_active);

        let result = dispatch(
            &mut app,
            Command::Cancel,
            CommandInvocationSource::Keymap,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 1);
        assert!(!host.ui_snapshot().search_active);
        assert_eq!(app.notice, None);
        assert!(palette_requests.is_empty());
    }

    #[test]
    fn collect_transition_events_emits_search_when_page_is_unchanged() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        host.submit_search(
            &mut app,
            pdf,
            "needle".to_string(),
            SearchMatcherKind::ContainsInsensitive,
        )
        .expect("submit-search should succeed");

        let prev_page = app.current_page;
        let prev_mode = app.mode;
        let events = collect_transition_events(
            &mut app,
            &host,
            prev_page,
            prev_mode,
            &Command::NextSearchHit,
            CommandOutcome::Applied,
        );

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AppEvent::PageChanged {
                from,
                to,
                reason: NavReason::Search { query }
            } if *from == 0 && *to == 0 && query == "needle"
        ));
    }

    #[test]
    fn dispatch_set_page_layout_emits_layout_normalize_when_anchor_changes() {
        let mut app = AppState {
            current_page: 3,
            ..AppState::default()
        };
        let pdf = Arc::new(StubPdf::new(8)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::SetPageLayout {
                mode: PageLayoutModeArg::Spread,
                direction: None,
            },
            CommandInvocationSource::Keymap,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 2);
        assert!(matches!(
            result.emitted_events[0],
            AppEvent::PageChanged {
                from: 3,
                to: 2,
                reason: NavReason::LayoutNormalize
            }
        ));
    }

    #[test]
    fn collect_transition_events_emits_outline_reason() {
        let mut app = AppState {
            current_page: 4,
            ..AppState::default()
        };
        let host = ExtensionHost::default();
        let prev_mode = app.mode;
        let events = collect_transition_events(
            &mut app,
            &host,
            1,
            prev_mode,
            &Command::OutlineGoto {
                page: 4,
                title: "Section".to_string(),
            },
            CommandOutcome::Applied,
        );

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AppEvent::PageChanged {
                from: 1,
                to: 4,
                reason: NavReason::Outline { title }
            } if title == "Section"
        ));
    }

    #[test]
    fn collect_transition_events_emits_outline_reason_when_page_is_unchanged() {
        let mut app = AppState::default();
        let host = ExtensionHost::default();
        let prev_mode = app.mode;
        let events = collect_transition_events(
            &mut app,
            &host,
            0,
            prev_mode,
            &Command::OutlineGoto {
                page: 0,
                title: "Section".to_string(),
            },
            CommandOutcome::Applied,
        );

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AppEvent::PageChanged {
                from: 0,
                to: 0,
                reason: NavReason::Outline { title }
            } if title == "Section"
        ));
    }

    #[test]
    fn dispatch_rejects_internal_command_from_keymap() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::SubmitSearch {
                query: "needle".to_string(),
                matcher: SearchMatcherKind::ContainsInsensitive,
            },
            CommandInvocationSource::Keymap,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert_eq!(
            app.notice.as_ref().map(|notice| notice.message.as_str()),
            Some("submit-search is an internal command and cannot be invoked directly")
        );
    }

    #[test]
    fn dispatch_rejects_unavailable_command_from_keymap() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::NextSearchHit,
            CommandInvocationSource::Keymap,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert!(app.notice.is_none());
    }
}
