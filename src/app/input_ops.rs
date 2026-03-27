use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::backend::SharedPdfBackend;
use crate::command::{
    Command, CommandDispatchResult, CommandInvocationSource, CommandRequest, dispatch,
    drain_background_events,
};
use crate::error::AppResult;
use crate::event::AppEvent;
use crate::input::keymap::{KeymapPreset, map_key_to_command_with_preset};
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
    pub command: Option<CommandRequest>,
}

impl InteractionSubsystem {
    pub(crate) fn handle_key_event(
        &mut self,
        state: &mut AppState,
        key: KeyEvent,
        keymap_preset: &str,
    ) -> AppResult<KeyEventOutcome> {
        if state.mode == Mode::Palette {
            return match self.handle_palette_key(state, key)? {
                PaletteKeyResult::Consumed { redraw } => Ok(KeyEventOutcome {
                    redraw,
                    clear_terminal: false,
                    quit_requested: false,
                    command: None,
                }),
                PaletteKeyResult::CloseRequested { session_id } => {
                    let closed = self.close_palette_session(state, session_id);
                    Ok(KeyEventOutcome {
                        redraw: closed,
                        clear_terminal: closed,
                        quit_requested: false,
                        command: None,
                    })
                }
                PaletteKeyResult::Submit(action) => {
                    let (changed_by_palette, command) =
                        self.handle_palette_submit_effect(state, action.session_id, action.effect)?;
                    Ok(KeyEventOutcome {
                        redraw: changed_by_palette,
                        clear_terminal: changed_by_palette,
                        quit_requested: false,
                        command,
                    })
                }
            };
        }

        if state.mode == Mode::Help {
            return Ok(self.handle_help_key_event(state, key));
        }

        if matches!(key.code, KeyCode::Char('?'))
            || (matches!(key.code, KeyCode::Char('/'))
                && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            state.mode = Mode::Help;
            state.reset_help_scroll();
            return Ok(KeyEventOutcome {
                redraw: true,
                clear_terminal: true,
                quit_requested: false,
                command: None,
            });
        }

        let mut command = None;
        match self.handle_extension_input(state, AppInputEvent::Key(key)) {
            InputHookResult::Ignored => {}
            InputHookResult::Consumed => {
                return Ok(KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    command: None,
                });
            }
            InputHookResult::EmitCommand(ext_command) => {
                command = Some(ext_command);
            }
        }

        if command.is_none() {
            let preset = KeymapPreset::parse(keymap_preset);
            command = map_key_to_command_with_preset(key, state.mode, preset);
        }

        let Some(command) = command else {
            return Ok(KeyEventOutcome::default());
        };

        if matches!(command, Command::Quit) {
            return Ok(KeyEventOutcome {
                redraw: false,
                clear_terminal: false,
                quit_requested: true,
                command: None,
            });
        }

        Ok(KeyEventOutcome {
            redraw: false,
            clear_terminal: false,
            quit_requested: false,
            command: Some(CommandRequest::new(
                command,
                CommandInvocationSource::Keymap,
            )),
        })
    }

    fn handle_help_key_event(&mut self, state: &mut AppState, key: KeyEvent) -> KeyEventOutcome {
        const HELP_PAGE_STEP: isize = 10;

        if matches!(key.code, KeyCode::Char('?'))
            || (matches!(key.code, KeyCode::Char('/'))
                && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            state.mode = Mode::Normal;
            state.reset_help_scroll();
            return KeyEventOutcome {
                redraw: true,
                clear_terminal: true,
                quit_requested: false,
                command: None,
            };
        }

        match key.code {
            KeyCode::Esc => {
                state.mode = Mode::Normal;
                state.reset_help_scroll();
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: true,
                    quit_requested: false,
                    command: None,
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                state.scroll_help_by(1);
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    command: None,
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.scroll_help_by(-1);
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    command: None,
                }
            }
            KeyCode::PageDown => {
                state.scroll_help_by(HELP_PAGE_STEP);
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    command: None,
                }
            }
            KeyCode::PageUp => {
                state.scroll_help_by(-HELP_PAGE_STEP);
                KeyEventOutcome {
                    redraw: true,
                    clear_terminal: false,
                    quit_requested: false,
                    command: None,
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
        state.mode = Mode::Normal;
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
                            state.mode = Mode::Palette;
                            changed = true;
                        }
                        Err(err) => {
                            state.set_error_notice(format!("failed to open palette: {err}"));
                        }
                    }
                }
                PaletteRequest::Close => {
                    if self.palette.manager.close() {
                        state.mode = Mode::Normal;
                        changed = true;
                    }
                }
            }
        }

        if !self.palette.manager.is_open() && state.mode == Mode::Palette {
            state.mode = Mode::Normal;
            changed = true;
        }
        changed
    }

    pub(crate) fn dispatch_command(
        &mut self,
        state: &mut AppState,
        request: CommandRequest,
        pdf: SharedPdfBackend,
    ) -> AppResult<CommandDispatchResult> {
        dispatch(
            state,
            request.command,
            request.source,
            pdf,
            &mut self.extensions.host,
            &mut self.palette.pending_requests,
        )
    }

    pub(crate) fn handle_app_event(&mut self, state: &mut AppState, event: &AppEvent) {
        self.extensions.host.handle_event(event, state);
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::AppState;

    use super::super::core::InteractionSubsystem;

    #[test]
    fn quit_key_requests_immediate_quit_without_command_requeue() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
                "default",
            )
            .expect("quit key should be handled");

        assert!(outcome.quit_requested);
        assert!(outcome.command.is_none());
        assert!(!outcome.redraw);
        assert!(!outcome.clear_terminal);
    }

    #[test]
    fn help_key_opens_help_mode_without_command_requeue() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();

        let outcome = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
                "default",
            )
            .expect("help key should be handled");

        assert_eq!(state.mode, crate::app::Mode::Help);
        assert!(outcome.command.is_none());
        assert!(outcome.redraw);
        assert!(outcome.clear_terminal);
    }

    #[test]
    fn help_mode_scrolls_and_closes() {
        let mut interaction = InteractionSubsystem::default();
        let mut state = AppState::default();
        state.mode = crate::app::Mode::Help;

        let down = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
                "default",
            )
            .expect("help scroll should be handled");
        assert_eq!(state.help_scroll, 1);
        assert!(down.redraw);
        assert!(!down.clear_terminal);

        let closed = interaction
            .handle_key_event(
                &mut state,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                "default",
            )
            .expect("help close should be handled");
        assert_eq!(state.mode, crate::app::Mode::Normal);
        assert_eq!(state.help_scroll, 0);
        assert!(closed.redraw);
        assert!(closed.clear_terminal);
    }
}
