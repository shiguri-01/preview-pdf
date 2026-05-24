use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time;

use crate::event::DomainEvent;
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::RenderTask;
use crate::render::worker::RenderWorker;

use super::actors::{InputActor, RenderActor, UiActor};
use super::event_bus::EventBusRuntime;
use super::perf_runner::HeadlessTerminalSession;
use super::render_ops::{CurrentInterestKeys, PrefetchDispatchContext, RequiredRenderPages};
use super::terminal_session::InteractiveTerminalSession;
use super::view_ops::InitialPreviewPlan;

pub(super) struct LoopRuntime<S> {
    pub(super) page_count: usize,
    pub(super) prefetch_pause_after_input: Duration,
    pub(super) input_poll_timeout_idle: Duration,
    pub(super) input_poll_timeout_busy: Duration,
    pub(super) input_actor: InputActor,
    pub(super) render_actor: RenderActor,
    pub(super) ui_actor: UiActor,
    pub(super) session: S,
    pub(super) render_worker: RenderWorker,
    pub(super) prefetch_tick: time::Interval,
    pub(super) redraw_tick: time::Interval,
    pub(super) loop_event_tx: UnboundedSender<DomainEvent>,
    pub(super) loop_event_rx: UnboundedReceiver<DomainEvent>,
    pub(super) loop_event_runtime: EventBusRuntime,
}

pub(super) struct LoopStep {
    pub(super) current_scale: f32,
    pub(super) visible_pages: super::state::VisiblePageSlots,
    pub(super) required: RequiredRenderPages,
    pub(super) current_interest_keys: CurrentInterestKeys,
    pub(super) initial_preview: Option<InitialPreviewPlan>,
    pub(super) initial_preview_tasks: Vec<RenderTask>,
    pub(super) prefetch_dispatch: PrefetchDispatchContext,
    pub(super) presenter_key: RenderedPageKey,
    pub(super) current_cached: bool,
}

pub(super) enum WaitEvent {
    Event(DomainEvent),
    Closed,
}

pub(super) enum LoopControl {
    Continue,
    Break,
}

pub(super) trait SessionRestore {
    fn restore(&mut self) -> std::io::Result<()>;
}

impl SessionRestore for InteractiveTerminalSession {
    fn restore(&mut self) -> std::io::Result<()> {
        InteractiveTerminalSession::restore(self)
    }
}

impl SessionRestore for HeadlessTerminalSession {
    fn restore(&mut self) -> std::io::Result<()> {
        HeadlessTerminalSession::restore(self)
    }
}

pub(super) fn terminate_process_now<S>(runtime: &mut LoopRuntime<S>) -> !
where
    S: super::terminal_session::TerminalSurface + SessionRestore,
{
    runtime.loop_event_runtime.shutdown();
    if let Err(err) = runtime.session.restore() {
        eprintln!("failed to restore terminal session before exit: {err}");
    }
    std::process::exit(0);
}
