use std::io::{self, Stdout};

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Size;
use ratatui::{Frame, Terminal};

use crate::error::{AppError, AppResult};

pub(crate) trait TerminalSurface {
    fn size(&self) -> io::Result<Size>;

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>);
}

pub(crate) struct InteractiveTerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

impl InteractiveTerminalSession {
    pub(crate) fn enter() -> AppResult<Self> {
        enable_raw_mode()
            .map_err(|source| AppError::io_with_context(source, "enabling terminal raw mode"))?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(AppError::io_with_context(
                err,
                "entering terminal alternate screen",
            ));
        }

        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(err) => {
                cleanup_terminal_enter_failure();
                return Err(AppError::io_with_context(
                    err,
                    "initializing terminal backend",
                ));
            }
        };

        Ok(Self {
            terminal,
            active: true,
        })
    }

    pub(crate) fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }

        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
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

impl Drop for InteractiveTerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn cleanup_terminal_enter_failure() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}
