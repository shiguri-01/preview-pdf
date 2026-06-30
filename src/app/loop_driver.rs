use tokio::sync::mpsc::UnboundedSender;

use crate::command::{CommandInvocationSource, CommandRequest};
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::metrics::PerfStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopEventMode {
    Interactive { watch: bool },
    Headless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoopObservation {
    pub(crate) page_count: usize,
    pub(crate) current_page: usize,
    pub(crate) current_cached: bool,
    pub(crate) render_in_flight: usize,
    pub(crate) presenter_pending: bool,
    pub(crate) redraw_pending: bool,
    pub(crate) event_queue_empty: bool,
    pub(crate) system_idle: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopDriverDecision {
    Continue,
    Finish,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopMetricsSnapshot {
    pub(crate) runtime: PerfStats,
    pub(crate) presenter: PerfStats,
}

pub(crate) trait LoopDriver {
    type Output;

    fn on_iteration(
        &mut self,
        observation: LoopObservation,
        handle: &mut LoopDriverHandle<'_>,
    ) -> AppResult<LoopDriverDecision>;

    fn on_finish(
        &mut self,
        observation: LoopObservation,
        metrics: LoopMetricsSnapshot,
    ) -> AppResult<Self::Output>;

    fn on_loop_break(&mut self) -> AppResult<Self::Output>;
}

pub(crate) struct LoopDriverHandle<'a> {
    loop_event_tx: &'a UnboundedSender<DomainEvent>,
}

impl<'a> LoopDriverHandle<'a> {
    pub(crate) fn new(loop_event_tx: &'a UnboundedSender<DomainEvent>) -> Self {
        Self { loop_event_tx }
    }

    pub(crate) fn enqueue_command(&mut self, request: CommandRequest) -> AppResult<()> {
        self.loop_event_tx
            .send(DomainEvent::Command(request))
            .map_err(|_| AppError::unsupported("event loop command channel closed"))
    }

    pub(crate) fn enqueue_commands(
        &mut self,
        requests: impl IntoIterator<Item = CommandRequest>,
    ) -> AppResult<()> {
        for request in requests {
            self.enqueue_command(request)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(crate) struct InteractiveLoopDriver;

impl LoopDriver for InteractiveLoopDriver {
    type Output = ();

    fn on_iteration(
        &mut self,
        _observation: LoopObservation,
        _handle: &mut LoopDriverHandle<'_>,
    ) -> AppResult<LoopDriverDecision> {
        Ok(LoopDriverDecision::Continue)
    }

    fn on_finish(
        &mut self,
        _observation: LoopObservation,
        _metrics: LoopMetricsSnapshot,
    ) -> AppResult<Self::Output> {
        Ok(())
    }

    fn on_loop_break(&mut self) -> AppResult<Self::Output> {
        Ok(())
    }
}

pub(crate) fn binding_request(command: crate::command::Command) -> CommandRequest {
    CommandRequest::new(command, CommandInvocationSource::Binding)
}
