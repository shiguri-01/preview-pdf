use std::convert::Infallible;
use std::io;
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::layout::Size;
use ratatui::{Frame, Terminal};

use crate::app::{
    LoopDriver, LoopDriverDecision, LoopDriverHandle, LoopMetricsSnapshot, LoopObservation,
    TerminalSession, TerminalSurface, binding_request,
};
use crate::command::Command;
use crate::error::{AppError, AppResult};
use crate::perf::{PerfIterationSnapshot, PerfScenarioId, PerfScenarioParameters};

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

impl TerminalSession for HeadlessTerminalSession {
    fn restore(&mut self) -> io::Result<()> {
        HeadlessTerminalSession::restore(self)
    }
}

impl TerminalSurface for HeadlessTerminalSession {
    fn size(&self) -> io::Result<Size> {
        infallible_to_io(self.terminal.size())
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

    fn start_measured_window(&mut self) {
        self.measured_started_at.get_or_insert_with(Instant::now);
    }
}

impl LoopDriver for PerfLoopDriver {
    type Output = PerfIterationSnapshot;

    fn on_iteration(
        &mut self,
        observation: LoopObservation,
        handle: &mut LoopDriverHandle<'_>,
    ) -> AppResult<LoopDriverDecision> {
        if !observation.system_idle {
            self.idle_started_at = None;
            return Ok(LoopDriverDecision::Continue);
        }

        match self.scenario {
            PerfScenarioId::ColdFirstPage => Ok(LoopDriverDecision::Finish),
            PerfScenarioId::SteadyNextPage => {
                let last_page = observation.page_count.saturating_sub(1);
                if observation.current_page >= last_page
                    || self.command_count >= self.parameters.page_steps
                {
                    self.start_measured_window();
                    return Ok(LoopDriverDecision::Finish);
                }
                self.start_measured_window();
                handle.enqueue_command(binding_request(Command::NextPage))?;
                self.command_count += 1;
                Ok(LoopDriverDecision::Continue)
            }
            PerfScenarioId::SteadyPrevPage => {
                let last_page = observation.page_count.saturating_sub(1);
                if !self.positioned_backward_start {
                    if observation.current_page < last_page {
                        handle.enqueue_command(binding_request(Command::LastPage))?;
                        self.positioned_backward_start = true;
                        return Ok(LoopDriverDecision::Continue);
                    }
                    self.positioned_backward_start = true;
                }

                if observation.current_page == 0 || self.command_count >= self.parameters.page_steps
                {
                    self.start_measured_window();
                    return Ok(LoopDriverDecision::Finish);
                }

                self.start_measured_window();
                handle.enqueue_command(binding_request(Command::PrevPage))?;
                self.command_count += 1;
                Ok(LoopDriverDecision::Continue)
            }
            PerfScenarioId::RapidNextPage => {
                if !self.initial_idle_seen {
                    self.initial_idle_seen = true;
                    self.start_measured_window();
                    let last_page = observation.page_count.saturating_sub(1);
                    let steps = self
                        .parameters
                        .page_steps
                        .min(last_page.saturating_sub(observation.current_page));
                    handle
                        .enqueue_commands((0..steps).map(|_| binding_request(Command::NextPage)))?;
                    self.command_count += steps;
                    self.rapid_commands_sent = true;
                    return Ok(if steps == 0 {
                        LoopDriverDecision::Finish
                    } else {
                        LoopDriverDecision::Continue
                    });
                }
                Ok(if self.rapid_commands_sent {
                    LoopDriverDecision::Finish
                } else {
                    LoopDriverDecision::Continue
                })
            }
            PerfScenarioId::ZoomStep => {
                if !self.initial_idle_seen {
                    self.initial_idle_seen = true;
                    self.start_measured_window();
                    handle.enqueue_command(binding_request(Command::ZoomIn))?;
                    self.command_count += 1;
                    self.zoomed_in = true;
                    return Ok(LoopDriverDecision::Continue);
                }
                if self.zoomed_in && !self.zoomed_out {
                    handle.enqueue_command(binding_request(Command::ZoomOut))?;
                    self.command_count += 1;
                    self.zoomed_out = true;
                    return Ok(LoopDriverDecision::Continue);
                }
                Ok(if self.zoomed_out {
                    LoopDriverDecision::Finish
                } else {
                    LoopDriverDecision::Continue
                })
            }
            PerfScenarioId::IdleSettledRedraw => {
                let Some(started_at) = self.idle_started_at else {
                    let started_at = Instant::now();
                    self.idle_started_at = Some(started_at);
                    self.measured_started_at = Some(started_at);
                    return Ok(LoopDriverDecision::Continue);
                };
                Ok(
                    if started_at.elapsed().as_millis()
                        >= u128::from(self.parameters.idle_duration_ms)
                    {
                        LoopDriverDecision::Finish
                    } else {
                        LoopDriverDecision::Continue
                    },
                )
            }
        }
    }

    fn on_finish(
        &mut self,
        observation: LoopObservation,
        metrics: LoopMetricsSnapshot,
    ) -> AppResult<Self::Output> {
        Ok(PerfIterationSnapshot {
            runtime: metrics.runtime,
            presenter: metrics.presenter,
            wall_time: self.measured_elapsed(),
            final_page: observation.current_page,
            visited_steps: self.visited_steps(),
        })
    }

    fn on_loop_break(&mut self) -> AppResult<Self::Output> {
        Err(AppError::unsupported(
            "perf run ended before producing a report",
        ))
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

    use crate::app::{
        LoopDriver, LoopDriverDecision, LoopDriverHandle, LoopObservation, TerminalSurface,
    };
    use crate::event::DomainEvent;
    use crate::perf::{PerfScenarioId, PerfScenarioParameters};

    use super::{HeadlessTerminalSession, PerfLoopDriver};

    fn observation(current_page: usize, page_count: usize, system_idle: bool) -> LoopObservation {
        LoopObservation {
            page_count,
            current_page,
            current_cached: system_idle,
            render_in_flight: 0,
            presenter_pending: false,
            redraw_pending: false,
            event_queue_empty: true,
            system_idle,
        }
    }

    #[test]
    fn headless_terminal_session_supports_terminal_surface_contract() {
        let mut session =
            HeadlessTerminalSession::new(80, 24).expect("headless terminal should initialize");
        let size = session.size().expect("size should resolve");
        assert_eq!(size, Size::new(80, 24));

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

        assert!(driver.measured_started_at.is_none());
        let mut handle = LoopDriverHandle::new(&tx);
        assert!(matches!(
            driver
                .on_iteration(observation(0, 3, false), &mut handle)
                .expect("driver should advance"),
            LoopDriverDecision::Continue
        ));
        assert!(driver.measured_started_at.is_none());

        assert!(matches!(
            driver
                .on_iteration(observation(0, 3, true), &mut handle)
                .expect("driver should advance"),
            LoopDriverDecision::Continue
        ));
        assert!(driver.measured_started_at.is_some());
        assert_eq!(driver.visited_steps(), 2);
    }

    #[test]
    fn steady_prev_starts_measured_window_after_last_page_positioning() {
        let (tx, mut rx) = unbounded_channel();
        let mut driver = PerfLoopDriver::new(
            PerfScenarioId::SteadyPrevPage,
            PerfScenarioParameters {
                page_steps: 1,
                idle_duration_ms: 0,
            },
            std::time::Instant::now(),
        );
        let mut handle = LoopDriverHandle::new(&tx);

        assert!(matches!(
            driver
                .on_iteration(observation(0, 3, true), &mut handle)
                .expect("driver should advance"),
            LoopDriverDecision::Continue
        ));
        assert!(driver.measured_started_at.is_none());
        assert!(matches!(
            rx.try_recv().expect("last page command should be queued"),
            DomainEvent::Command(_)
        ));

        assert!(matches!(
            driver
                .on_iteration(observation(2, 3, true), &mut handle)
                .expect("driver should advance"),
            LoopDriverDecision::Continue
        ));
        assert!(driver.measured_started_at.is_some());
        assert_eq!(driver.visited_steps(), 1);
    }
}
