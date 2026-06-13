use crossterm::event::{KeyCode, KeyEvent};

use crate::backend::SharedPdfBackend;
use crate::command::{
    Command, CommandDispatchResult, CommandInvocationSource, CommandRequest,
    dispatch_with_view_policy, drain_background_events,
};
use crate::config::ViewPolicy;
use crate::error::AppResult;
use crate::event::AppEvent;
use crate::input::sequence::SequenceResolution;
use crate::input::{AppInputEvent, InputHookResult};
use crate::palette::PaletteKeyResult;
use crate::palette::{PalettePostAction, PaletteSubmitEffect, PaletteView};

use super::core::InteractionSubsystem;
use super::state::{AppState, Mode, PaletteRequest, notice_action_for_error};

#[derive(Debug, Clone, Default)]
pub(crate) struct KeyEventOutcome {
    pub redraw: bool,
    pub quit_requested: bool,
    pub commands: Vec<CommandRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyEventRoute {
    Escape,
    Palette,
    Help,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpKeyAction {
    ScrollBy(isize),
    Ignore,
}

impl InteractionSubsystem {
    pub(crate) fn handle_key_event(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
    ) -> AppResult<KeyEventOutcome> {
        self.sync_sequences_with_mode(state);
        match Self::route_key_event(state, key) {
            KeyEventRoute::Escape => return Ok(self.handle_escape_key_event(state, key)),
            KeyEventRoute::Palette => return self.handle_palette_key_event(state, key),
            KeyEventRoute::Help => return Ok(self.handle_help_key_event(state, key)),
            KeyEventRoute::Normal => {}
        }

        Ok(self.handle_normal_key_event(state, key))
    }

    fn route_key_event(state: &AppState, key: KeyEvent) -> KeyEventRoute {
        if key.code == KeyCode::Esc {
            return KeyEventRoute::Escape;
        }
        match state.mode {
            Mode::Palette => KeyEventRoute::Palette,
            Mode::Help => KeyEventRoute::Help,
            Mode::Normal => KeyEventRoute::Normal,
        }
    }

    fn handle_escape_key_event(&mut self, state: &mut AppState, key: KeyEvent) -> KeyEventOutcome {
        if let Some(outcome) = self.close_active_overlay(state) {
            return outcome;
        }
        if self.sequences.resolver.has_pending() {
            let resolution = self.sequences.resolver.handle_key(key);
            return Self::sequence_outcome(resolution, false);
        }
        let search_active = self.extensions.host.ui_snapshot().search_active;
        KeyEventOutcome {
            redraw: search_active,
            quit_requested: false,
            commands: if search_active {
                vec![CommandRequest::new(
                    Command::CancelSearch,
                    CommandInvocationSource::Keymap,
                )]
            } else {
                Vec::new()
            },
        }
    }

    fn handle_palette_key_event(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
    ) -> AppResult<KeyEventOutcome> {
        let result = self.handle_palette_key(state, key)?;
        self.handle_palette_key_result(state, result)
    }

    fn handle_palette_key_result(
        &mut self,
        state: &mut AppState,
        result: PaletteKeyResult,
    ) -> AppResult<KeyEventOutcome> {
        match result {
            PaletteKeyResult::Consumed { redraw } => Ok(KeyEventOutcome {
                redraw,
                quit_requested: false,
                commands: Vec::new(),
            }),
            PaletteKeyResult::Submit(action) => {
                let (changed_by_palette, command) =
                    self.handle_palette_submit_effect(state, action.session_id, action.effect)?;
                Ok(KeyEventOutcome {
                    redraw: changed_by_palette,
                    quit_requested: false,
                    commands: command.into_iter().collect(),
                })
            }
            PaletteKeyResult::SubmitError(err) => {
                state.apply_notice_action(notice_action_for_error(err));
                Ok(KeyEventOutcome {
                    redraw: true,
                    quit_requested: false,
                    commands: Vec::new(),
                })
            }
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
                        quit_requested: false,
                        commands: Vec::new(),
                    };
                }
                InputHookResult::EmitCommand(ext_command) => {
                    return KeyEventOutcome {
                        redraw: false,
                        quit_requested: false,
                        commands: vec![CommandRequest::new(
                            ext_command,
                            CommandInvocationSource::Keymap,
                        )],
                    };
                }
            }
        }

        let resolution = self.sequences.resolver.handle_key(key);
        Self::sequence_outcome(resolution, false)
    }

    fn handle_help_key_event(&mut self, state: &mut AppState, key: KeyEvent) -> KeyEventOutcome {
        match Self::classify_help_key(key) {
            HelpKeyAction::ScrollBy(delta) => {
                state.scroll_help_by(delta);
                KeyEventOutcome {
                    redraw: true,
                    quit_requested: false,
                    commands: Vec::new(),
                }
            }
            HelpKeyAction::Ignore => KeyEventOutcome::default(),
        }
    }

    fn classify_help_key(key: KeyEvent) -> HelpKeyAction {
        match key.code {
            KeyCode::Char('j') => HelpKeyAction::ScrollBy(1),
            KeyCode::Char('k') => HelpKeyAction::ScrollBy(-1),
            _ => HelpKeyAction::Ignore,
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

    pub(crate) fn handle_palette_key(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
    ) -> AppResult<PaletteKeyResult> {
        let extensions = self.extensions.host.ui_snapshot();
        self.palette
            .manager
            .handle_key(&self.palette.registry, state, &extensions, key)
    }

    fn close_active_overlay(&mut self, state: &mut AppState) -> Option<KeyEventOutcome> {
        let closed = match state.mode {
            Mode::Palette => self.close_palette_overlay(state),
            Mode::Help => self.close_help_overlay(state),
            Mode::Normal => return None,
        };
        Some(KeyEventOutcome {
            redraw: closed,
            quit_requested: false,
            commands: Vec::new(),
        })
    }

    fn close_palette_overlay(&mut self, state: &mut AppState) -> bool {
        let changed = state.mode != Mode::Normal;
        let _ = self.palette.manager.close();
        if changed {
            state.mode = Mode::Normal;
            self.sync_sequences_with_mode(state);
        }
        changed
    }

    fn close_help_overlay(&mut self, state: &mut AppState) -> bool {
        if state.mode != Mode::Help {
            return false;
        }

        state.mode = Mode::Normal;
        state.reset_help_scroll();
        self.sync_sequences_with_mode(state);
        true
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
            pdf,
            &mut self.extensions.host,
            &mut self.palette.pending_requests,
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

    fn command_effects(command: &Command, redraw_on_dispatch: bool) -> (bool, bool) {
        (
            redraw_on_dispatch || matches!(command, Command::OpenHelp),
            matches!(command, Command::Quit),
        )
    }

    fn sequence_outcome(
        resolution: SequenceResolution,
        redraw_on_dispatch: bool,
    ) -> KeyEventOutcome {
        match resolution {
            SequenceResolution::Noop => KeyEventOutcome::default(),
            SequenceResolution::Pending | SequenceResolution::Cleared => KeyEventOutcome {
                redraw: true,
                quit_requested: false,
                commands: Vec::new(),
            },
            SequenceResolution::Dispatch(Command::Quit) => KeyEventOutcome {
                redraw: false,
                quit_requested: true,
                commands: Vec::new(),
            },
            SequenceResolution::DispatchThen { first, next } => {
                // `DispatchThen` represents "commit the timed-out sequence, then keep
                // processing the key that arrived after it" without dropping input.
                let (first_redraw, first_quit_requested) =
                    Self::command_effects(&first, redraw_on_dispatch);
                let mut outcome = Self::sequence_outcome(*next, redraw_on_dispatch);
                outcome.redraw |= first_redraw;
                outcome.quit_requested |= first_quit_requested;
                outcome.commands.insert(
                    0,
                    CommandRequest::new(first, CommandInvocationSource::Keymap),
                );
                outcome
            }
            SequenceResolution::Dispatch(command) => {
                let (redraw, _) = Self::command_effects(&command, redraw_on_dispatch);
                KeyEventOutcome {
                    redraw,
                    quit_requested: false,
                    commands: vec![CommandRequest::new(
                        command,
                        CommandInvocationSource::Keymap,
                    )],
                }
            }
        }
    }

    pub(crate) fn handle_palette_submit_effect(
        &mut self,
        state: &mut AppState,
        session_id: u64,
        effect: PaletteSubmitEffect,
    ) -> AppResult<(bool, Option<CommandRequest>)> {
        if !self.palette.manager.close_if_matches(session_id) {
            return Ok((false, None));
        }
        state.mode = Mode::Normal;
        self.sync_sequences_with_mode(state);
        let mut changed = true;
        let mut pending_command = None;

        match effect {
            PaletteSubmitEffect::Close => {}
            PaletteSubmitEffect::Reopen { kind, payload } => {
                self.palette
                    .pending_requests
                    .push_back(PaletteRequest::Open { kind, payload });
            }
            PaletteSubmitEffect::Dispatch {
                command,
                history_record,
                next,
            } => {
                if let Some(record) = history_record {
                    self.history.record(record);
                }
                pending_command = Some(CommandRequest::new(
                    command,
                    CommandInvocationSource::PaletteProvider,
                ));
                match next {
                    PalettePostAction::Close => {}
                    PalettePostAction::Reopen { kind, payload } => {
                        self.palette
                            .pending_requests
                            .push_back(PaletteRequest::Open { kind, payload });
                    }
                }
            }
        }

        if self.apply_palette_requests(state) {
            changed = true;
        }
        Ok((changed, pending_command))
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
    use crate::error::AppError;
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;
    use crate::palette::{PaletteKeyResult, PaletteKind};

    use super::super::actors::InputActor;
    use super::super::core::InteractionSubsystem;
    use super::super::state::{NoticeLevel, notice_action_for_error};

    fn test_pdf_backend() -> SharedPdfBackend {
        let file = unique_temp_path(".pdf");
        fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");
        fs::remove_file(&file).expect("test pdf should be removed");
        Arc::new(doc)
    }

    #[test]
    fn quit_key_requests_immediate_quit_without_command_requeue() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            )
            .expect("quit key should be handled");

        assert!(outcome.quit_requested);
        assert!(outcome.commands.is_empty());
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
    fn help_mode_scrolls_and_requests_close_help() {
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
        assert_eq!(state.help_scroll, 1);
        assert!(down.redraw);
        assert!(down.commands.is_empty());

        let closed = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("help close should be handled");
        assert_eq!(state.mode, crate::app::Mode::Normal);
        assert_eq!(state.help_scroll, 0);
        assert!(closed.commands.is_empty());
        assert!(closed.redraw);
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
    fn help_close_returns_to_normal_mode_and_requests_redraw() {
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
        let (commands, events, redraws, quit_requested) = effects.into_parts();

        assert_eq!(state.mode, Mode::Normal);
        assert_eq!(state.help_scroll, 0);
        assert!(!redraws.is_empty());
        assert!(commands.is_empty());
        assert!(events.is_empty());
        assert!(!quit_requested);
    }

    #[test]
    fn palette_close_returns_to_normal_mode_and_requests_redraw() {
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
        let (commands, events, redraws, quit_requested) = effects.into_parts();

        assert_eq!(state.mode, Mode::Normal);
        assert!(!redraws.is_empty());
        assert!(commands.is_empty());
        assert!(events.is_empty());
        assert!(!quit_requested);
    }

    #[test]
    fn palette_submit_returns_to_normal_mode_and_dispatches_command() {
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
        let (commands, events, redraws, quit_requested) = effects.into_parts();

        assert_eq!(state.mode, Mode::Normal);
        assert!(!redraws.is_empty());
        assert!(matches!(
            commands.as_slice(),
            [request]
                if request.command == Command::NextPage
                    && request.source == CommandInvocationSource::PaletteProvider
        ));
        assert!(events.is_empty());
        assert!(!quit_requested);
    }

    #[test]
    fn palette_close_repairs_stale_palette_mode_even_without_active_session() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState {
            mode: Mode::Palette,
            ..AppState::default()
        };

        let outcome = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("stale palette close should be handled");

        assert_eq!(state.mode, Mode::Normal);
        assert!(outcome.redraw);
        assert!(outcome.commands.is_empty());
    }

    #[test]
    fn palette_submit_error_applies_notice_without_dispatching_command() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();
        let expected = notice_action_for_error(AppError::invalid_argument("bad palette input"));

        let outcome = interaction
            .handle_palette_key_result(
                &mut state,
                PaletteKeyResult::SubmitError(AppError::invalid_argument("bad palette input")),
            )
            .expect("palette error should be converted to a notice");

        assert_eq!(
            state.notice,
            match expected {
                super::super::state::NoticeAction::Show { level, message } =>
                    Some(super::super::state::Notice { level, message }),
                super::super::state::NoticeAction::Keep
                | super::super::state::NoticeAction::Clear => None,
            }
        );
        assert_eq!(
            state.notice.as_ref().map(|notice| notice.level),
            Some(NoticeLevel::Warning)
        );
        assert!(outcome.redraw);
        assert!(!outcome.quit_requested);
        assert!(outcome.commands.is_empty());
    }

    #[test]
    fn pending_sequence_waits_for_confirmation() {
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

        let confirm = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            )
            .expect("Enter should confirm the pending sequence");
        assert!(matches!(
            confirm.commands.as_slice(),
            [request]
                if request.command == Command::FirstPage
                    && request.source == CommandInvocationSource::Keymap
        ));
        assert_eq!(interaction.pending_sequence_status(), None);
    }

    #[test]
    fn pending_sequence_prevents_help_toggle_from_stealing_followup_key() {
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

        let mismatch = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
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
        assert!(!flushed.quit_requested);
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
    fn timed_out_sequence_followed_by_quit_sets_quit_requested() {
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
            .expect("next key should queue the expired command and request quit");

        assert!(next.quit_requested);
        assert_eq!(
            next.commands,
            vec![CommandRequest::new(
                Command::FirstPage,
                CommandInvocationSource::Keymap,
            )]
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
