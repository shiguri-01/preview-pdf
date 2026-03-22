use std::convert::Infallible;
use std::io;
use std::time::Instant;

use ratatui::backend::TestBackend;
use ratatui::layout::Size;
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc::UnboundedSender;

use crate::command::{Command, CommandInvocationSource, CommandRequest};
use crate::event::DomainEvent;
use crate::perf::PerfScenarioId;

use super::state::AppState;
use super::terminal_session::TerminalSurface;

pub(crate) const PERF_HEADLESS_WIDTH: u16 = 120;
pub(crate) const PERF_HEADLESS_HEIGHT: u16 = 40;

pub(crate) struct HeadlessTerminalSession {
    terminal: Terminal<TestBackend>,
}

impl HeadlessTerminalSession {
    pub(crate) fn new(width: u16, height: u16) -> io::Result<Self> {
        let terminal = infallible_to_io(Terminal::new(TestBackend::new(width, height)))?;
        Ok(Self { terminal })
    }

    pub(crate) fn restore(&mut self) -> io::Result<()> {
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

pub(crate) struct PerfLoopDriver {
    scenario: PerfScenarioId,
    command_count: usize,
    positioned_backward_start: bool,
    idle_started_at: Option<Instant>,
}

impl PerfLoopDriver {
    pub(crate) fn new(scenario: PerfScenarioId) -> Self {
        Self {
            scenario,
            command_count: 0,
            positioned_backward_start: false,
            idle_started_at: None,
        }
    }

    pub(crate) fn advance(
        &mut self,
        state: &AppState,
        page_count: usize,
        system_idle: bool,
        loop_event_tx: &UnboundedSender<DomainEvent>,
    ) -> bool {
        if !system_idle {
            self.idle_started_at = None;
            return false;
        }

        match self.scenario {
            PerfScenarioId::PageFlipForward => {
                let params = self.scenario.parameters();
                let last_page = page_count.saturating_sub(1);
                if state.current_page >= last_page || self.command_count >= params.page_flip_limit {
                    return true;
                }
                let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                    Command::NextPage,
                    CommandInvocationSource::Keymap,
                )));
                self.command_count += 1;
                false
            }
            PerfScenarioId::PageFlipBackward => {
                let params = self.scenario.parameters();
                let last_page = page_count.saturating_sub(1);
                if !self.positioned_backward_start {
                    if state.current_page < last_page {
                        let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                            Command::LastPage,
                            CommandInvocationSource::Keymap,
                        )));
                        self.positioned_backward_start = true;
                        return false;
                    }
                    self.positioned_backward_start = true;
                }

                if state.current_page == 0 || self.command_count >= params.page_flip_limit {
                    return true;
                }

                let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                    Command::PrevPage,
                    CommandInvocationSource::Keymap,
                )));
                self.command_count += 1;
                false
            }
            PerfScenarioId::IdlePendingRedraw => {
                let Some(started_at) = self.idle_started_at else {
                    self.idle_started_at = Some(Instant::now());
                    return false;
                };
                started_at.elapsed().as_millis()
                    >= u128::from(self.scenario.parameters().idle_duration_ms)
            }
        }
    }
}

fn infallible_to_io<T>(result: Result<T, Infallible>) -> io::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(err) => match err {},
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Rect, Size};
    use ratatui::widgets::Paragraph;

    use super::{HeadlessTerminalSession, TerminalSurface};

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
}
