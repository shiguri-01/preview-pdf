use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{self, MissedTickBehavior};

use crate::backend::PdfBackend;
use crate::command::{ActionId, CommandOutcome};
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::presenter::{PanOffset, Viewport};
use crate::render::cache::RenderedPageKey;
use crate::render::worker::RenderWorker;

use super::actors::{InputActor, RenderActor, UiActor};
use super::core::App;
use super::event_bus::EventBusRuntime;
use super::render_ops::{CurrentTaskContext, PrefetchDispatchContext};
use super::scale::select_input_poll_timeout;
use super::terminal_session::{TerminalSession, TerminalSurface};
use super::view_ops::RenderFramePlan;

struct LoopRuntime {
    page_count: usize,
    prefetch_pause_after_input: Duration,
    input_poll_timeout_idle: Duration,
    input_poll_timeout_busy: Duration,
    input_actor: InputActor,
    render_actor: RenderActor,
    ui_actor: UiActor,
    session: TerminalSession,
    render_worker: RenderWorker,
    prefetch_tick: time::Interval,
    redraw_tick: time::Interval,
    loop_event_tx: UnboundedSender<DomainEvent>,
    loop_event_rx: UnboundedReceiver<DomainEvent>,
    loop_event_runtime: EventBusRuntime,
}

struct LoopStep {
    current_scale: f32,
    prefetch_viewport: Option<Viewport>,
    base_pan: PanOffset,
    enable_crop: bool,
    interactive: bool,
    current_key: RenderedPageKey,
    current_cached: bool,
}

enum WaitEvent {
    Event(DomainEvent),
    Closed,
}

enum LoopControl {
    Continue,
    Break,
}

impl App {
    fn terminate_process_now(runtime: &mut LoopRuntime) -> ! {
        runtime.loop_event_runtime.shutdown();
        let _ = runtime.session.restore();
        std::process::exit(0);
    }

    pub async fn run(&mut self, pdf: &mut dyn PdfBackend) -> AppResult<()> {
        let page_count = pdf.page_count();
        if page_count == 0 {
            return Err(AppError::invalid_argument("pdf has no pages"));
        }

        let mut runtime = self.initialize_loop_runtime(pdf, page_count)?;

        loop {
            let step = self.build_loop_step(
                &runtime.session,
                pdf,
                &runtime.input_actor,
                runtime.prefetch_pause_after_input,
            );
            let changed = self.drain_background_and_sync_navigation(
                pdf,
                &mut runtime.render_actor,
                step.current_scale,
            );
            self.render.ensure_current_task_enqueued(
                &mut self.state,
                pdf,
                &runtime.render_actor,
                &mut runtime.render_worker,
                CurrentTaskContext {
                    current_key: step.current_key,
                    current_scale: step.current_scale,
                    current_cached: step.current_cached,
                },
            );
            self.render.dispatch_prefetch_if_due(
                &mut self.state,
                &mut runtime.render_actor,
                &mut runtime.render_worker,
                PrefetchDispatchContext {
                    current_key: step.current_key,
                    current_cached: step.current_cached,
                    prefetch_viewport: step.prefetch_viewport,
                    base_pan: step.base_pan,
                    enable_crop: step.enable_crop,
                    interactive: step.interactive,
                    dispatch_budget: self.config.render.prefetch_dispatch_budget_per_tick,
                },
            );
            self.update_ui_and_render_frame(&mut runtime, pdf, changed, step.current_cached)?;

            let wake_timeout = select_input_poll_timeout(
                runtime.render_worker.in_flight_len() > 0,
                self.render.presenter.has_pending_work(),
                runtime.input_poll_timeout_idle,
                runtime.input_poll_timeout_busy,
            );
            let waited = wait_next_event(
                &mut runtime.loop_event_rx,
                &mut runtime.render_worker,
                &mut runtime.prefetch_tick,
                &mut runtime.redraw_tick,
                wake_timeout,
            )
            .await;
            if matches!(
                self.handle_waited_event(waited, &mut runtime, pdf)?,
                LoopControl::Break
            ) {
                break;
            }
        }

        runtime.loop_event_runtime.shutdown();
        runtime.session.restore()?;
        Ok(())
    }

    fn initialize_loop_runtime(
        &mut self,
        pdf: &dyn PdfBackend,
        page_count: usize,
    ) -> AppResult<LoopRuntime> {
        self.state.current_page = self.state.current_page.min(page_count - 1);

        let loop_started_at = Instant::now();
        let pending_redraw_interval =
            Duration::from_millis(self.config.render.pending_redraw_interval_ms);
        let input_actor = InputActor::new(loop_started_at);
        let ui_actor = UiActor::new(loop_started_at, pending_redraw_interval);
        let session = TerminalSession::enter()?;
        self.render.presenter.initialize_terminal()?;

        let prefetch_pause_after_input =
            Duration::from_millis(self.config.render.prefetch_pause_ms);
        let prefetch_tick_interval = Duration::from_millis(self.config.render.prefetch_tick_ms);
        let input_poll_timeout_idle =
            Duration::from_millis(self.config.render.input_poll_timeout_idle_ms);
        let input_poll_timeout_busy =
            Duration::from_millis(self.config.render.input_poll_timeout_busy_ms);
        let (loop_event_tx, loop_event_rx, loop_event_runtime) = EventBusRuntime::spawn();
        let mut prefetch_tick = time::interval(prefetch_tick_interval);
        prefetch_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut redraw_tick = time::interval(pending_redraw_interval);
        redraw_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let render_worker = RenderWorker::spawn(
            pdf.path().to_path_buf(),
            pdf.doc_id(),
            self.config.render.worker_threads,
        );
        let tracked_scale = self.compute_current_scale(
            pdf,
            self.state.current_page,
            Self::current_viewport(&session, self.state.debug_status_visible),
        );
        let mut render_actor =
            RenderActor::new(self.state.current_page, self.state.zoom, tracked_scale);
        self.render.runtime.reset_prefetch(
            pdf,
            self.state.current_page,
            render_actor.nav_mut().intent(),
            tracked_scale,
        );

        Ok(LoopRuntime {
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
            loop_event_tx,
            loop_event_rx,
            loop_event_runtime,
        })
    }

    fn build_loop_step(
        &mut self,
        session: &impl TerminalSurface,
        pdf: &dyn PdfBackend,
        input_actor: &InputActor,
        prefetch_pause_after_input: Duration,
    ) -> LoopStep {
        let prefetch_viewport = Self::current_viewport(session, self.state.debug_status_visible);
        let current_scale =
            self.compute_current_scale(pdf, self.state.current_page, prefetch_viewport);
        let base_pan = self.current_pan();
        let enable_crop = self.state.zoom > 1.0;
        let interactive = input_actor.is_interactive(prefetch_pause_after_input);
        let current_key =
            RenderedPageKey::new(pdf.doc_id(), self.state.current_page, current_scale);
        let current_cached = self.render.runtime.has_cached_frame(&current_key);

        LoopStep {
            current_scale,
            prefetch_viewport,
            base_pan,
            enable_crop,
            interactive,
            current_key,
            current_cached,
        }
    }

    fn drain_background_and_sync_navigation(
        &mut self,
        pdf: &dyn PdfBackend,
        render_actor: &mut RenderActor,
        current_scale: f32,
    ) -> bool {
        let mut changed = false;
        if self.interaction.drain_background_events(&mut self.state) {
            changed = true;
        }
        if self.render.presenter.drain_background_events() {
            changed = true;
        }
        if self.interaction.apply_palette_requests(&mut self.state) {
            changed = true;
        }

        let mut nav_sync_parts = render_actor.nav_sync_parts_mut();
        if self
            .render
            .sync_navigation_state(&self.state, pdf, &mut nav_sync_parts, current_scale)
        {
            changed = true;
        }
        changed
    }

    fn update_ui_and_render_frame(
        &mut self,
        runtime: &mut LoopRuntime,
        pdf: &dyn PdfBackend,
        changed: bool,
        current_cached: bool,
    ) -> AppResult<()> {
        let render_busy = runtime.render_worker.in_flight_len() > 0;
        let presenter_busy = self.render.presenter.has_pending_work();
        if runtime.ui_actor.should_request_pending_redraw(
            current_cached,
            render_busy,
            presenter_busy,
        ) {
            runtime.ui_actor.mark_redraw();
        }

        if changed {
            runtime.ui_actor.mark_redraw();
        }

        if runtime.ui_actor.needs_redraw() {
            let palette_view = self.interaction.palette_view();
            self.render.render_frame(
                &mut self.state,
                &self.config,
                &mut runtime.session,
                pdf,
                RenderFramePlan {
                    palette_view,
                    page_count: runtime.page_count,
                    generation: runtime.render_actor.generation(),
                },
            )?;
            runtime.ui_actor.clear_redraw();
            if !current_cached {
                runtime.ui_actor.on_drawn_non_cached_page();
            }
        }
        Ok(())
    }

    fn handle_waited_event(
        &mut self,
        waited: WaitEvent,
        runtime: &mut LoopRuntime,
        pdf: &mut dyn PdfBackend,
    ) -> AppResult<LoopControl> {
        match waited {
            WaitEvent::Event(DomainEvent::Input(event)) => {
                let input_outcome = self.handle_input_event(
                    event,
                    &mut runtime.session,
                    runtime.ui_actor.needs_redraw_mut(),
                    runtime.input_actor.last_input_at_mut(),
                )?;
                if input_outcome.quit_requested {
                    Self::terminate_process_now(runtime);
                }
                if let Some(command) = input_outcome.command {
                    let _ = runtime.loop_event_tx.send(DomainEvent::Command(command));
                }
            }
            WaitEvent::Event(DomainEvent::InputError(message)) => {
                self.state.status.last_action_id = Some(ActionId::Input);
                self.state.status.message = format!("input error: {message}");
                runtime.ui_actor.mark_redraw();
            }
            WaitEvent::Event(DomainEvent::Command(command)) => {
                let dispatch = self
                    .interaction
                    .dispatch_command(&mut self.state, command, pdf)?;
                for event in dispatch.emitted_events {
                    let _ = runtime.loop_event_tx.send(DomainEvent::App(event));
                }
                if self.interaction.apply_palette_requests(&mut self.state) {
                    runtime.ui_actor.mark_redraw();
                }
                match dispatch.outcome {
                    CommandOutcome::QuitRequested => {
                        Self::terminate_process_now(runtime);
                    }
                    CommandOutcome::Applied | CommandOutcome::Noop => {
                        runtime.ui_actor.mark_redraw()
                    }
                }
            }
            WaitEvent::Event(DomainEvent::App(event)) => {
                self.interaction.handle_app_event(&mut self.state, &event);
                runtime.ui_actor.mark_redraw();
            }
            WaitEvent::Event(DomainEvent::RenderComplete(completed)) => {
                let viewport =
                    Self::current_viewport(&runtime.session, self.state.debug_status_visible);
                let scale = self.compute_current_scale(pdf, self.state.current_page, viewport);
                let current_key =
                    RenderedPageKey::new(pdf.doc_id(), self.state.current_page, scale);
                let pan = self.current_pan();
                let enable_crop = self.state.zoom > 1.0;
                if self.render.process_render_result(
                    &mut self.state,
                    completed,
                    current_key,
                    viewport,
                    pan,
                    enable_crop,
                    runtime
                        .input_actor
                        .is_interactive(runtime.prefetch_pause_after_input),
                ) {
                    runtime.ui_actor.mark_redraw();
                }
            }
            WaitEvent::Event(DomainEvent::PrefetchTick) => {
                runtime.render_actor.mark_prefetch_due();
            }
            WaitEvent::Event(DomainEvent::RedrawTick) => {
                runtime.ui_actor.mark_redraw();
            }
            WaitEvent::Event(DomainEvent::Wake) => {}
            WaitEvent::Closed => return Ok(LoopControl::Break),
        }
        Ok(LoopControl::Continue)
    }
}

async fn wait_next_event(
    loop_event_rx: &mut UnboundedReceiver<DomainEvent>,
    render_worker: &mut RenderWorker,
    prefetch_tick: &mut time::Interval,
    redraw_tick: &mut time::Interval,
    wake_timeout: Duration,
) -> WaitEvent {
    tokio::select! {
        biased;
        maybe_loop = loop_event_rx.recv() => {
            match maybe_loop {
                Some(event) => WaitEvent::Event(event),
                None => WaitEvent::Closed,
            }
        },
        maybe_render = render_worker.recv_result() => {
            match maybe_render {
                Some(result) => WaitEvent::Event(DomainEvent::RenderComplete(result)),
                None => WaitEvent::Closed,
            }
        },
        _ = prefetch_tick.tick() => {
            WaitEvent::Event(DomainEvent::PrefetchTick)
        },
        _ = redraw_tick.tick() => {
            WaitEvent::Event(DomainEvent::RedrawTick)
        },
        _ = time::sleep(wake_timeout) => {
            WaitEvent::Event(DomainEvent::Wake)
        }
    }
}
