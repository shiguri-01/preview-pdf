use super::super::terminal_session::{TerminalSession, TerminalSurface};

pub(in crate::app) struct RestoringSession<S: TerminalSession> {
    session: S,
    active: bool,
}

impl<S: TerminalSession> RestoringSession<S> {
    pub(in crate::app) fn new(session: S) -> Self {
        Self {
            session,
            active: true,
        }
    }
}

impl<S: TerminalSession> TerminalSurface for RestoringSession<S> {
    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        self.session.size()
    }

    fn draw<F>(&mut self, render: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.session.draw(render)
    }
}

impl<S: TerminalSession> TerminalSession for RestoringSession<S> {
    fn restore(&mut self) -> std::io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.session.restore()?;
        self.active = false;
        Ok(())
    }
}

impl<S: TerminalSession> Drop for RestoringSession<S> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
