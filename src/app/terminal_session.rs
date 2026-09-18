use std::io;

use ratatui::layout::Size;
use ratatui::{DefaultTerminal, Frame};

use crate::error::{AppError, AppResult};

pub(crate) trait TerminalSurface {
    fn size(&self) -> io::Result<Size>;

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>);
}

pub(crate) trait TerminalSession: TerminalSurface {
    fn restore(&mut self) -> io::Result<()>;
}

pub(crate) struct InteractiveTerminalSession {
    terminal: DefaultTerminal,
    active: bool,
}

impl InteractiveTerminalSession {
    pub(crate) fn enter() -> AppResult<Self> {
        let terminal = ratatui::try_init().map_err(|source| {
            // try_init installs the panic hook, but an ordinary error can leave
            // raw mode or the alternate screen enabled before a session exists.
            let _ = ratatui::try_restore();
            AppError::io_with_context(source, "initializing terminal session")
        })?;
        let mut session = Self {
            terminal,
            active: true,
        };
        session.terminal.clear().map_err(|source| {
            AppError::io_with_context(source, "clearing terminal alternate screen")
        })?;
        Ok(session)
    }

    pub(crate) fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }

        let restore_result = ratatui::try_restore();
        let cursor_result = self.terminal.show_cursor();
        restore_result.and(cursor_result)?;
        self.active = false;
        Ok(())
    }
}

impl TerminalSurface for InteractiveTerminalSession {
    fn size(&self) -> io::Result<Size> {
        self.terminal.size()
    }

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.terminal.draw(render).map(|_| ())
    }
}

impl TerminalSession for InteractiveTerminalSession {
    fn restore(&mut self) -> io::Result<()> {
        InteractiveTerminalSession::restore(self)
    }
}

impl Drop for InteractiveTerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
