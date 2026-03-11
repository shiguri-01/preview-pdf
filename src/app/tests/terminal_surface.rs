use ratatui::layout::{Rect, Size};
use ratatui::widgets::Paragraph;

use super::super::perf_runner::HeadlessTerminalSession;
use super::super::terminal_session::TerminalSurface;

#[test]
fn headless_terminal_session_supports_terminal_surface_contract() {
    let mut session =
        HeadlessTerminalSession::new(80, 24).expect("headless terminal should initialize");
    let size = session.size().expect("size should resolve");
    assert_eq!(size, Size::new(80, 24));

    session.clear().expect("clear should succeed");
    session
        .draw(|frame| {
            frame.render_widget(Paragraph::new("ok"), Rect::new(0, 0, 2, 1));
        })
        .expect("draw should succeed");
}
