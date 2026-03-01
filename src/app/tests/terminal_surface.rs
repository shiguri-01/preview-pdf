use std::convert::Infallible;
use std::io;

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Paragraph;

use super::super::terminal_session::TerminalSurface;

struct TestTerminalSurface {
    terminal: Terminal<TestBackend>,
}

impl TestTerminalSurface {
    fn new(width: u16, height: u16) -> io::Result<Self> {
        let terminal = infallible_to_io(Terminal::new(TestBackend::new(width, height)))?;
        Ok(Self { terminal })
    }
}

impl TerminalSurface for TestTerminalSurface {
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

fn infallible_to_io<T>(result: Result<T, Infallible>) -> io::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(err) => match err {},
    }
}

#[test]
fn terminal_surface_supports_size_clear_and_draw() {
    let mut session = TestTerminalSurface::new(80, 24).expect("test terminal should initialize");
    let size = session.size().expect("size should resolve");
    assert_eq!(size, Size::new(80, 24));

    session.clear().expect("clear should succeed");
    session
        .draw(|frame| {
            frame.render_widget(Paragraph::new("ok"), Rect::new(0, 0, 2, 1));
        })
        .expect("draw should succeed");
}
