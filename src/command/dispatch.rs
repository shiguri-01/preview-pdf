use std::collections::VecDeque;

use crate::app::{AppState, Mode, NoticeAction, PaletteRequest};
use crate::backend::SharedPdfBackend;
use crate::config::ViewPolicy;
use crate::error::AppResult;
use crate::event::{AppEvent, HistoryOp, NavReason, PageGotoKind};
use crate::extension::ExtensionHost;
use crate::input::InputHistoryService;
use crate::palette::{PaletteManager, PaletteRegistry};

use super::catalog::{Command, CommandRequest, execute_registered_command};
use crate::condition::RuntimeConditionContext;

use super::effects::{CommandExecution, CommandLifecycleEffect};
use super::spec::{CommandPolicyContext, rejection_message_for_command};
use super::types::{CommandInvocationSource, CommandOutcome};

#[derive(Debug, Clone)]
pub struct CommandDispatchResult {
    pub outcome: CommandOutcome,
    pub emitted_events: Vec<AppEvent>,
    pub follow_up_commands: Vec<CommandRequest>,
    pub lifecycle: CommandLifecycleEffect,
}

pub struct CommandDispatchContext<'a> {
    pub pdf: SharedPdfBackend,
    pub extension_host: &'a mut ExtensionHost,
    pub palette_registry: &'a PaletteRegistry,
    pub palette_manager: &'a mut PaletteManager,
    pub palette_requests: &'a mut VecDeque<PaletteRequest>,
    pub input_history: &'a mut InputHistoryService,
}

pub(super) struct CommandExecContext<'a> {
    pub app: &'a mut AppState,
    pub view_policy: ViewPolicy,
    pub pdf: SharedPdfBackend,
    pub extension_host: &'a mut ExtensionHost,
    pub palette_registry: &'a PaletteRegistry,
    pub palette_manager: &'a mut PaletteManager,
}

impl CommandExecContext<'_> {
    pub(super) fn page_count(&self) -> usize {
        self.pdf.page_count()
    }
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

pub fn dispatch_with_view_policy(
    app: &mut AppState,
    view_policy: ViewPolicy,
    cmd: Command,
    source: CommandInvocationSource,
    dispatch_ctx: CommandDispatchContext<'_>,
) -> AppResult<CommandDispatchResult> {
    let CommandDispatchContext {
        pdf,
        extension_host,
        palette_registry,
        palette_manager,
        palette_requests,
        input_history,
    } = dispatch_ctx;
    let command_id = cmd.command_id();
    let extensions = extension_host.ui_snapshot();
    let active_palette = palette_manager.active_kind();
    let palette_input_empty = palette_manager.active_input_is_empty();
    let ctx = CommandPolicyContext {
        source,
        runtime: RuntimeConditionContext::with_palette_input_empty(
            app.mode,
            active_palette,
            palette_input_empty,
            &extensions,
        ),
    };
    if let Some(message) = rejection_message_for_command(&cmd, &ctx) {
        apply_notice(app, rejection_notice(&cmd, message));
        let outcome = CommandOutcome::Noop;
        return Ok(CommandDispatchResult {
            outcome,
            emitted_events: vec![AppEvent::CommandExecuted {
                id: command_id,
                outcome,
            }],
            follow_up_commands: Vec::new(),
            lifecycle: CommandLifecycleEffect::None,
        });
    }

    let previous_page = app.current_page;
    let prev_mode = app.mode;
    let dispatched_command = cmd.clone();
    let execution = execute_registered_command(
        &mut CommandExecContext {
            app,
            view_policy,
            pdf,
            extension_host: &mut *extension_host,
            palette_registry,
            palette_manager: &mut *palette_manager,
        },
        cmd,
    )?;
    let CommandExecution { outcome, effects } = execution;
    apply_notice(app, effects.notice);
    for record in effects.input_history_records {
        input_history.record(record);
    }
    palette_requests.extend(effects.palette_requests);

    let mut emitted_events = collect_transition_events(
        app,
        extension_host,
        previous_page,
        prev_mode,
        &dispatched_command,
        outcome,
    );
    emitted_events.extend(effects.events);
    emitted_events.push(AppEvent::CommandExecuted {
        id: command_id,
        outcome,
    });

    Ok(CommandDispatchResult {
        outcome,
        emitted_events,
        follow_up_commands: effects.follow_up_commands,
        lifecycle: effects.lifecycle,
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
        Command::FirstPage => Some(NavReason::PageGoto(PageGotoKind::First)),
        Command::LastPage => Some(NavReason::PageGoto(PageGotoKind::Last)),
        Command::GotoPage { .. } => Some(NavReason::PageGoto(PageGotoKind::Specific)),
        Command::PageLayoutSingle | Command::PageLayoutSpread { .. } => {
            Some(NavReason::LayoutNormalize)
        }
        Command::SearchResultGoto { .. } | Command::NextSearchHit | Command::PrevSearchHit => {
            Some(NavReason::Search {
                query: extension_host.search_query().to_string(),
            })
        }
        Command::HistoryBack => Some(NavReason::History(HistoryOp::Back)),
        Command::HistoryForward => Some(NavReason::History(HistoryOp::Forward)),
        Command::HistoryGoto { .. } => Some(NavReason::History(HistoryOp::Goto)),
        Command::OutlineGoto { title, .. } => Some(NavReason::Outline {
            title: title.clone(),
        }),
        Command::SetZoom { .. }
        | Command::ZoomIn
        | Command::ZoomOut
        | Command::ZoomReset
        | Command::Pan { .. }
        | Command::DebugStatusShow
        | Command::DebugStatusHide
        | Command::DebugStatusToggle
        | Command::OpenPalette { .. }
        | Command::ClosePalette
        | Command::PaletteSubmit
        | Command::PaletteComplete
        | Command::PaletteSelectNext
        | Command::PaletteSelectPrev
        | Command::TextInsert { .. }
        | Command::TextDeleteBackward
        | Command::TextDeleteForward
        | Command::TextMoveLeft
        | Command::TextMoveRight
        | Command::TextMoveStart
        | Command::TextMoveEnd
        | Command::TextMovePrevWord
        | Command::TextMoveNextWord
        | Command::TextDeletePrevWord
        | Command::TextDeleteNextWord
        | Command::TextDeleteLine
        | Command::TextDeleteToEnd
        | Command::TextYank
        | Command::PaletteInputHistoryOlder
        | Command::PaletteInputHistoryNewer
        | Command::OpenHelp
        | Command::CloseHelp
        | Command::HelpScrollDown
        | Command::HelpScrollUp
        | Command::OpenSearch
        | Command::OpenSearchResults
        | Command::SubmitSearch { .. }
        | Command::OpenHistory
        | Command::OpenOutline
        | Command::CancelSearch
        | Command::ReloadDocument
        | Command::Quit => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::app::scale::zoom_eq;
    use crate::app::{
        AppState, Mode, Notice, NoticeLevel, PaletteRequest, SpreadCoverPolicy, SpreadDirection,
    };
    use crate::backend::{PdfBackend, RgbaFrame, SharedPdfBackend, TextPage};
    use crate::command::{
        Command, CommandId, CommandInvocationSource, CommandLifecycleEffect, CommandOutcome,
        PanAmount, PanDirection, SearchMatcherKind, SpreadCoverPolicyArg,
    };
    use crate::config::ViewPolicy;
    use crate::event::{AppEvent, NavReason};
    use crate::extension::ExtensionHost;
    use crate::input::InputHistoryService;
    use crate::palette::{PaletteKind, PaletteManager, PaletteRegistry};

    use super::{
        CommandDispatchContext, CommandDispatchResult, collect_transition_events,
        dispatch_with_view_policy,
    };

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

    fn dispatch(
        app: &mut AppState,
        cmd: Command,
        source: CommandInvocationSource,
        pdf: SharedPdfBackend,
        extension_host: &mut ExtensionHost,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> crate::error::AppResult<CommandDispatchResult> {
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let mut history = InputHistoryService::default();
        dispatch_with_view_policy(
            app,
            ViewPolicy::default(),
            cmd,
            source,
            CommandDispatchContext {
                pdf,
                extension_host,
                palette_registry: &registry,
                palette_manager: &mut manager,
                palette_requests,
                input_history: &mut history,
            },
        )
    }

    fn new_zoom_test_fixture() -> (SharedPdfBackend, ExtensionHost, VecDeque<PaletteRequest>) {
        (
            Arc::new(StubPdf::new(3)) as SharedPdfBackend,
            ExtensionHost::default(),
            VecDeque::new(),
        )
    }

    #[test]
    fn dispatch_quit_requests_quit_and_emits_command_executed() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::Quit,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.lifecycle, CommandLifecycleEffect::Quit);
        assert_eq!(
            result.emitted_events,
            vec![AppEvent::CommandExecuted {
                id: CommandId::Quit,
                outcome: CommandOutcome::Applied,
            }]
        );
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
            CommandInvocationSource::Binding,
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
                id: CommandId::NextPage,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_zoom_in_and_out_follow_the_zoom_ladder() {
        let mut app = AppState {
            zoom: 1.0,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::ZoomIn,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 1.1));
        assert_eq!(result.outcome, CommandOutcome::Applied);

        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();
        let result = dispatch(
            &mut app,
            Command::ZoomOut,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 1.0));
        assert_eq!(result.outcome, CommandOutcome::Applied);
    }

    #[test]
    fn dispatch_zoom_reset_restores_default_zoom_and_pan() {
        let mut app = AppState {
            zoom: 2.0,
            pan_x: 4,
            pan_y: -3,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::ZoomReset,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 1.0));
        assert_eq!(app.pan_x, 0);
        assert_eq!(app.pan_y, 0);
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(result.emitted_events.len(), 1);
        assert!(matches!(
            result.emitted_events[0],
            AppEvent::CommandExecuted {
                id: CommandId::ZoomReset,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_pan_applies_explicit_cell_amount() {
        let mut app = AppState::default();
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::Pan {
                direction: PanDirection::Right,
                amount: PanAmount::Cells(3),
            },
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(app.pan_x, 3);
        assert_eq!(app.pan_y, 0);
        assert_eq!(result.outcome, CommandOutcome::Applied);
    }

    #[test]
    fn dispatch_set_zoom_warns_when_input_is_clamped() {
        let mut app = AppState {
            zoom: 4.0,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::SetZoom { value: 10.0 },
            CommandInvocationSource::CommandPaletteInput,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 4.0));
        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "maximum zoom is 4.00x".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_set_zoom_warns_when_input_is_slightly_above_maximum() {
        let mut app = AppState::default();
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::SetZoom { value: 4.0004 },
            CommandInvocationSource::CommandPaletteInput,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 4.0));
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "maximum zoom is 4.00x".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_set_zoom_warns_when_input_is_below_minimum() {
        let mut app = AppState::default();
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::SetZoom { value: 0.1 },
            CommandInvocationSource::CommandPaletteInput,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 0.25));
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "minimum zoom is 0.25x".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_set_zoom_warns_when_input_is_slightly_below_minimum() {
        let mut app = AppState::default();
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::SetZoom { value: 0.2497 },
            CommandInvocationSource::CommandPaletteInput,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 0.25));
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "minimum zoom is 0.25x".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_zoom_in_at_maximum_keeps_the_boundary_warning() {
        let mut app = AppState {
            zoom: 4.0,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::ZoomIn,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 4.0));
        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "maximum zoom is 4.00x".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_zoom_out_at_minimum_keeps_the_boundary_warning() {
        let mut app = AppState {
            zoom: 0.25,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::ZoomOut,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 0.25));
        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "minimum zoom is 0.25x".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_zoom_in_near_maximum_advances_without_boundary_warning() {
        let mut app = AppState {
            zoom: 3.9997,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::ZoomIn,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 4.0));
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(app.notice, None);
    }

    #[test]
    fn dispatch_zoom_out_near_minimum_advances_without_boundary_warning() {
        let mut app = AppState {
            zoom: 0.2503,
            ..AppState::default()
        };
        let (pdf, mut host, mut palette_requests) = new_zoom_test_fixture();

        let result = dispatch(
            &mut app,
            Command::ZoomOut,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert!(zoom_eq(app.zoom, 0.25));
        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(app.notice, None);
    }

    #[test]
    fn dispatch_open_palette_emits_command_executed_only() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::OpenPalette {
                kind: PaletteKind::Command,
                payload: None,
            },
            CommandInvocationSource::Binding,
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
                id: CommandId::OpenPalette,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_palette_submit_closes_palette_and_returns_completed_command() {
        let mut app = AppState {
            mode: Mode::Palette,
            ..AppState::default()
        };
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let extensions = host.ui_snapshot();
        manager
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Command,
                None,
                None,
            )
            .expect("command palette should open");
        let mut palette_requests = VecDeque::new();
        let mut history = InputHistoryService::default();

        let result = dispatch_with_view_policy(
            &mut app,
            ViewPolicy::default(),
            Command::PaletteSubmit,
            CommandInvocationSource::Binding,
            CommandDispatchContext {
                pdf,
                extension_host: &mut host,
                palette_registry: &registry,
                palette_manager: &mut manager,
                palette_requests: &mut palette_requests,
                input_history: &mut history,
            },
        )
        .expect("palette submit should dispatch");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(app.mode, Mode::Normal);
        assert!(!manager.is_open());
        assert!(matches!(
            result.follow_up_commands.as_slice(),
            [request]
                if request.command == Command::NextPage
                    && request.source == CommandInvocationSource::CommandPaletteInput
        ));
    }

    #[test]
    fn dispatch_search_palette_submit_returns_internal_follow_up() {
        let mut app = AppState {
            mode: Mode::Palette,
            ..AppState::default()
        };
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let extensions = host.ui_snapshot();
        manager
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Search,
                None,
                None,
            )
            .expect("search palette should open");
        manager
            .insert_text(&registry, &app, &extensions, "needle")
            .expect("search input should be inserted");
        let mut palette_requests = VecDeque::new();
        let mut history = InputHistoryService::default();

        let result = dispatch_with_view_policy(
            &mut app,
            ViewPolicy::default(),
            Command::PaletteSubmit,
            CommandInvocationSource::Binding,
            CommandDispatchContext {
                pdf,
                extension_host: &mut host,
                palette_registry: &registry,
                palette_manager: &mut manager,
                palette_requests: &mut palette_requests,
                input_history: &mut history,
            },
        )
        .expect("palette submit should dispatch");

        assert!(matches!(
            result.follow_up_commands.as_slice(),
            [request]
                if matches!(request.command, Command::SubmitSearch { .. })
                    && request.source == CommandInvocationSource::Internal
        ));
    }

    #[test]
    fn dispatch_rejects_palette_command_without_active_palette() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let mut palette_requests = VecDeque::new();
        let mut history = InputHistoryService::default();

        let result = dispatch_with_view_policy(
            &mut app,
            ViewPolicy::default(),
            Command::PaletteSelectNext,
            CommandInvocationSource::Binding,
            CommandDispatchContext {
                pdf,
                extension_host: &mut host,
                palette_registry: &registry,
                palette_manager: &mut manager,
                palette_requests: &mut palette_requests,
                input_history: &mut history,
            },
        )
        .expect("palette command rejection should be reported as noop");

        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert!(
            app.notice
                .as_ref()
                .is_some_and(|notice| notice.message.contains("active palette"))
        );
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
            CommandInvocationSource::Binding,
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
                id: CommandId::OpenHelp,
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
            CommandInvocationSource::Binding,
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
                id: CommandId::CloseHelp,
                outcome: CommandOutcome::Applied
            }
        ));
    }

    #[test]
    fn dispatch_help_scroll_commands_require_help_mode() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::HelpScrollDown,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should reject through noop result");

        assert_eq!(app.mode, crate::app::Mode::Normal);
        assert_eq!(app.help_scroll, 0);
        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert_eq!(
            app.notice,
            Some(Notice {
                level: NoticeLevel::Warning,
                message: "help-scroll-down is unavailable outside help".to_string(),
            })
        );
    }

    #[test]
    fn dispatch_help_scroll_commands_update_help_scroll() {
        let mut app = AppState {
            mode: crate::app::Mode::Help,
            ..AppState::default()
        };
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let down = dispatch(
            &mut app,
            Command::HelpScrollDown,
            CommandInvocationSource::Binding,
            Arc::clone(&pdf),
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(app.help_scroll, 1);
        assert_eq!(down.outcome, CommandOutcome::Applied);
        assert!(matches!(
            down.emitted_events.as_slice(),
            [AppEvent::CommandExecuted {
                id: CommandId::HelpScrollDown,
                outcome: CommandOutcome::Applied
            }]
        ));

        let up = dispatch(
            &mut app,
            Command::HelpScrollUp,
            CommandInvocationSource::Binding,
            Arc::clone(&pdf),
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(app.help_scroll, 0);
        assert_eq!(up.outcome, CommandOutcome::Applied);
        assert!(matches!(
            up.emitted_events.as_slice(),
            [AppEvent::CommandExecuted {
                id: CommandId::HelpScrollUp,
                outcome: CommandOutcome::Applied
            }]
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
            Command::CancelSearch,
            CommandInvocationSource::Binding,
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
            Command::PageLayoutSpread {
                direction: None,
                cover_policy: None,
            },
            CommandInvocationSource::Binding,
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
    fn dispatch_page_layout_spread_without_cover_policy_resets_to_paired() {
        let mut app = AppState {
            current_page: 1,
            page_layout_mode: crate::app::PageLayoutMode::Spread,
            spread_cover_policy: SpreadCoverPolicy::Cover,
            ..AppState::default()
        };
        let pdf = Arc::new(StubPdf::new(8)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::PageLayoutSpread {
                direction: None,
                cover_policy: None,
            },
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(app.spread_cover_policy, SpreadCoverPolicy::Paired);
        assert_eq!(app.current_page, 0);
    }

    #[test]
    fn dispatch_page_layout_spread_cover_keeps_cover_policy() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(8)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::PageLayoutSpread {
                direction: None,
                cover_policy: Some(SpreadCoverPolicyArg::Cover),
            },
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(app.spread_cover_policy, SpreadCoverPolicy::Cover);
        assert_eq!(app.current_page, 0);
    }

    #[test]
    fn dispatch_page_layout_spread_uses_view_policy_for_omitted_spread_args() {
        let mut app = AppState {
            spread_direction: SpreadDirection::Ltr,
            spread_cover_policy: SpreadCoverPolicy::Paired,
            ..AppState::default()
        };
        let view_policy = ViewPolicy {
            spread_direction: SpreadDirection::Rtl,
            spread_cover: SpreadCoverPolicy::Cover,
            ..ViewPolicy::default()
        };
        let pdf = Arc::new(StubPdf::new(8)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let registry = PaletteRegistry::default();
        let mut manager = PaletteManager::default();
        let mut palette_requests = VecDeque::new();
        let mut history = InputHistoryService::default();

        let result = dispatch_with_view_policy(
            &mut app,
            view_policy,
            Command::PageLayoutSpread {
                direction: None,
                cover_policy: None,
            },
            CommandInvocationSource::Binding,
            CommandDispatchContext {
                pdf,
                extension_host: &mut host,
                palette_registry: &registry,
                palette_manager: &mut manager,
                palette_requests: &mut palette_requests,
                input_history: &mut history,
            },
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Applied);
        assert_eq!(app.spread_direction, SpreadDirection::Rtl);
        assert_eq!(app.spread_cover_policy, SpreadCoverPolicy::Cover);
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
    fn dispatch_rejects_internal_command_from_binding() {
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
            CommandInvocationSource::Binding,
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
    fn dispatch_rejects_unavailable_command_from_binding() {
        let mut app = AppState::default();
        let pdf = Arc::new(StubPdf::new(3)) as SharedPdfBackend;
        let mut host = ExtensionHost::default();
        let mut palette_requests = VecDeque::new();

        let result = dispatch(
            &mut app,
            Command::NextSearchHit,
            CommandInvocationSource::Binding,
            pdf,
            &mut host,
            &mut palette_requests,
        )
        .expect("dispatch should succeed");

        assert_eq!(result.outcome, CommandOutcome::Noop);
        assert!(app.notice.is_none());
    }
}
