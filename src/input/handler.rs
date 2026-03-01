use std::time::Instant;

use crossterm::event::{Event, KeyEventKind};

use crate::command::Command;
use crate::error::AppResult;

use crate::app::App;
use crate::app::terminal_session::TerminalSurface;

pub(crate) struct InputEventOutcome {
    pub(crate) quit_requested: bool,
    pub(crate) command: Option<Command>,
}

impl App {
    pub(crate) fn handle_input_event(
        &mut self,
        event: Event,
        session: &mut impl TerminalSurface,
        needs_redraw: &mut bool,
        last_input_at: &mut Instant,
    ) -> AppResult<InputEventOutcome> {
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                *last_input_at = Instant::now();
                let outcome = self.interaction.handle_key_event(
                    &mut self.state,
                    key,
                    &self.config.keymap.preset,
                )?;
                if outcome.clear_terminal {
                    session.clear()?;
                }
                if outcome.redraw {
                    *needs_redraw = true;
                }
                Ok(InputEventOutcome {
                    quit_requested: outcome.quit_requested,
                    command: outcome.command,
                })
            }
            Event::Resize(_, _) => {
                *last_input_at = Instant::now();
                *needs_redraw = true;
                Ok(InputEventOutcome {
                    quit_requested: false,
                    command: None,
                })
            }
            _ => Ok(InputEventOutcome {
                quit_requested: false,
                command: None,
            }),
        }
    }
}
