use std::convert::Infallible;
use std::io::{self, Stdout};

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::Size;
use ratatui::{Frame, Terminal};

use crate::error::AppResult;

pub(crate) trait TerminalSurface {
    fn size(&self) -> io::Result<Size>;

    fn clear(&mut self) -> io::Result<()>;

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>);
}

pub(crate) enum TerminalSession {
    Interactive(InteractiveTerminalSession),
    Headless(HeadlessTerminalSession),
}

impl TerminalSession {
    pub(crate) fn enter() -> AppResult<Self> {
        Ok(Self::Interactive(InteractiveTerminalSession::enter()?))
    }

    pub(crate) fn headless(width: u16, height: u16) -> AppResult<Self> {
        Ok(Self::Headless(HeadlessTerminalSession::new(width, height)?))
    }

    pub(crate) fn restore(&mut self) -> io::Result<()> {
        match self {
            Self::Interactive(session) => session.restore(),
            Self::Headless(session) => session.restore(),
        }
    }
}

impl TerminalSurface for TerminalSession {
    fn size(&self) -> io::Result<Size> {
        match self {
            Self::Interactive(session) => session.size(),
            Self::Headless(session) => session.size(),
        }
    }

    fn clear(&mut self) -> io::Result<()> {
        match self {
            Self::Interactive(session) => session.clear(),
            Self::Headless(session) => session.clear(),
        }
    }

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        match self {
            Self::Interactive(session) => session.draw(render),
            Self::Headless(session) => session.draw(render),
        }
    }
}

pub(crate) struct InteractiveTerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

impl InteractiveTerminalSession {
    fn enter() -> AppResult<Self> {
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

    fn restore(&mut self) -> io::Result<()> {
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

impl Drop for InteractiveTerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub(crate) struct HeadlessTerminalSession {
    terminal: Terminal<TestBackend>,
}

impl HeadlessTerminalSession {
    fn new(width: u16, height: u16) -> io::Result<Self> {
        let terminal = infallible_to_io(Terminal::new(TestBackend::new(width, height)))?;
        Ok(Self { terminal })
    }

    fn restore(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl TerminalSurface for HeadlessTerminalSession {
    fn size(&self) -> io::Result<Size> {
        infallible_to_io(self.terminal.size())
    }

    fn clear(&mut self) -> io::Result<()> {
        infallible_to_io(self.terminal.clear())
    }

    fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        infallible_to_io(self.terminal.draw(render)).map(|_| ())
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

fn infallible_to_io<T>(result: Result<T, Infallible>) -> io::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(err) => match err {},
    }
}
