use std::time::Instant;

use crossterm::event::{Event, KeyEventKind};

use crate::command::CommandRequest;
use crate::error::AppResult;

use crate::app::App;
use crate::app::terminal_session::TerminalSurface;

pub(crate) struct InputEventOutcome {
    pub(crate) quit_requested: bool,
    pub(crate) command: Option<CommandRequest>,
    pub(crate) redraw_requested: bool,
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
                let outcome = self.interaction.handle_key_event(&mut self.state, key)?;
                if outcome.clear_terminal {
                    session.clear()?;
                }
                if outcome.redraw {
                    *needs_redraw = true;
                }
                Ok(InputEventOutcome {
                    quit_requested: outcome.quit_requested,
                    command: outcome.command,
                    redraw_requested: outcome.redraw,
                })
            }
            Event::Resize(_, _) => {
                *last_input_at = Instant::now();
                *needs_redraw = true;
                Ok(InputEventOutcome {
                    quit_requested: false,
                    command: None,
                    redraw_requested: true,
                })
            }
            _ => Ok(InputEventOutcome {
                quit_requested: false,
                command: None,
                redraw_requested: false,
            }),
        }
    }
}
