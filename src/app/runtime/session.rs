use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time;

use crate::backend::SharedPdfBackend;
use crate::event::{DocumentReloadRequest, DomainEvent};
use crate::extension::ExtensionWorkerEvent;
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::RenderTask;
use crate::render::worker::RenderWorker;

use super::super::actors::{InputActor, RenderActor, UiActor};
use super::super::event_bus::EventBusRuntime;
use super::super::render_ops::{CurrentInterestKeys, PrefetchDispatchContext, RequiredRenderPages};
use super::super::terminal_session::TerminalSession;
use super::super::view_ops::InitialPreviewPlan;

pub(in crate::app) struct AppRuntime<S> {
    pub(in crate::app) page_count: usize,
    pub(in crate::app) prefetch_pause_after_input: Duration,
    pub(in crate::app) input_poll_timeout_idle: Duration,
    pub(in crate::app) input_poll_timeout_busy: Duration,
    pub(in crate::app) input_actor: InputActor,
    pub(in crate::app) render_actor: RenderActor,
    pub(in crate::app) ui_actor: UiActor,
    pub(in crate::app) session: S,
    pub(in crate::app) render_worker: RenderWorker,
    pub(in crate::app) prefetch_tick: time::Interval,
    pub(in crate::app) redraw_tick: time::Interval,
    pub(in crate::app) event_tx: UnboundedSender<DomainEvent>,
    pub(in crate::app) event_rx: UnboundedReceiver<DomainEvent>,
    pub(in crate::app) extension_worker_rx: UnboundedReceiver<ExtensionWorkerEvent>,
    pub(in crate::app) event_bus: EventBusRuntime,
    pub(in crate::app) reload_in_flight: bool,
    pub(in crate::app) pending_reload: Option<DocumentReloadRequest>,
    pub(in crate::app) reload_retry_attempts: u8,
    pub(in crate::app) reload_generation: u64,
}

pub(in crate::app) struct ActiveDocument {
    pub(in crate::app) pdf: SharedPdfBackend,
    pub(in crate::app) path: PathBuf,
}

impl ActiveDocument {
    pub(in crate::app) fn new(pdf: SharedPdfBackend) -> Self {
        let path = pdf.path().to_path_buf();
        Self { pdf, path }
    }

    pub(in crate::app) fn replace(&mut self, pdf: SharedPdfBackend) {
        self.path = pdf.path().to_path_buf();
        self.pdf = pdf;
    }
}

pub(in crate::app) struct IterationStep {
    pub(in crate::app) current_scale: f32,
    pub(in crate::app) visible_pages: super::super::state::VisiblePageSlots,
    pub(in crate::app) required: RequiredRenderPages,
    pub(in crate::app) current_interest_keys: CurrentInterestKeys,
    pub(in crate::app) initial_preview: Option<InitialPreviewPlan>,
    pub(in crate::app) initial_preview_tasks: Vec<RenderTask>,
    pub(in crate::app) prefetch_dispatch: PrefetchDispatchContext,
    pub(in crate::app) presenter_key: RenderedPageKey,
    pub(in crate::app) current_cached: bool,
}

pub(in crate::app) enum RuntimeEvent {
    Event(DomainEvent),
    Closed,
}

pub(in crate::app) enum RuntimeControl {
    Continue,
    Break,
}

pub(in crate::app) fn terminate_process_now<S>(runtime: &mut AppRuntime<S>) -> !
where
    S: TerminalSession,
{
    runtime.event_bus.shutdown();
    if let Err(err) = runtime.session.restore() {
        eprintln!("failed to restore terminal session before exit: {err}");
    }
    std::process::exit(0);
}
