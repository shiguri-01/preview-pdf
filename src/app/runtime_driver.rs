use tokio::sync::mpsc::UnboundedSender;

use crate::command::{CommandInvocationSource, CommandRequest};
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::metrics::PerfStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeMode {
    Interactive { watch: bool },
    Headless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeObservation {
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
pub(crate) enum RuntimeDriverDecision {
    Continue,
    Finish,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RuntimeMetricsSnapshot {
    pub(crate) runtime: PerfStats,
    pub(crate) presenter: PerfStats,
}

pub(crate) trait RuntimeDriver {
    type Output;

    fn on_iteration(
        &mut self,
        observation: RuntimeObservation,
        handle: &mut RuntimeDriverHandle<'_>,
    ) -> AppResult<RuntimeDriverDecision>;

    fn on_finish(
        &mut self,
        observation: RuntimeObservation,
        metrics: RuntimeMetricsSnapshot,
    ) -> AppResult<Self::Output>;

    fn on_break(&mut self) -> AppResult<Self::Output>;
}

pub(crate) struct RuntimeDriverHandle<'a> {
    event_tx: &'a UnboundedSender<DomainEvent>,
}

impl<'a> RuntimeDriverHandle<'a> {
    pub(crate) fn new(event_tx: &'a UnboundedSender<DomainEvent>) -> Self {
        Self { event_tx }
    }

    pub(crate) fn enqueue_command(&mut self, request: CommandRequest) -> AppResult<()> {
        self.event_tx
            .send(DomainEvent::Command(request))
            .map_err(|_| AppError::unsupported("runtime command channel closed"))
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
pub(crate) struct InteractiveRuntimeDriver;

impl RuntimeDriver for InteractiveRuntimeDriver {
    type Output = ();

    fn on_iteration(
        &mut self,
        _observation: RuntimeObservation,
        _handle: &mut RuntimeDriverHandle<'_>,
    ) -> AppResult<RuntimeDriverDecision> {
        Ok(RuntimeDriverDecision::Continue)
    }

    fn on_finish(
        &mut self,
        _observation: RuntimeObservation,
        _metrics: RuntimeMetricsSnapshot,
    ) -> AppResult<Self::Output> {
        Ok(())
    }

    fn on_break(&mut self) -> AppResult<Self::Output> {
        Ok(())
    }
}

pub(crate) fn binding_request(command: crate::command::Command) -> CommandRequest {
    CommandRequest::new(command, CommandInvocationSource::Binding)
}
