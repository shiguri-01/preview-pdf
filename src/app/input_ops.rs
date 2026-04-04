use crossterm::event::{KeyCode, KeyEvent};

use crate::backend::SharedPdfBackend;
use crate::command::{
    Command, CommandDispatchResult, CommandInvocationSource, CommandRequest, dispatch,
    drain_background_events,
};
use crate::error::AppResult;
use crate::event::AppEvent;
use crate::input::keymap::map_help_mode_key;
use crate::input::sequence::SequenceResolution;
use crate::input::{AppInputEvent, InputHookResult};
use crate::palette::PaletteKeyResult;
use crate::palette::{PalettePostAction, PaletteSubmitEffect, PaletteView};

use super::core::InteractionSubsystem;
use super::state::{AppState, Mode, PaletteRequest};

#[derive(Debug, Clone, Default)]
pub(crate) struct KeyEventOutcome {
    pub redraw: bool,
    pub clear_terminal: bool,
    pub quit_requested: bool,
    pub commands: Vec<CommandRequest>,
}

impl InteractionSubsystem {
    pub(crate) fn handle_key_event(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
    ) -> AppResult<KeyEventOutcome> {
        self.sync_sequences_with_mode(state);
        if state.mode == Mode::Palette {
            return match self.handle_palette_key(state, key)? {
                PaletteKeyResult::Consumed { redraw } => Ok(KeyEventOutcome {
                    redraw,
                    clear_terminal: false,
                    quit_requested: false,
                    commands: Vec::new(),
                }),
                PaletteKeyResult::CloseRequested { session_id } => {
                    let closed = self.close_palette_session(state, session_id);
                    Ok(KeyEventOutcome {
                        redraw: closed,
                        clear_terminal: closed,
                        quit_requested: false,
                        commands: Vec::new(),
                    })
                }
                PaletteKeyResult::Submit(action) => {
                    let (changed_by_palette, command) =
                        self.handle_palette_submit_effect(state, action.session_id, action.effect)?;
                    Ok(KeyEventOutcome {
                        redraw: changed_by_palette,
                        clear_terminal: changed_by_palette,
                        quit_requested: false,
                        commands: command.into_iter().collect(),
                    })
                }
            };
        }

        if state.mode == Mode::Help {
            return Ok(self.handle_help_key_event(state, key));
        }

        // Once a sequence has started, keep routing keys through the resolver until it
        // either dispatches, times out, or is canceled. That keeps multi-key handling
        // from being stolen by global shortcuts or extension-local hooks mid-sequence.
        if !self.sequences.resolver.has_pending() {
            match self.handle_extension_input(state, AppInputEvent::Key(key)) {
                InputHookResult::Ignored => {}
                InputHookResult::Consumed => {
                    return Ok(KeyEventOutcome {
                        redraw: true,
                        clear_terminal: false,
                        quit_requested: false,
                        commands: Vec::new(),
                    });
                }
                InputHookResult::EmitCommand(ext_command) => {
                    return Ok(KeyEventOutcome {
                        redraw: false,
                        clear_terminal: false,
                        quit_requested: false,
                        commands: vec![CommandRequest::new(
                            ext_command,
                            CommandInvocationSource::Keymap,
                        )],
                    });
                }
            }
        }

        let resolution = self.sequences.resolver.handle_key(key);
        Ok(Self::sequence_outcome(resolution, false))
    }

    fn handle_help_key_event(&mut self, state: &mut AppState, key: KeyEvent) -> KeyEventOutcome {
        if let Some(Command::CloseHelp) = map_help_mode_key(key) {
            let closed = self.close_help_session(state);
            return KeyEventOutcome {
                redraw: closed,
                clear_terminal: closed,
                quit_requested: false,
                commands: vec![CommandRequest::new(
                    Command::CloseHelp,
                    CommandInvocationSource::Keymap,
                )],
            };
        }

        match key.code {
            KeyCode::Char('j') => {
                state.scroll_help_by(1);
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    commands: Vec::new(),
                }
            }
            KeyCode::Char('k') => {
                state.scroll_help_by(-1);
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    commands: Vec::new(),
                }
            }
            _ => KeyEventOutcome::default(),
        }
    }

    pub(crate) fn drain_background_events(&mut self, state: &mut AppState) -> bool {
        drain_background_events(state, &mut self.extensions.host)
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

    pub(crate) fn close_palette_session(&mut self, state: &mut AppState, session_id: u64) -> bool {
        if !self.palette.manager.close_if_matches(session_id) {
            return false;
        }
        let changed = state.mode != Mode::Normal;
        state.mode = Mode::Normal;
        self.sync_sequences_with_mode(state);
        changed
    }

    pub(crate) fn close_help_session(&mut self, state: &mut AppState) -> bool {
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
                PaletteRequest::Open { kind, seed } => {
                    let extensions = self.extensions.host.ui_snapshot();
                    match self.palette.manager.open(
                        &self.palette.registry,
                        state,
                        &extensions,
                        kind,
                        seed,
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
        self.sync_sequences_with_mode(state);
        changed
    }

    pub(crate) fn dispatch_command(
        &mut self,
        state: &mut AppState,
        request: CommandRequest,
        pdf: SharedPdfBackend,
    ) -> AppResult<CommandDispatchResult> {
        let result = dispatch(
            state,
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

    fn command_effects(command: &Command, redraw_on_dispatch: bool) -> (bool, bool, bool) {
        (
            redraw_on_dispatch || matches!(command, Command::OpenHelp),
            matches!(command, Command::OpenHelp),
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
                clear_terminal: false,
                quit_requested: false,
                commands: Vec::new(),
            },
            SequenceResolution::Dispatch(Command::Quit) => KeyEventOutcome {
                redraw: false,
                clear_terminal: false,
                quit_requested: true,
                commands: Vec::new(),
            },
            SequenceResolution::DispatchThen { first, next } => {
                // `DispatchThen` represents "commit the timed-out sequence, then keep
                // processing the key that arrived after it" without dropping input.
                let (first_redraw, first_clear_terminal, first_quit_requested) =
                    Self::command_effects(&first, redraw_on_dispatch);
                let mut outcome = Self::sequence_outcome(*next, redraw_on_dispatch);
                outcome.redraw |= first_redraw;
                outcome.clear_terminal |= first_clear_terminal;
                outcome.quit_requested |= first_quit_requested;
                outcome.commands.insert(
                    0,
                    CommandRequest::new(first, CommandInvocationSource::Keymap),
                );
                outcome
            }
            SequenceResolution::Dispatch(command) => {
                let (redraw, clear_terminal, _) =
                    Self::command_effects(&command, redraw_on_dispatch);
                KeyEventOutcome {
                    redraw,
                    clear_terminal,
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
            PaletteSubmitEffect::Reopen { kind, seed } => {
                self.palette
                    .pending_requests
                    .push_back(PaletteRequest::Open { kind, seed });
            }
            PaletteSubmitEffect::Dispatch { command, next } => {
                pending_command = Some(CommandRequest::new(
                    command,
                    CommandInvocationSource::PaletteProvider,
                ));
                match next {
                    PalettePostAction::Close => {}
                    PalettePostAction::Reopen { kind, seed } => {
                        self.palette
                            .pending_requests
                            .push_back(PaletteRequest::Open { kind, seed });
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
    use std::io;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Size;

    use crate::app::terminal_session::TerminalSurface;
    use crate::app::{AppState, Mode, PaletteRequest};
    use crate::backend::{PdfDoc, SharedPdfBackend};
    use crate::command::{Command, CommandInvocationSource, CommandRequest};
    use crate::config::Config;
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;
    use crate::palette::PaletteKind;
    use crate::presenter::PresenterKind;

    use super::super::App;
    use super::super::core::InteractionSubsystem;

    struct MockSession {
        clear_count: usize,
        size: Size,
    }

    impl MockSession {
        fn new(width: u16, height: u16) -> Self {
            Self {
                clear_count: 0,
                size: Size::new(width, height),
            }
        }
    }

    impl TerminalSurface for MockSession {
        fn size(&self) -> io::Result<Size> {
            Ok(self.size)
        }

        fn clear(&mut self) -> io::Result<()> {
            self.clear_count += 1;
            Ok(())
        }

        fn draw<F>(&mut self, _render: F) -> io::Result<()>
        where
            F: FnOnce(&mut ratatui::Frame<'_>),
        {
            Ok(())
        }
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("pvf-input-ops-{nanos}{suffix}"))
    }

    fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
        let page_texts = if page_texts.is_empty() {
            vec!["".to_string()]
        } else {
            page_texts
                .iter()
                .map(|text| format!("BT /F1 14 Tf 36 260 Td ({text}) Tj ET"))
                .collect()
        };

        let page_count = page_texts.len();
        let page_ids: Vec<usize> = (0..page_count).map(|i| 4 + i * 2).collect();

        let mut objects = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!(
            "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
        ));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

        for (index, stream) in page_texts.iter().enumerate() {
            let content_id = 5 + index * 2;
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            ));
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = vec![0_usize];
        for (index, object) in objects.iter().enumerate() {
            let object_id = index + 1;
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{object_id} 0 obj\n{object}\nendobj\n").as_bytes());
        }

        let xref_start = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_start
            )
            .as_bytes(),
        );
        bytes
    }

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
        assert!(!outcome.clear_terminal);
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
        assert!(outcome.clear_terminal);
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
        assert!(!down.clear_terminal);
        assert!(down.commands.is_empty());

        let closed = interaction
            .handle_key_event(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .expect("help close should be handled");
        assert_eq!(state.mode, crate::app::Mode::Normal);
        assert_eq!(state.help_scroll, 0);
        assert!(matches!(
            closed.commands.as_slice(),
            [request]
                if request.command == Command::CloseHelp
                    && request.source == CommandInvocationSource::Keymap
        ));
        assert!(closed.redraw);
        assert!(closed.clear_terminal);
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
        assert!(!outcome.clear_terminal);
        assert!(outcome.commands.is_empty());
    }

    #[test]
    fn help_close_requests_viewer_area_clear() {
        let mut app = App::new_with_config(PresenterKind::RatatuiImage, Config::default())
            .expect("app should initialize");
        app.state.mode = Mode::Help;
        let mut session = MockSession::new(80, 24);
        let mut needs_redraw = false;
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let mut last_input_at = std::time::Instant::now();

        app.handle_input_event(
            crossterm::event::Event::Key(key),
            &mut session,
            &mut needs_redraw,
            &mut last_input_at,
        )
        .expect("help close should be handled");

        assert_eq!(session.clear_count, 1);
        assert_eq!(app.state.mode, Mode::Normal);
        assert_eq!(app.state.help_scroll, 0);
        assert!(needs_redraw);
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
        assert!(!flushed.clear_terminal);
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
                seed: None,
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
                seed: None,
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
                CommandRequest::new(Command::OpenHelp, CommandInvocationSource::Keymap),
                pdf,
            )
            .expect("help command should dispatch");

        assert_eq!(state.mode, Mode::Help);
        assert_eq!(interaction.pending_sequence_status(), None);
    }
}
