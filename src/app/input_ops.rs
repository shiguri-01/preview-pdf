use crossterm::event::KeyEvent;

use crate::backend::SharedPdfBackend;
use crate::command::{
    Command, CommandDispatchContext, CommandDispatchResult, CommandInvocationSource,
    CommandRequest, dispatch_with_view_policy,
};
use crate::condition::RuntimeConditionContext;
use crate::config::ViewPolicy;
use crate::error::AppResult;
use crate::event::AppEvent;
use crate::extension::{ExtensionUiSnapshot, ExtensionWorkerEvent};
use crate::input::sequence::{KeyBindingContext, SequenceResolution};
use crate::input::{AppInputEvent, InputHookResult};
use crate::palette::PaletteView;

use super::core::InteractionSubsystem;
use super::state::{AppState, Mode, PaletteRequest};

#[derive(Debug, Clone, Default)]
pub(crate) struct KeyEventOutcome {
    pub redraw: bool,
    pub commands: Vec<CommandRequest>,
}

impl InteractionSubsystem {
    pub(crate) fn handle_key_event(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
    ) -> AppResult<KeyEventOutcome> {
        Ok(self.handle_key_binding(state, key))
    }

    fn handle_key_binding(&mut self, state: &mut AppState, key: KeyEvent) -> KeyEventOutcome {
        // Once a sequence has started, keep routing keys through the resolver until it
        // either dispatches, times out, or is canceled. That keeps multi-key handling
        // from being stolen by global shortcuts or extension-local hooks mid-sequence.
        if state.mode == Mode::Normal && !self.sequences.resolver.has_pending() {
            match self.handle_extension_input(state, AppInputEvent::Key(key)) {
                InputHookResult::Ignored => {}
                InputHookResult::Consumed => {
                    return KeyEventOutcome {
                        redraw: true,
                        commands: Vec::new(),
                    };
                }
                InputHookResult::EmitCommand(ext_command) => {
                    return KeyEventOutcome {
                        redraw: false,
                        commands: vec![CommandRequest::new(
                            ext_command,
                            CommandInvocationSource::Binding,
                        )],
                    };
                }
            }
        }

        let extensions = self.extensions.host.ui_snapshot();
        let ctx = self.key_binding_context(state, &extensions);
        let resolution = self.sequences.resolver.handle_key_in_context(ctx, key);
        Self::sequence_outcome(resolution, CommandInvocationSource::Binding, false)
    }

    fn key_binding_context<'a>(
        &self,
        state: &AppState,
        extensions: &'a ExtensionUiSnapshot,
    ) -> KeyBindingContext<'a> {
        let active_palette = self.palette.manager.active_kind();
        let palette_input_empty = self.palette.manager.active_input_is_empty();

        KeyBindingContext {
            runtime: RuntimeConditionContext::with_palette_input_empty(
                state.mode,
                active_palette,
                palette_input_empty,
                extensions,
            ),
        }
    }

    pub(crate) fn start_extension_workers(
        &mut self,
        event_tx: tokio::sync::mpsc::UnboundedSender<ExtensionWorkerEvent>,
    ) {
        self.extensions.host.start_workers(event_tx);
    }

    pub(crate) fn handle_extension_worker_events(
        &mut self,
        state: &mut AppState,
        events: Vec<ExtensionWorkerEvent>,
    ) -> bool {
        self.extensions
            .host
            .handle_worker_events(events, state)
            .changed
    }

    pub(crate) fn prepare_extensions_for_document(&mut self, pdf: SharedPdfBackend) {
        self.extensions.host.on_document_opened(pdf);
    }

    pub(crate) fn reset_extensions_for_document_reload(
        &mut self,
        state: &mut AppState,
        pdf: SharedPdfBackend,
    ) {
        self.extensions.host.on_document_reloaded(state, pdf);
    }

    pub(crate) fn sync_extensions_after_page_change(
        &mut self,
        pdf: SharedPdfBackend,
        visible_pages: [Option<usize>; 2],
    ) {
        self.extensions
            .host
            .on_visible_pages_changed(pdf, visible_pages);
    }

    pub(crate) fn palette_view(&self) -> Option<PaletteView> {
        self.palette.manager.view()
    }

    pub(crate) fn pending_sequence_status(&self) -> Option<String> {
        self.sequences
            .resolver
            .pending_display()
            .map(|pending| format!("keys {pending}"))
    }

    pub(crate) fn flush_sequence_timeout(&mut self, state: &AppState) -> KeyEventOutcome {
        let extensions = self.extensions.host.ui_snapshot();
        let ctx = self.key_binding_context(state, &extensions);
        let resolution = self.sequences.resolver.flush_timeout(ctx);
        Self::sequence_outcome(resolution, CommandInvocationSource::Binding, true)
    }

    pub(crate) fn handle_extension_input(
        &mut self,
        state: &mut AppState,
        event: AppInputEvent,
    ) -> InputHookResult {
        self.extensions.host.handle_input(event, state)
    }

    pub(crate) fn apply_palette_requests(&mut self, state: &mut AppState) -> bool {
        let mut changed = false;
        while let Some(request) = self.palette.pending_requests.pop_front() {
            match request {
                PaletteRequest::Open { kind, payload } => {
                    let extensions = self.extensions.host.ui_snapshot();
                    match self.palette.manager.open(
                        &self.palette.registry,
                        state,
                        &extensions,
                        kind,
                        payload,
                        self.history.snapshot_for_palette(kind),
                    ) {
                        Ok(()) => {
                            changed |= state.mode != Mode::Palette;
                            state.mode = Mode::Palette;
                        }
                        Err(err) => {
                            state.set_error_notice(format!("failed to open palette: {err}"));
                        }
                    }
                }
                PaletteRequest::Close => {
                    if self.palette.manager.close() {
                        changed |= state.mode != Mode::Normal;
                        state.mode = Mode::Normal;
                    }
                }
            }
        }
        if changed {
            self.reconcile_sequences(state);
        }

        changed
    }

    pub(crate) fn dispatch_command(
        &mut self,
        state: &mut AppState,
        view_policy: ViewPolicy,
        request: CommandRequest,
        pdf: SharedPdfBackend,
    ) -> AppResult<CommandDispatchResult> {
        let result = dispatch_with_view_policy(
            state,
            view_policy,
            request.command,
            request.source,
            CommandDispatchContext {
                pdf,
                extension_host: &mut self.extensions.host,
                palette_registry: &self.palette.registry,
                palette_manager: &mut self.palette.manager,
                palette_requests: &mut self.palette.pending_requests,
                input_history: &mut self.history,
            },
        );
        self.reconcile_sequences(state);
        result
    }

    pub(crate) fn handle_app_event(&mut self, state: &mut AppState, event: &AppEvent) {
        self.extensions.host.handle_event(event, state);
    }

    fn reconcile_sequences(&mut self, state: &AppState) {
        let extensions = self.extensions.host.ui_snapshot();
        let ctx = self.key_binding_context(state, &extensions);
        self.sequences.resolver.reconcile(ctx);
    }

    fn command_redraw(command: &Command, redraw_on_dispatch: bool) -> bool {
        redraw_on_dispatch || matches!(command, Command::OpenHelp)
    }

    fn sequence_outcome(
        resolution: SequenceResolution,
        source: CommandInvocationSource,
        redraw_on_dispatch: bool,
    ) -> KeyEventOutcome {
        match resolution {
            SequenceResolution::Noop => KeyEventOutcome::default(),
            SequenceResolution::Pending | SequenceResolution::Cleared => KeyEventOutcome {
                redraw: true,
                commands: Vec::new(),
            },
            SequenceResolution::DispatchThen {
                first,
                next,
                redraw,
            } => {
                // `DispatchThen` represents "commit the pending sequence, then keep
                // processing the latest key" without dropping input.
                let first_redraw = Self::command_redraw(&first, redraw_on_dispatch);
                let mut outcome = Self::sequence_outcome(*next, source, redraw_on_dispatch);
                outcome.redraw |= redraw || first_redraw;
                outcome
                    .commands
                    .insert(0, CommandRequest::new(first, source));
                outcome
            }
            SequenceResolution::Dispatch(command) => {
                let redraw = Self::command_redraw(&command, redraw_on_dispatch);
                KeyEventOutcome {
                    redraw,
                    commands: vec![CommandRequest::new(command, source)],
                }
            }
            SequenceResolution::DispatchWithRedraw(command) => KeyEventOutcome {
                redraw: true,
                commands: vec![CommandRequest::new(command, source)],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::{AppState, Mode, PaletteRequest};
    use crate::backend::test_support::{build_pdf, unique_temp_path};
    use crate::backend::{PdfDoc, SharedPdfBackend};
    use crate::command::{Command, CommandInvocationSource, CommandRequest};
    use crate::condition::ConditionExpr;
    use crate::config::ViewPolicy;
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;
    use crate::palette::PaletteKind;

    use super::super::actors::InputActor;
    use super::super::core::InteractionSubsystem;

    fn test_pdf_backend() -> SharedPdfBackend {
        let file = unique_temp_path(".pdf");
        fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");
        fs::remove_file(&file).expect("test pdf should be removed");
        Arc::new(doc)
    }

    #[test]
    fn quit_key_dispatches_quit_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            )
            .expect("quit key should be handled");

        assert_eq!(
            outcome.commands,
            vec![CommandRequest::new(
                Command::Quit,
                CommandInvocationSource::Binding,
            )]
        );
        assert!(!outcome.redraw);
    }

    #[test]
    fn help_key_requests_open_help_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            )
            .expect("help key should be handled");

        assert_eq!(state.mode, crate::app::Mode::Normal);
        assert!(matches!(
            outcome.commands.as_slice(),
            [request]
                if request.command == Command::OpenHelp
                    && request.source == CommandInvocationSource::Binding
        ));
        assert!(outcome.redraw);
    }

    #[test]
    fn help_mode_scroll_keys_request_help_scroll_commands() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState {
            mode: crate::app::Mode::Help,
            ..AppState::default()
        };

        let down = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            )
            .expect("help scroll should be handled");
        assert_eq!(state.help_scroll, 0);
        assert!(!down.redraw);
        assert_eq!(
            down.commands,
            vec![CommandRequest::new(
                Command::HelpScrollDown,
                CommandInvocationSource::Binding
            )]
        );

        let up = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
            )
            .expect("help scroll should be handled");
        assert_eq!(
            up.commands,
            vec![CommandRequest::new(
                Command::HelpScrollUp,
                CommandInvocationSource::Binding
            )]
        );

        let closed = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("help close should be handled");
        assert_eq!(state.mode, crate::app::Mode::Help);
        assert_eq!(
            closed.commands,
            vec![CommandRequest::new(
                Command::CloseHelp,
                CommandInvocationSource::Binding
            )]
        );
        assert!(!closed.redraw);
    }

    #[test]
    fn help_mode_ignores_modified_scroll_keys() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState {
            mode: crate::app::Mode::Help,
            ..AppState::default()
        };

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            )
            .expect("modified help key should be handled");

        assert!(!outcome.redraw);
        assert!(outcome.commands.is_empty());
    }

    #[test]
    fn help_mode_ignores_question_mark_key() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState {
            mode: crate::app::Mode::Help,
            ..AppState::default()
        };

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            )
            .expect("help key should be handled");

        assert_eq!(state.mode, crate::app::Mode::Help);
        assert_eq!(state.help_scroll, 0);
        assert!(!outcome.redraw);
        assert!(outcome.commands.is_empty());
    }

    #[test]
    fn help_escape_requests_close_help_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState {
            mode: Mode::Help,
            ..AppState::default()
        };
        let mut actor = InputActor::new(std::time::Instant::now());
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

        let effects = actor
            .handle_terminal_event(
                crossterm::event::Event::Key(key),
                &mut interaction,
                &mut state,
            )
            .expect("help close should be handled");
        let (commands, events, redraws) = effects.into_parts();

        assert_eq!(state.mode, Mode::Help);
        assert!(redraws.is_empty());
        assert_eq!(
            commands,
            vec![CommandRequest::new(
                Command::CloseHelp,
                CommandInvocationSource::Binding
            )]
        );
        assert!(events.is_empty());
    }

    #[test]
    fn palette_escape_requests_close_palette_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();
        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Command,
                payload: None,
            });
        assert!(interaction.apply_palette_requests(&mut state));
        assert_eq!(state.mode, Mode::Palette);

        let mut actor = InputActor::new(std::time::Instant::now());
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

        let effects = actor
            .handle_terminal_event(
                crossterm::event::Event::Key(key),
                &mut interaction,
                &mut state,
            )
            .expect("palette close should be handled");
        let (commands, events, redraws) = effects.into_parts();

        assert_eq!(state.mode, Mode::Palette);
        assert!(redraws.is_empty());
        assert_eq!(
            commands,
            vec![CommandRequest::new(
                Command::ClosePalette,
                CommandInvocationSource::Binding
            )]
        );
        assert!(events.is_empty());
    }

    #[test]
    fn palette_enter_requests_palette_submit_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();
        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Command,
                payload: None,
            });
        assert!(interaction.apply_palette_requests(&mut state));
        assert_eq!(state.mode, Mode::Palette);

        let mut actor = InputActor::new(std::time::Instant::now());
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        let effects = actor
            .handle_terminal_event(
                crossterm::event::Event::Key(key),
                &mut interaction,
                &mut state,
            )
            .expect("palette submit should be handled");
        let (commands, events, redraws) = effects.into_parts();

        assert_eq!(state.mode, Mode::Palette);
        assert!(redraws.is_empty());
        assert_eq!(
            commands,
            vec![CommandRequest::new(
                Command::PaletteSubmit,
                CommandInvocationSource::Binding
            )]
        );
        assert!(events.is_empty());
    }

    #[test]
    fn command_palette_up_requests_palette_input_history_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();
        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Command,
                payload: None,
            });
        assert!(interaction.apply_palette_requests(&mut state));

        let outcome = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .expect("palette key should be handled");

        assert_eq!(
            outcome.commands,
            vec![CommandRequest::new(
                Command::PaletteInputHistoryOlder,
                CommandInvocationSource::Binding
            )]
        );
    }

    #[test]
    fn non_history_palette_up_requests_selection_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();
        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Outline,
                payload: None,
            });
        assert!(interaction.apply_palette_requests(&mut state));

        let outcome = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .expect("palette key should be handled");

        assert_eq!(
            outcome.commands,
            vec![CommandRequest::new(
                Command::PaletteSelectPrev,
                CommandInvocationSource::Binding
            )]
        );
    }

    #[test]
    fn pending_sequence_reprocesses_enter_after_mismatch() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::key(KeyCode::Enter)],
                Command::NextPage,
            )
            .expect("enter binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        let first = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");
        assert!(first.redraw);
        assert!(first.commands.is_empty());
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys g")
        );

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            )
            .expect("Enter should be reprocessed after the pending sequence");
        assert_eq!(
            mismatch.commands,
            vec![
                CommandRequest::new(Command::FirstPage, CommandInvocationSource::Binding),
                CommandRequest::new(Command::NextPage, CommandInvocationSource::Binding),
            ]
        );
        assert!(mismatch.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_exact_mismatch_requests_redraw_even_when_commands_do_not() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x')],
                Command::DebugStatusHide,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x'), ShortcutKey::char('x')],
                Command::NextPage,
            )
            .expect("multi-key binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys x")
        );

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
            )
            .expect("mismatched key should commit the pending sequence");
        assert_eq!(
            mismatch.commands,
            vec![CommandRequest::new(
                Command::DebugStatusHide,
                CommandInvocationSource::Binding,
            )]
        );
        assert!(mismatch.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_exact_mismatch_does_not_reprocess_after_mode_change() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x')],
                Command::OpenHelp,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x'), ShortcutKey::char('x')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('j')],
                Command::NextPage,
            )
            .expect("single-key binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys x")
        );

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            )
            .expect("mismatched key should commit the pending sequence");
        assert_eq!(
            mismatch.commands,
            vec![CommandRequest::new(
                Command::OpenHelp,
                CommandInvocationSource::Binding,
            )]
        );
        assert!(mismatch.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_reprocesses_followup_key_after_mismatch() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('?')],
                Command::OpenHelp,
            )
            .expect("single-key binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            )
            .expect("mismatched key should be reprocessed");
        assert!(mismatch.redraw);
        assert_eq!(
            mismatch.commands,
            vec![CommandRequest::new(
                Command::OpenHelp,
                CommandInvocationSource::Binding,
            )]
        );
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_redraws_when_reprocessed_followup_dispatches() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('j')],
                Command::NextPage,
            )
            .expect("single-key binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            )
            .expect("mismatched key should be reprocessed");
        assert!(mismatch.redraw);
        assert_eq!(
            mismatch.commands,
            vec![CommandRequest::new(
                Command::NextPage,
                CommandInvocationSource::Binding,
            )]
        );
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_requests_redraw_when_unbound_followup_clears_it() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys g")
        );

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            )
            .expect("mismatched key should clear the sequence");

        assert!(mismatch.redraw);
        assert!(mismatch.commands.is_empty());
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn wake_flushes_timed_out_sequence() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let flushed = interaction.flush_sequence_timeout(&state);
        assert!(matches!(
            flushed.commands.as_slice(),
            [request]
                if request.command == Command::FirstPage
                    && request.source == CommandInvocationSource::Binding
        ));
        assert!(flushed.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn timeout_flush_uses_current_conditions_outside_normal_mode() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        state.mode = Mode::Help;
        let flushed = interaction.flush_sequence_timeout(&state);

        assert!(flushed.redraw);
        assert_eq!(
            flushed.commands,
            vec![CommandRequest::new(
                Command::FirstPage,
                CommandInvocationSource::Binding
            )]
        );
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn opening_palette_preserves_pending_sequences_that_remain_enabled() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Command,
                payload: None,
            });
        assert!(interaction.apply_palette_requests(&mut state));
        assert_eq!(state.mode, Mode::Palette);
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys g")
        );
    }

    #[test]
    fn batched_palette_open_and_close_preserves_enabled_pending_sequences() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Command,
                payload: None,
            });
        interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Close);

        assert!(interaction.apply_palette_requests(&mut state));
        assert_eq!(state.mode, Mode::Normal);
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys g")
        );
    }

    #[test]
    fn timed_out_sequence_returns_expired_command_before_processing_new_key() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('j')],
                Command::NextPage,
            )
            .expect("single-key binding should register");
        let mut interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let next = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            )
            .expect("next key should dispatch after queuing the expired command");

        assert_eq!(
            next.commands,
            vec![
                CommandRequest::new(Command::FirstPage, CommandInvocationSource::Binding,),
                CommandRequest::new(Command::NextPage, CommandInvocationSource::Binding,)
            ]
        );
    }

    #[test]
    fn timed_out_sequence_followed_by_quit_dispatches_expired_command_then_quit() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('q')],
                Command::Quit,
            )
            .expect("single-key binding should register");
        let mut interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let next = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            )
            .expect("next key should queue the expired command and quit command");

        assert_eq!(
            next.commands,
            vec![
                CommandRequest::new(Command::FirstPage, CommandInvocationSource::Binding,),
                CommandRequest::new(Command::Quit, CommandInvocationSource::Binding,)
            ]
        );
    }

    #[test]
    fn dispatch_command_preserves_pending_sequences_that_remain_enabled() {
        let pdf = test_pdf_backend();
        let mut registry = SequenceRegistry::new();
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut interaction = InteractionSubsystem::with_sequence_registry(registry);
        let mut state = AppState::default();

        interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys g")
        );

        interaction
            .dispatch_command(
                &mut state,
                ViewPolicy::default(),
                CommandRequest::new(Command::OpenHelp, CommandInvocationSource::Binding),
                pdf,
            )
            .expect("help command should dispatch");

        assert_eq!(state.mode, Mode::Help);
        assert_eq!(
            interaction.pending_sequence_status().as_deref(),
            Some("keys g")
        );
    }
}
