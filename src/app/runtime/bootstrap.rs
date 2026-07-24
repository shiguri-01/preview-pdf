use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::{self, MissedTickBehavior};

use crate::backend::SharedPdfBackend;
use crate::error::AppResult;
use crate::event::DomainEvent;
use crate::render::worker::RenderWorker;

use super::super::actors::{InputActor, RenderActor, UiActor};
use super::super::core::App;
use super::super::event_bus::EventBusRuntime;
use super::super::terminal_session::TerminalSurface;
use super::AppRuntime;

impl App {
    pub(in crate::app) fn initialize_runtime<S>(
        &mut self,
        pdf: SharedPdfBackend,
        page_count: usize,
        session: S,
        event_tx: UnboundedSender<DomainEvent>,
        event_rx: UnboundedReceiver<DomainEvent>,
        event_bus: EventBusRuntime,
    ) -> AppResult<AppRuntime<S>>
    where
        S: TerminalSurface,
    {
        self.state.current_page = self.state.current_page.min(page_count - 1);
        self.state.normalize_current_page(page_count);

        let runtime_started_at = Instant::now();
        let pending_redraw_interval = self.event_loop_policy.pending_redraw_interval;
        let input_actor = InputActor::new(runtime_started_at);
        let ui_actor = UiActor::new(runtime_started_at, pending_redraw_interval);
        self.render.presenter.initialize_terminal()?;

        let prefetch_pause_after_input = self.event_loop_policy.prefetch_pause_after_input;
        let prefetch_tick_interval = self.event_loop_policy.prefetch_tick_interval;
        let input_poll_timeout_idle = self.event_loop_policy.input_poll_timeout_idle;
        let input_poll_timeout_busy = self.event_loop_policy.input_poll_timeout_busy;
        let mut prefetch_tick = time::interval(prefetch_tick_interval);
        prefetch_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut redraw_tick = time::interval(pending_redraw_interval);
        redraw_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let render_worker =
            RenderWorker::spawn(Arc::clone(&pdf), self.render_policy.worker_threads);
        let viewport = Self::current_viewport(&session, self.state.debug_status_visible);
        let visible_pages = self.state.visible_page_slots(page_count);
        let tracked_scale =
            self.compute_current_scale(pdf.as_ref(), visible_pages.anchor_page, viewport);
        let mut render_actor =
            RenderActor::new(visible_pages.anchor_page, self.state.zoom, tracked_scale);
        let (extension_worker_tx, extension_worker_rx) = unbounded_channel();
        self.interaction
            .start_extension_workers(extension_worker_tx);
        self.render.runtime.reset_prefetch(
            pdf.as_ref(),
            visible_pages.anchor_page,
            render_actor.nav_mut().intent(),
            tracked_scale,
        );
        self.interaction
            .prepare_extensions_for_document(Arc::clone(&pdf));

        Ok(AppRuntime {
            page_count,
            prefetch_pause_after_input,
            input_poll_timeout_idle,
            input_poll_timeout_busy,
            input_actor,
            render_actor,
            ui_actor,
            session,
            render_worker,
            prefetch_tick,
            redraw_tick,
            event_tx,
            event_rx,
            extension_worker_rx,
            event_bus,
            reload_in_flight: false,
            pending_reload: None,
            reload_retry_attempts: 0,
            reload_generation: 0,
        })
    }
}
