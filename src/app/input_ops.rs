use crossterm::event::KeyEvent;

use crate::backend::SharedPdfBackend;
use crate::command::{
    Command, CommandDispatchContext, CommandDispatchResult, CommandInvocationSource,
    CommandRequest, dispatch_with_view_policy, drain_background_events,
};
use crate::config::ViewPolicy;
use crate::error::AppResult;
use crate::event::AppEvent;
use crate::input::sequence::{KeyBindingContext, KeyBindingScope, SequenceResolution};
use crate::input::{AppInputEvent, InputHookResult};
use crate::palette::{PaletteKind, PaletteView};

use super::core::InteractionSubsystem;
use super::state::{AppState, Mode, PaletteRequest};

#[derive(Debug, Clone, Default)]
pub(crate) struct KeyEventOutcome {
    pub redraw: bool,
    pub commands: Vec<CommandRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyEventRoute {
    Palette,
    Help,
    Normal,
}

impl InteractionSubsystem {
    pub(crate) fn handle_key_event(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
    ) -> AppResult<KeyEventOutcome> {
        match Self::route_key_event(state, key) {
            KeyEventRoute::Palette => return Ok(self.handle_scoped_key_event(state, key)),
            KeyEventRoute::Help => return Ok(self.handle_scoped_key_event(state, key)),
            KeyEventRoute::Normal => {}
        }

        Ok(self.handle_normal_key_event(state, key))
    }

    fn route_key_event(state: &AppState, _key: KeyEvent) -> KeyEventRoute {
        match state.mode {
            Mode::Palette => KeyEventRoute::Palette,
            Mode::Help => KeyEventRoute::Help,
            Mode::Normal => KeyEventRoute::Normal,
        }
    }

    fn handle_normal_key_event(&mut self, state: &mut AppState, key: KeyEvent) -> KeyEventOutcome {
        // Once a sequence has started, keep routing keys through the resolver until it
        // either dispatches, times out, or is canceled. That keeps multi-key handling
        // from being stolen by global shortcuts or extension-local hooks mid-sequence.
        if !self.sequences.resolver.has_pending() {
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
                            CommandInvocationSource::Keymap,
                        )],
                    };
                }
            }
        }

        let ctx = self.key_binding_context(state);
        let resolution = self.sequences.resolver.handle_key_in_context(ctx, key);
        Self::sequence_outcome(resolution, false)
    }

    fn handle_scoped_key_event(&mut self, state: &AppState, key: KeyEvent) -> KeyEventOutcome {
        let ctx = self.key_binding_context(state);
        let resolution = self.sequences.resolver.handle_key_in_context(ctx, key);
        Self::sequence_outcome(resolution, false)
    }

    fn key_binding_context(&self, state: &AppState) -> KeyBindingContext {
        let extensions = self.extensions.host.ui_snapshot();
        let scope = match state.mode {
            Mode::Normal => KeyBindingScope::Normal,
            Mode::Palette => KeyBindingScope::Palette,
            Mode::Help => KeyBindingScope::Help,
        };
        let active_palette = self.palette.manager.active_kind();

        KeyBindingContext {
            scope,
            search_active: extensions.search_active,
            focused_text_input: self.palette.manager.focused_text_input_available(),
            text_history_available: matches!(
                active_palette,
                Some(PaletteKind::Command | PaletteKind::Search)
            ),
        }
    }

    pub(crate) fn drain_background_events(&mut self, state: &mut AppState) -> bool {
        drain_background_events(state, &mut self.extensions.host)
    }

    pub(crate) fn prewarm_search_text(&mut self, pdf: SharedPdfBackend) {
        self.extensions.host.prewarm_search_text(pdf);
    }

    pub(crate) fn reset_extensions_for_document_reload(
        &mut self,
        state: &mut AppState,
        pdf: SharedPdfBackend,
    ) {
        self.extensions.host.reset_for_document_reload(state, pdf);
    }

    pub(crate) fn sync_search_after_page_change(
        &mut self,
        pdf: SharedPdfBackend,
        current_page: usize,
    ) {
        self.extensions.host.prewarm_search_text(pdf.clone());
        self.extensions
            .host
            .resolve_search_priority_geometry(pdf, [Some(current_page), None]);
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

    pub(crate) fn flush_sequence_timeout(&mut self, mode: Mode) -> KeyEventOutcome {
        if mode != Mode::Normal {
            self.sync_sequences_with_mode_value(mode);
            return KeyEventOutcome::default();
        }
        let resolution = self.sequences.resolver.flush_timeout();
        Self::sequence_outcome(resolution, true)
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
                            self.sync_sequences_with_mode(state);
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
                        self.sync_sequences_with_mode(state);
                    }
                }
            }
        }

        if !self.palette.manager.is_open() && state.mode == Mode::Palette {
            state.mode = Mode::Normal;
            changed = true;
            self.sync_sequences_with_mode(state);
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
        self.sync_sequences_with_mode(state);
        result
    }

    pub(crate) fn handle_app_event(&mut self, state: &mut AppState, event: &AppEvent) {
        self.extensions.host.handle_event(event, state);
    }

    fn sync_sequences_with_mode(&mut self, state: &AppState) {
        self.sync_sequences_with_mode_value(state.mode);
    }

    fn sync_sequences_with_mode_value(&mut self, mode: Mode) {
        if mode != Mode::Normal {
            self.sequences.resolver.clear();
        }
    }

    fn command_redraw(command: &Command, redraw_on_dispatch: bool) -> bool {
        redraw_on_dispatch || matches!(command, Command::OpenHelp)
    }

    fn sequence_outcome(
        resolution: SequenceResolution,
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
                let mut outcome = Self::sequence_outcome(*next, redraw_on_dispatch);
                outcome.redraw |= redraw || first_redraw;
                outcome.commands.insert(
                    0,
                    CommandRequest::new(first, CommandInvocationSource::Keymap),
                );
                outcome
            }
            SequenceResolution::Dispatch(command) => {
                let redraw = Self::command_redraw(&command, redraw_on_dispatch);
                KeyEventOutcome {
                    redraw,
                    commands: vec![CommandRequest::new(
                        command,
                        CommandInvocationSource::Keymap,
                    )],
                }
            }
            SequenceResolution::DispatchWithRedraw(command) => KeyEventOutcome {
                redraw: true,
                commands: vec![CommandRequest::new(
                    command,
                    CommandInvocationSource::Keymap,
                )],
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
                CommandInvocationSource::Keymap,
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
                    && request.source == CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
            )]
        );
        assert!(events.is_empty());
    }

    #[test]
    fn command_palette_up_requests_text_history_command() {
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
                Command::TextHistoryOlder,
                CommandInvocationSource::Keymap
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
                CommandInvocationSource::Keymap
            )]
        );
    }

    #[test]
    fn stale_palette_mode_escape_still_requests_close_palette_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState {
            mode: Mode::Palette,
            ..AppState::default()
        };

        let outcome = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("stale palette close should be handled");

        assert_eq!(state.mode, Mode::Palette);
        assert!(!outcome.redraw);
        assert_eq!(
            outcome.commands,
            vec![CommandRequest::new(
                Command::ClosePalette,
                CommandInvocationSource::Keymap
            )]
        );
    }

    #[test]
    fn pending_sequence_reprocesses_enter_after_mismatch() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::key(KeyCode::Enter)], Command::NextPage)
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
                CommandRequest::new(Command::FirstPage, CommandInvocationSource::Keymap),
                CommandRequest::new(Command::NextPage, CommandInvocationSource::Keymap),
            ]
        );
        assert!(mismatch.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_exact_mismatch_requests_redraw_even_when_commands_do_not() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('x')], Command::DebugStatusHide)
            .expect("single-key binding should register");
        registry
            .register_static(
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
                CommandInvocationSource::Keymap,
            )]
        );
        assert!(mismatch.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_exact_mismatch_does_not_reprocess_after_mode_change() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('x')], Command::OpenHelp)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('x'), ShortcutKey::char('x')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
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
                CommandInvocationSource::Keymap,
            )]
        );
        assert!(mismatch.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_reprocesses_followup_key_after_mismatch() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('?')], Command::OpenHelp)
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
                CommandInvocationSource::Keymap,
            )]
        );
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_redraws_when_reprocessed_followup_dispatches() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
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
                CommandInvocationSource::Keymap,
            )]
        );
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_requests_redraw_when_unbound_followup_clears_it() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
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
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
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

        let flushed = interaction.flush_sequence_timeout(state.mode);
        assert!(matches!(
            flushed.commands.as_slice(),
            [request]
                if request.command == Command::FirstPage
                    && request.source == CommandInvocationSource::Keymap
        ));
        assert!(flushed.redraw);
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn timeout_flush_ignores_pending_sequences_outside_normal_mode() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
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
        let flushed = interaction.flush_sequence_timeout(state.mode);

        assert!(!flushed.redraw);
        assert!(flushed.commands.is_empty());
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn opening_palette_clears_pending_sequences() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
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
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn batched_palette_open_and_close_clears_pending_sequences() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
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
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn timed_out_sequence_returns_expired_command_before_processing_new_key() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
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
                CommandRequest::new(Command::FirstPage, CommandInvocationSource::Keymap,),
                CommandRequest::new(Command::NextPage, CommandInvocationSource::Keymap,)
            ]
        );
    }

    #[test]
    fn timed_out_sequence_followed_by_quit_dispatches_expired_command_then_quit() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('q')], Command::Quit)
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
                CommandRequest::new(Command::FirstPage, CommandInvocationSource::Keymap,),
                CommandRequest::new(Command::Quit, CommandInvocationSource::Keymap,)
            ]
        );
    }

    #[test]
    fn dispatch_command_clears_pending_sequences_after_mode_change() {
        let pdf = test_pdf_backend();
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
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
                CommandRequest::new(Command::OpenHelp, CommandInvocationSource::Keymap),
                pdf,
            )
            .expect("help command should dispatch");

        assert_eq!(state.mode, Mode::Help);
        assert_eq!(interaction.pending_sequence_status(), None);
    }
}
