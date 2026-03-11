use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;

use crate::command::Command;
use crate::event::DomainEvent;
use crate::perf::PerfScenarioId;

use super::state::AppState;

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
                let _ = loop_event_tx.send(DomainEvent::Command(Command::NextPage));
                self.command_count += 1;
                false
            }
            PerfScenarioId::PageFlipBackward => {
                let params = self.scenario.parameters();
                let last_page = page_count.saturating_sub(1);
                if !self.positioned_backward_start {
                    if state.current_page < last_page {
                        let _ = loop_event_tx.send(DomainEvent::Command(Command::LastPage));
                        self.positioned_backward_start = true;
                        return false;
                    }
                    self.positioned_backward_start = true;
                }

                if state.current_page == 0 || self.command_count >= params.page_flip_limit {
                    return true;
                }

                let _ = loop_event_tx.send(DomainEvent::Command(Command::PrevPage));
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
