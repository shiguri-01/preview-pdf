use crossterm::event::KeyEvent;

use crate::backend::PdfBackend;
use crate::command::{ActionId, Command, CommandDispatchResult, dispatch, drain_background_events};
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
    pub command: Option<Command>,
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
            command: Some(command),
        })
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
        self.palette
            .manager
            .handle_key(&self.palette.registry, state, key)
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
                    match self
                        .palette
                        .manager
                        .open(&self.palette.registry, state, kind, seed)
                    {
                        Ok(()) => {
                            state.mode = Mode::Palette;
                            state.status.last_action_id = Some(ActionId::OpenPalette);
                            state.status.message = format!("palette opened: {}", kind.id());
                            changed = true;
                        }
                        Err(err) => {
                            state.status.last_action_id = Some(ActionId::OpenPalette);
                            state.status.message = format!("failed to open palette: {err}");
                        }
                    }
                }
                PaletteRequest::Close => {
                    if self.palette.manager.close() {
                        state.mode = Mode::Normal;
                        state.status.last_action_id = Some(ActionId::ClosePalette);
                        state.status.message = "palette closed".to_string();
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
        command: Command,
        pdf: &mut dyn PdfBackend,
    ) -> AppResult<CommandDispatchResult> {
        dispatch(
            state,
            command,
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
    ) -> AppResult<(bool, Option<Command>)> {
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
                pending_command = Some(command);
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
}
