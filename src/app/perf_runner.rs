use std::convert::Infallible;
use std::io;
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::layout::Size;
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc::UnboundedSender;

use crate::command::{Command, CommandInvocationSource, CommandRequest};
use crate::event::DomainEvent;
use crate::perf::{PerfScenarioId, PerfScenarioParameters};

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
    parameters: PerfScenarioParameters,
    command_count: usize,
    positioned_backward_start: bool,
    rapid_commands_sent: bool,
    initial_idle_seen: bool,
    zoomed_in: bool,
    zoomed_out: bool,
    idle_started_at: Option<Instant>,
    measured_started_at: Option<Instant>,
}

impl PerfLoopDriver {
    pub(crate) fn new(
        scenario: PerfScenarioId,
        parameters: PerfScenarioParameters,
        cold_started_at: Instant,
    ) -> Self {
        Self {
            scenario,
            parameters,
            command_count: 0,
            positioned_backward_start: false,
            rapid_commands_sent: false,
            initial_idle_seen: false,
            zoomed_in: false,
            zoomed_out: false,
            idle_started_at: None,
            measured_started_at: match scenario {
                PerfScenarioId::ColdFirstPage => Some(cold_started_at),
                _ => None,
            },
        }
    }

    pub(crate) fn visited_steps(&self) -> usize {
        self.command_count
    }

    pub(crate) fn measured_elapsed(&self) -> Duration {
        self.measured_started_at
            .map(|started_at| started_at.elapsed())
            .unwrap_or_default()
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
            PerfScenarioId::ColdFirstPage => true,
            PerfScenarioId::SteadyNextPage => {
                let last_page = page_count.saturating_sub(1);
                if state.current_page >= last_page
                    || self.command_count >= self.parameters.page_steps
                {
                    self.start_measured_window();
                    return true;
                }
                self.start_measured_window();
                let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                    Command::NextPage,
                    CommandInvocationSource::Keymap,
                )));
                self.command_count += 1;
                false
            }
            PerfScenarioId::SteadyPrevPage => {
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

                if state.current_page == 0 || self.command_count >= self.parameters.page_steps {
                    self.start_measured_window();
                    return true;
                }

                self.start_measured_window();
                let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                    Command::PrevPage,
                    CommandInvocationSource::Keymap,
                )));
                self.command_count += 1;
                false
            }
            PerfScenarioId::RapidNextPage => {
                if !self.initial_idle_seen {
                    self.initial_idle_seen = true;
                    self.start_measured_window();
                    let last_page = page_count.saturating_sub(1);
                    let steps = self
                        .parameters
                        .page_steps
                        .min(last_page.saturating_sub(state.current_page));
                    for _ in 0..steps {
                        let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                            Command::NextPage,
                            CommandInvocationSource::Keymap,
                        )));
                    }
                    self.command_count += steps;
                    self.rapid_commands_sent = true;
                    return steps == 0;
                }
                self.rapid_commands_sent
            }
            PerfScenarioId::ZoomStep => {
                if !self.initial_idle_seen {
                    self.initial_idle_seen = true;
                    self.start_measured_window();
                    let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                        Command::ZoomIn,
                        CommandInvocationSource::Keymap,
                    )));
                    self.command_count += 1;
                    self.zoomed_in = true;
                    return false;
                }
                if self.zoomed_in && !self.zoomed_out {
                    let _ = loop_event_tx.send(DomainEvent::Command(CommandRequest::new(
                        Command::ZoomOut,
                        CommandInvocationSource::Keymap,
                    )));
                    self.command_count += 1;
                    self.zoomed_out = true;
                    return false;
                }
                self.zoomed_out
            }
            PerfScenarioId::IdleSettledRedraw => {
                let Some(started_at) = self.idle_started_at else {
                    let started_at = Instant::now();
                    self.idle_started_at = Some(started_at);
                    self.measured_started_at = Some(started_at);
                    return false;
                };
                started_at.elapsed().as_millis() >= u128::from(self.parameters.idle_duration_ms)
            }
        }
    }

    fn start_measured_window(&mut self) {
        self.measured_started_at.get_or_insert_with(Instant::now);
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
    use tokio::sync::mpsc::unbounded_channel;

    use crate::perf::{PerfScenarioId, PerfScenarioParameters};

    use super::{HeadlessTerminalSession, PerfLoopDriver, TerminalSurface};

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

    #[test]
    fn non_cold_scenario_starts_measured_window_after_initial_idle() {
        let (tx, _rx) = unbounded_channel();
        let mut driver = PerfLoopDriver::new(
            PerfScenarioId::RapidNextPage,
            PerfScenarioParameters {
                page_steps: 2,
                idle_duration_ms: 0,
            },
            std::time::Instant::now(),
        );
        let state = crate::app::state::AppState::default();

        assert!(driver.measured_started_at.is_none());
        assert!(!driver.advance(&state, 3, false, &tx));
        assert!(driver.measured_started_at.is_none());

        assert!(!driver.advance(&state, 3, true, &tx));
        assert!(driver.measured_started_at.is_some());
        assert_eq!(driver.visited_steps(), 2);
    }

    #[test]
    fn steady_prev_starts_measured_window_after_last_page_positioning() {
        let (tx, _rx) = unbounded_channel();
        let mut driver = PerfLoopDriver::new(
            PerfScenarioId::SteadyPrevPage,
            PerfScenarioParameters {
                page_steps: 1,
                idle_duration_ms: 0,
            },
            std::time::Instant::now(),
        );
        let mut state = crate::app::state::AppState::default();

        assert!(!driver.advance(&state, 3, true, &tx));
        assert!(driver.measured_started_at.is_none());

        state.current_page = 2;
        assert!(!driver.advance(&state, 3, true, &tx));
        assert!(driver.measured_started_at.is_some());
        assert_eq!(driver.visited_steps(), 1);
    }
}
