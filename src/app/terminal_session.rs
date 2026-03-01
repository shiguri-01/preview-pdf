use std::io::{self, Stdout};

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Size;

use crate::error::AppResult;

pub(crate) trait TerminalSurface {
    fn size(&self) -> io::Result<Size>;

    fn clear(&mut self) -> io::Result<()>;

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>);
}

pub(crate) struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

impl TerminalSession {
    pub(crate) fn enter() -> AppResult<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err.into());
        }

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(err) => {
                cleanup_terminal_enter_failure(None);
                return Err(err.into());
            }
        };
        if let Err(err) = terminal.clear() {
            cleanup_terminal_enter_failure(Some(&mut terminal));
            return Err(err.into());
        }

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

impl TerminalSurface for TerminalSession {
    fn size(&self) -> io::Result<Size> {
        self.terminal.size()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear()
    }

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.terminal.draw(render).map(|_| ())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn cleanup_terminal_enter_failure(terminal: Option<&mut Terminal<CrosstermBackend<Stdout>>>) {
    match terminal {
        Some(terminal) => {
            let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
        }
        None => {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen);
        }
    }

    let _ = disable_raw_mode();
}
