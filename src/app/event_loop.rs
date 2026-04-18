use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{self, MissedTickBehavior};

use crate::backend::{PdfBackend, SharedPdfBackend};
use crate::command::{Command, CommandOutcome, CommandRequest, PanAmount};
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::perf::{PerfIterationSnapshot, PerfScenarioId, RedrawReason};
use crate::presenter::{ImagePresenter, PanOffset, PresenterBackgroundEvent, Viewport};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::RenderTask;
use crate::render::worker::RenderWorker;
use crate::work::WorkClass;

use super::actors::{InputActor, RenderActor, UiActor};
use super::core::App;
use super::event_bus::EventBusRuntime;
use super::perf_runner::{
    HeadlessTerminalSession, PERF_HEADLESS_HEIGHT, PERF_HEADLESS_WIDTH, PerfLoopDriver,
};
use super::render_ops::{CurrentTaskContext, PrefetchDispatchContext};
use super::scale::select_input_poll_timeout;
use super::state::notice_action_for_error;
use super::terminal_session::{InteractiveTerminalSession, TerminalSurface};
use super::view_ops::{InitialPreviewPlan, RenderFramePlan, compute_initial_preview_plan};

struct LoopRuntime<S> {
    page_count: usize,
    prefetch_pause_after_input: Duration,
    input_poll_timeout_idle: Duration,
    input_poll_timeout_busy: Duration,
    input_actor: InputActor,
    render_actor: RenderActor,
    ui_actor: UiActor,
    session: S,
    render_worker: RenderWorker,
    prefetch_tick: time::Interval,
    redraw_tick: time::Interval,
    loop_event_tx: UnboundedSender<DomainEvent>,
    loop_event_rx: UnboundedReceiver<DomainEvent>,
    loop_event_runtime: EventBusRuntime,
}

struct LoopStep {
    current_scale: f32,
    overlay_stamp: u64,
    prefetch_viewport: Option<Viewport>,
    base_pan: PanOffset,
    enable_crop: bool,
    interactive: bool,
    visible_pages: super::state::VisiblePageSlots,
    required_pages: Vec<usize>,
    required_render_keys: Vec<RenderedPageKey>,
    current_interest_keys: Vec<RenderedPageKey>,
    initial_preview: Option<InitialPreviewPlan>,
    presenter_key: RenderedPageKey,
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

trait SessionRestore {
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

impl App {
    fn enqueue_loop_event<S>(runtime: &mut LoopRuntime<S>, event: DomainEvent) -> LoopControl {
        match runtime.loop_event_tx.send(event) {
            Ok(()) => LoopControl::Continue,
            Err(_) => LoopControl::Break,
        }
    }

    fn enqueue_commands_and_optional_quit<S>(
        runtime: &mut LoopRuntime<S>,
        commands: Vec<CommandRequest>,
        quit_requested: bool,
    ) -> LoopControl {
        for request in commands {
            if matches!(
                Self::enqueue_loop_event(runtime, DomainEvent::Command(request)),
                LoopControl::Break
            ) {
                return LoopControl::Break;
            }
        }
        if quit_requested {
            return Self::enqueue_loop_event(runtime, DomainEvent::Quit);
        }
        LoopControl::Continue
    }

    fn resolve_command_request<S: TerminalSurface>(
        &self,
        session: &S,
        request: CommandRequest,
    ) -> CommandRequest {
        CommandRequest {
            command: self.resolve_command(session, request.command),
            source: request.source,
        }
    }

    fn resolve_command<S: TerminalSurface>(&self, session: &S, command: Command) -> Command {
        match command {
            Command::Pan {
                direction,
                amount: PanAmount::DefaultStep,
            } => Command::Pan {
                direction,
                amount: PanAmount::Cells(self.default_pan_step_cells(session)),
            },
            _ => command,
        }
    }

    fn default_pan_step_cells<S: TerminalSurface>(&self, session: &S) -> i32 {
        let Some(viewport) = Self::current_viewport(session, self.state.debug_status_visible)
        else {
            return 1;
        };
        i32::from((viewport.width.min(viewport.height) / 5).max(1))
    }

    fn terminate_process_now<S>(runtime: &mut LoopRuntime<S>) -> !
    where
        S: TerminalSurface + SessionRestore,
    {
        runtime.loop_event_runtime.shutdown();
        let _ = runtime.session.restore();
        std::process::exit(0);
    }

    pub async fn run(&mut self, pdf: SharedPdfBackend) -> AppResult<()> {
        let page_count = pdf.page_count();
        if page_count == 0 {
            return Err(AppError::invalid_argument("pdf has no pages"));
        }

        let session = InteractiveTerminalSession::enter()?;
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            EventBusRuntime::spawn_interactive();
        let mut runtime = self.initialize_loop_runtime(
            Arc::clone(&pdf),
            page_count,
            session,
            loop_event_tx,
            loop_event_rx,
            loop_event_runtime,
        )?;
        runtime
            .loop_event_runtime
            .start_input(runtime.loop_event_tx.clone());
        let result = self.run_interactive_loop(&mut runtime, pdf).await;
        runtime.loop_event_runtime.shutdown();
        let restore_result = runtime.session.restore();
        result?;
        restore_result?;
        Ok(())
    }

    pub async fn run_perf(
        &mut self,
        pdf: SharedPdfBackend,
        scenario: PerfScenarioId,
    ) -> AppResult<PerfIterationSnapshot> {
        let page_count = pdf.page_count();
        if page_count == 0 {
            return Err(AppError::invalid_argument("pdf has no pages"));
        }

        self.render.runtime.perf_stats.reset();
        self.render.presenter.initialize_headless_for_perf()?;
        self.render.runtime.perf_stats.enable_sample_collection();
        self.render.presenter.enable_perf_sample_collection();
        let session = HeadlessTerminalSession::new(PERF_HEADLESS_WIDTH, PERF_HEADLESS_HEIGHT)?;
        let (loop_event_tx, loop_event_rx, loop_event_runtime) = EventBusRuntime::spawn_headless();
        let mut runtime = self.initialize_loop_runtime(
            Arc::clone(&pdf),
            page_count,
            session,
            loop_event_tx,
            loop_event_rx,
            loop_event_runtime,
        )?;
        let result = self.run_perf_loop(&mut runtime, pdf, scenario).await;
        runtime.loop_event_runtime.shutdown();
        let restore_result = runtime.session.restore();
        let snapshot = result?;
        restore_result?;
        Ok(snapshot)
    }

    fn initialize_loop_runtime<S>(
        &mut self,
        pdf: SharedPdfBackend,
        page_count: usize,
        session: S,
        loop_event_tx: UnboundedSender<DomainEvent>,
        loop_event_rx: UnboundedReceiver<DomainEvent>,
        loop_event_runtime: EventBusRuntime,
    ) -> AppResult<LoopRuntime<S>>
    where
        S: TerminalSurface,
    {
        self.state.current_page = self.state.current_page.min(page_count - 1);
        self.state.normalize_current_page(page_count);

        let loop_started_at = Instant::now();
        let pending_redraw_interval =
            Duration::from_millis(self.config.render.pending_redraw_interval_ms);
        let input_actor = InputActor::new(loop_started_at);
        let ui_actor = UiActor::new(loop_started_at, pending_redraw_interval);
        self.render.presenter.initialize_terminal()?;

        let prefetch_pause_after_input =
            Duration::from_millis(self.config.render.prefetch_pause_ms);
        let prefetch_tick_interval = Duration::from_millis(self.config.render.prefetch_tick_ms);
        let input_poll_timeout_idle =
            Duration::from_millis(self.config.render.input_poll_timeout_idle_ms);
        let input_poll_timeout_busy =
            Duration::from_millis(self.config.render.input_poll_timeout_busy_ms);
        let mut prefetch_tick = time::interval(prefetch_tick_interval);
        prefetch_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut redraw_tick = time::interval(pending_redraw_interval);
        redraw_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let render_worker =
            RenderWorker::spawn(Arc::clone(&pdf), self.config.render.worker_threads);
        let viewport = Self::current_viewport(&session, self.state.debug_status_visible);
        let visible_pages = self.state.visible_page_slots(page_count);
        let tracked_scale =
            self.compute_current_scale(pdf.as_ref(), visible_pages.anchor_page, viewport);
        let mut render_actor =
            RenderActor::new(visible_pages.anchor_page, self.state.zoom, tracked_scale);
        self.render.runtime.reset_prefetch(
            pdf.as_ref(),
            visible_pages.anchor_page,
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

    async fn run_interactive_loop(
        &mut self,
        runtime: &mut LoopRuntime<InteractiveTerminalSession>,
        pdf: SharedPdfBackend,
    ) -> AppResult<()> {
        loop {
            let step = self.process_loop_iteration(runtime, pdf.as_ref())?;
            match self
                .wait_and_handle_next_event(runtime, &step, Arc::clone(&pdf))
                .await?
            {
                LoopControl::Continue => {}
                LoopControl::Break => return Ok(()),
            }
        }
    }

    async fn run_perf_loop(
        &mut self,
        runtime: &mut LoopRuntime<HeadlessTerminalSession>,
        pdf: SharedPdfBackend,
        scenario: PerfScenarioId,
    ) -> AppResult<PerfIterationSnapshot> {
        let mut perf_driver = PerfLoopDriver::new(scenario);

        loop {
            let step = self.process_loop_iteration(runtime, pdf.as_ref())?;
            let system_idle = step.current_cached
                && runtime.render_worker.in_flight_len() == 0
                && !self.render.presenter.has_pending_work()
                && !runtime.ui_actor.needs_redraw();
            if perf_driver.advance(
                &self.state,
                runtime.page_count,
                system_idle,
                &runtime.loop_event_tx,
            ) {
                return Ok(PerfIterationSnapshot {
                    runtime: self.render.runtime.perf_stats.clone(),
                    presenter: self.render.presenter.perf_snapshot().unwrap_or_default(),
                });
            }

            match self
                .wait_and_handle_next_event(runtime, &step, Arc::clone(&pdf))
                .await?
            {
                LoopControl::Continue => {}
                LoopControl::Break => {
                    return Err(AppError::unsupported(
                        "perf run ended before producing a report",
                    ));
                }
            }
        }
    }

    fn process_loop_iteration<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        pdf: &dyn PdfBackend,
    ) -> AppResult<LoopStep>
    where
        S: TerminalSurface,
    {
        let step = self.build_loop_step(
            &runtime.session,
            pdf,
            &runtime.input_actor,
            runtime.render_actor.generation() == 0,
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
                current_scale: step.current_scale,
                required_pages: step.required_pages.clone(),
                required_keys: step.required_render_keys.clone(),
                current_interest_keys: step.current_interest_keys.clone(),
                current_cached: step.current_cached,
                preview_tasks: step
                    .initial_preview
                    .as_ref()
                    .map(|plan| {
                        plan.page_keys
                            .iter()
                            .enumerate()
                            .map(|(idx, key)| RenderTask {
                                doc_id: key.doc_id,
                                page: key.page,
                                scale: key.scale_milli as f32 / 1000.0,
                                class: WorkClass::CriticalCurrent,
                                generation: runtime.render_actor.generation(),
                                reason: if idx == 0 {
                                    "initial-preview"
                                } else {
                                    "initial-preview-spread"
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        );
        self.render.dispatch_prefetch_if_due(
            &mut self.state,
            &mut runtime.render_actor,
            &mut runtime.render_worker,
            PrefetchDispatchContext {
                required_keys: step.required_render_keys.clone(),
                current_cached: step.current_cached,
                overlay_stamp: step.overlay_stamp,
                prefetch_viewport: step.prefetch_viewport,
                base_pan: step.base_pan,
                enable_crop: step.enable_crop,
                interactive: step.interactive,
                dispatch_budget: self.config.render.prefetch_dispatch_budget_per_tick,
            },
        );
        self.update_ui_and_render_frame(runtime, pdf, changed, &step)?;
        Ok(step)
    }

    async fn wait_and_handle_next_event<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        step: &LoopStep,
        pdf: SharedPdfBackend,
    ) -> AppResult<LoopControl>
    where
        S: TerminalSurface + SessionRestore,
    {
        let render_busy = runtime.render_worker.in_flight_len() > 0;
        let presenter_busy = self.render.presenter.has_pending_work();
        let prefetch_pending = self.render.runtime.has_prefetch_work();
        let wait_for_pending_redraw = runtime.ui_actor.should_wait_for_pending_redraw(
            step.current_cached,
            render_busy,
            presenter_busy,
        );
        let wake_timeout = select_input_poll_timeout(
            render_busy,
            presenter_busy,
            prefetch_pending,
            runtime.input_poll_timeout_idle,
            runtime.input_poll_timeout_busy,
        );
        let waited = wait_next_event(
            &mut runtime.loop_event_rx,
            &mut runtime.render_worker,
            &mut *self.render.presenter,
            &mut runtime.prefetch_tick,
            &mut runtime.redraw_tick,
            wait_for_pending_redraw,
            wake_timeout,
        )
        .await;
        self.handle_waited_event(waited, runtime, pdf)
    }

    fn build_loop_step(
        &mut self,
        session: &impl TerminalSurface,
        pdf: &dyn PdfBackend,
        input_actor: &InputActor,
        is_cold_start: bool,
        prefetch_pause_after_input: Duration,
    ) -> LoopStep {
        let prefetch_viewport = Self::current_viewport(session, self.state.debug_status_visible);
        let visible_pages = self.state.visible_page_slots(pdf.page_count());
        let current_scale =
            self.compute_current_scale(pdf, visible_pages.anchor_page, prefetch_viewport);
        let overlay_stamp = self
            .interaction
            .extensions
            .host
            .highlight_overlay_for(visible_pages.existing_pages())
            .stamp;
        let base_pan = self.current_pan();
        let enable_crop = self.state.zoom > 1.0;
        let interactive = input_actor.is_interactive(prefetch_pause_after_input);
        let mut required_pages = vec![visible_pages.anchor_page];
        if let Some(trailing_page) = visible_pages.trailing_page {
            required_pages.push(trailing_page);
        }
        let required_render_keys = required_pages
            .iter()
            .map(|page| RenderedPageKey::new(pdf.doc_id(), *page, current_scale))
            .collect::<Vec<_>>();
        let current_cached = required_render_keys
            .iter()
            .all(|key| self.render.runtime.has_cached_frame(key));
        let presenter_layout_tag = self
            .state
            .presenter_layout_tag(visible_pages.trailing_page.is_some());
        let initial_preview = cold_start_initial_preview_plan(
            is_cold_start,
            current_cached,
            pdf.doc_id(),
            visible_pages,
            self.state.page_layout_mode,
            current_scale,
            presenter_layout_tag,
        );
        let mut current_interest_keys = required_render_keys.clone();
        if let Some(preview_plan) = initial_preview.as_ref() {
            current_interest_keys.extend(preview_plan.page_keys.iter().copied());
        }
        let presenter_key = RenderedPageKey::with_layout(
            pdf.doc_id(),
            visible_pages.anchor_page,
            current_scale,
            presenter_layout_tag,
        );

        LoopStep {
            current_scale,
            overlay_stamp,
            prefetch_viewport,
            base_pan,
            enable_crop,
            interactive,
            visible_pages,
            required_pages,
            required_render_keys,
            current_interest_keys,
            initial_preview,
            presenter_key,
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
        let previous_page = self.state.current_page;
        self.state.normalize_current_page(pdf.page_count());
        if self.state.current_page != previous_page {
            changed = true;
        }
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

    fn update_ui_and_render_frame<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        pdf: &dyn PdfBackend,
        changed: bool,
        step: &LoopStep,
    ) -> AppResult<()>
    where
        S: TerminalSurface,
    {
        let render_busy = runtime.render_worker.in_flight_len() > 0;
        let presenter_busy = self.render.presenter.has_pending_work();
        if runtime.ui_actor.should_request_pending_redraw(
            step.current_cached,
            render_busy,
            presenter_busy,
        ) {
            self.request_redraw(runtime, RedrawReason::PendingWork);
        }

        if changed {
            self.request_redraw(runtime, RedrawReason::StateChanged);
        }

        if runtime.ui_actor.needs_redraw() {
            let palette_view = self.interaction.palette_view();
            let mut status_bar_segments = self
                .interaction
                .extensions
                .host
                .status_bar_segments(&self.state);
            if let Some(pending_sequence) = self.interaction.pending_sequence_status() {
                status_bar_segments.push(pending_sequence);
            }
            self.render.render_frame(
                &mut self.state,
                &self.config,
                &mut runtime.session,
                pdf,
                RenderFramePlan {
                    palette_view,
                    help_keymap: self.interaction.sequences.resolver.snapshot(),
                    status_bar_segments,
                    page_count: runtime.page_count,
                    visible_pages: step.visible_pages,
                    current_scale: step.current_scale,
                    initial_preview: step.initial_preview.clone(),
                    presenter_key: step.presenter_key,
                    highlight_overlay: self
                        .interaction
                        .extensions
                        .host
                        .highlight_overlay_for(step.visible_pages.existing_pages()),
                    generation: runtime.render_actor.generation(),
                    nav_streak: runtime.render_actor.nav_streak(),
                },
            )?;
            runtime.ui_actor.clear_redraw();
            if !step.current_cached {
                runtime.ui_actor.on_drawn_non_cached_page();
            }
        }
        Ok(())
    }

    fn handle_waited_event<S>(
        &mut self,
        waited: WaitEvent,
        runtime: &mut LoopRuntime<S>,
        pdf: SharedPdfBackend,
    ) -> AppResult<LoopControl>
    where
        S: TerminalSurface + SessionRestore,
    {
        // Wake events are not guaranteed to arrive before the next input event, so the
        // loop checks for timed-out sequences at the start of every iteration as well.
        let timeout_outcome = self.interaction.flush_sequence_timeout(self.state.mode);
        if timeout_outcome.clear_terminal {
            runtime.session.clear()?;
        }
        if timeout_outcome.redraw {
            self.request_redraw(runtime, RedrawReason::Input);
        }
        if matches!(
            Self::enqueue_commands_and_optional_quit(
                runtime,
                timeout_outcome.commands,
                timeout_outcome.quit_requested,
            ),
            LoopControl::Break
        ) {
            return Ok(LoopControl::Break);
        }

        match waited {
            WaitEvent::Event(DomainEvent::Input(event)) => {
                let input_outcome = self.handle_input_event(
                    event,
                    &mut runtime.session,
                    runtime.ui_actor.needs_redraw_mut(),
                    runtime.input_actor.last_input_at_mut(),
                )?;
                if input_outcome.redraw_requested {
                    self.request_redraw(runtime, RedrawReason::Input);
                }
                if matches!(
                    Self::enqueue_commands_and_optional_quit(
                        runtime,
                        input_outcome.commands,
                        input_outcome.quit_requested,
                    ),
                    LoopControl::Break
                ) {
                    return Ok(LoopControl::Break);
                }
            }
            WaitEvent::Event(DomainEvent::InputError(message)) => {
                self.state
                    .set_error_notice(format!("input error: {message}"));
                self.request_redraw(runtime, RedrawReason::InputError);
            }
            WaitEvent::Event(DomainEvent::Command(request)) => {
                let request = self.resolve_command_request(&runtime.session, request);
                let dispatch = match self.interaction.dispatch_command(
                    &mut self.state,
                    request,
                    Arc::clone(&pdf),
                ) {
                    Ok(dispatch) => dispatch,
                    Err(err) => {
                        self.state.apply_notice_action(notice_action_for_error(err));
                        self.request_redraw(runtime, RedrawReason::Command);
                        return Ok(LoopControl::Continue);
                    }
                };
                for event in dispatch.emitted_events {
                    let _ = runtime.loop_event_tx.send(DomainEvent::App(event));
                }
                if self.interaction.apply_palette_requests(&mut self.state) {
                    self.request_redraw(runtime, RedrawReason::StateChanged);
                }
                match dispatch.outcome {
                    CommandOutcome::QuitRequested => {
                        Self::terminate_process_now(runtime);
                    }
                    CommandOutcome::Applied | CommandOutcome::Noop => {
                        self.request_redraw(runtime, RedrawReason::Command)
                    }
                }
            }
            WaitEvent::Event(DomainEvent::App(event)) => {
                self.interaction.handle_app_event(&mut self.state, &event);
                self.request_redraw(runtime, RedrawReason::AppEvent);
            }
            WaitEvent::Event(DomainEvent::RenderComplete(completed)) => {
                let viewport =
                    Self::current_viewport(&runtime.session, self.state.debug_status_visible);
                let visible_pages = self.state.visible_page_slots(pdf.page_count());
                let scale =
                    self.compute_current_scale(pdf.as_ref(), visible_pages.anchor_page, viewport);
                let mut current_keys = vec![RenderedPageKey::new(
                    pdf.doc_id(),
                    visible_pages.anchor_page,
                    scale,
                )];
                if let Some(trailing_page) = visible_pages.trailing_page {
                    current_keys.push(RenderedPageKey::new(pdf.doc_id(), trailing_page, scale));
                }
                if let Some(preview_plan) = cold_start_initial_preview_plan(
                    runtime.render_actor.generation() == 0,
                    current_keys
                        .iter()
                        .all(|key| self.render.runtime.has_cached_frame(key)),
                    pdf.doc_id(),
                    visible_pages,
                    self.state.page_layout_mode,
                    scale,
                    self.state
                        .presenter_layout_tag(visible_pages.trailing_page.is_some()),
                ) {
                    current_keys.extend(preview_plan.page_keys);
                }
                let pan = self.current_pan();
                let enable_crop = self.state.zoom > 1.0;
                if self.render.process_render_result(
                    &mut self.state,
                    completed,
                    &current_keys,
                    viewport,
                    pan,
                    enable_crop,
                    runtime
                        .input_actor
                        .is_interactive(runtime.prefetch_pause_after_input),
                ) {
                    self.request_redraw(runtime, RedrawReason::RenderComplete);
                }
                self.render
                    .runtime
                    .set_queue_depth_with_inflight(runtime.render_worker.in_flight_len());
            }
            WaitEvent::Event(DomainEvent::EncodeComplete(
                PresenterBackgroundEvent::EncodeComplete { redraw_requested },
            )) => {
                if redraw_requested {
                    self.request_redraw(runtime, RedrawReason::RenderComplete);
                }
            }
            WaitEvent::Event(DomainEvent::PrefetchTick) => {
                runtime.render_actor.mark_prefetch_due();
            }
            WaitEvent::Event(DomainEvent::RedrawTick) => {
                self.request_redraw(runtime, RedrawReason::Timer);
            }
            WaitEvent::Event(DomainEvent::Quit) => {
                Self::terminate_process_now(runtime);
            }
            WaitEvent::Event(DomainEvent::Wake) => {}
            WaitEvent::Closed => return Ok(LoopControl::Break),
        }
        Ok(LoopControl::Continue)
    }

    fn request_redraw<S>(&mut self, runtime: &mut LoopRuntime<S>, reason: RedrawReason)
    where
        S: TerminalSurface,
    {
        runtime.ui_actor.mark_redraw();
        self.render.runtime.perf_stats.record_redraw(reason);
    }
}

fn cold_start_initial_preview_plan(
    is_cold_start: bool,
    current_cached: bool,
    doc_id: u64,
    visible_pages: super::state::VisiblePageSlots,
    page_layout_mode: super::state::PageLayoutMode,
    current_scale: f32,
    presenter_layout_tag: u16,
) -> Option<InitialPreviewPlan> {
    if !is_cold_start || current_cached {
        return None;
    }

    compute_initial_preview_plan(
        doc_id,
        visible_pages,
        page_layout_mode,
        current_scale,
        presenter_layout_tag,
    )
}

async fn wait_next_event(
    loop_event_rx: &mut UnboundedReceiver<DomainEvent>,
    render_worker: &mut RenderWorker,
    presenter: &mut dyn ImagePresenter,
    prefetch_tick: &mut time::Interval,
    redraw_tick: &mut time::Interval,
    wait_for_pending_redraw: bool,
    wake_timeout: Duration,
) -> WaitEvent {
    if presenter.has_pending_work() {
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
            maybe_presenter = presenter.recv_background_event() => {
                match maybe_presenter {
                    Some(event) => WaitEvent::Event(DomainEvent::EncodeComplete(event)),
                    None => WaitEvent::Event(DomainEvent::Wake),
                }
            },
            _ = prefetch_tick.tick() => {
                WaitEvent::Event(DomainEvent::PrefetchTick)
            },
            _ = redraw_tick.tick(), if wait_for_pending_redraw => {
                WaitEvent::Event(DomainEvent::RedrawTick)
            },
            _ = time::sleep(wake_timeout) => {
                WaitEvent::Event(DomainEvent::Wake)
            }
        }
    } else {
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
            _ = redraw_tick.tick(), if wait_for_pending_redraw => {
                WaitEvent::Event(DomainEvent::RedrawTick)
            },
            _ = time::sleep(wake_timeout) => {
                WaitEvent::Event(DomainEvent::Wake)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::future::Future;
    use std::io;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::process;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Size;
    use tokio::runtime::Builder;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{WaitEvent, cold_start_initial_preview_plan, wait_next_event};
    use crate::app::App;
    use crate::app::core::InteractionSubsystem;
    use crate::app::state::{PageLayoutMode, VisiblePageSlots};
    use crate::app::terminal_session::TerminalSurface;
    use crate::backend::{PdfDoc, SharedPdfBackend};
    use crate::command::{
        Command, CommandInvocationSource, CommandRequest, PanAmount, PanDirection,
    };
    use crate::config::Config;
    use crate::event::DomainEvent;
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;
    use crate::presenter::PresenterKind;
    use crate::presenter::{
        ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterCaps, PresenterFeedback,
        PresenterRenderOptions, PresenterRenderOutcome, PresenterRuntimeInfo, Viewport,
    };
    use crate::render::cache::RenderedPageKey;
    use crate::render::worker::RenderWorker;

    #[derive(Default)]
    struct StubPresenter {
        events: VecDeque<Option<PresenterBackgroundEvent>>,
    }

    impl StubPresenter {
        fn with_events(events: impl IntoIterator<Item = Option<PresenterBackgroundEvent>>) -> Self {
            Self {
                events: events.into_iter().collect(),
            }
        }
    }

    impl ImagePresenter for StubPresenter {
        fn prepare(
            &mut self,
            _cache_key: RenderedPageKey,
            _frame: &crate::backend::RgbaFrame,
            _viewport: Viewport,
            _pan: PanOffset,
            _overlay_stamp: u64,
            _generation: u64,
        ) -> crate::error::AppResult<()> {
            Ok(())
        }

        fn render(
            &mut self,
            _frame: &mut ratatui::Frame<'_>,
            _area: ratatui::layout::Rect,
            _options: PresenterRenderOptions,
        ) -> crate::error::AppResult<PresenterRenderOutcome> {
            Ok(PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
            })
        }

        fn capabilities(&self) -> PresenterCaps {
            PresenterCaps {
                backend_name: "stub",
                supports_l2_cache: false,
                cell_px: None,
                preferred_max_render_scale: 1.0,
            }
        }

        fn runtime_info(&self) -> PresenterRuntimeInfo {
            PresenterRuntimeInfo::default()
        }

        fn has_pending_work(&self) -> bool {
            !self.events.is_empty()
        }

        fn recv_background_event<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = Option<PresenterBackgroundEvent>> + 'a>> {
            let event = self.events.pop_front().flatten();
            Box::pin(async move { event })
        }
    }

    struct StubSession {
        size: Size,
        clear_count: usize,
    }

    impl StubSession {
        fn new(width: u16, height: u16) -> Self {
            Self {
                size: Size::new(width, height),
                clear_count: 0,
            }
        }
    }

    impl TerminalSurface for StubSession {
        fn size(&self) -> io::Result<Size> {
            Ok(self.size)
        }

        fn clear(&mut self) -> io::Result<()> {
            self.clear_count += 1;
            Ok(())
        }

        fn draw<F>(&mut self, _render: F) -> io::Result<()>
        where
            F: FnOnce(&mut ratatui::Frame<'_>),
        {
            Ok(())
        }
    }

    impl super::SessionRestore for StubSession {
        fn restore(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("pvf-event-loop-{}-{nanos}{suffix}", process::id()))
    }

    fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
        let page_texts = if page_texts.is_empty() {
            vec!["".to_string()]
        } else {
            page_texts
                .iter()
                .map(|text| format!("BT /F1 14 Tf 36 260 Td ({text}) Tj ET"))
                .collect()
        };

        let page_count = page_texts.len();
        let page_ids: Vec<usize> = (0..page_count).map(|i| 4 + i * 2).collect();

        let mut objects = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!(
            "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
        ));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

        for (index, stream) in page_texts.iter().enumerate() {
            let content_id = 5 + index * 2;
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            ));
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = vec![0_usize];
        for (index, object) in objects.iter().enumerate() {
            let object_id = index + 1;
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{object_id} 0 obj\n{object}\nendobj\n").as_bytes());
        }

        let xref_start = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_start
            )
            .as_bytes(),
        );
        bytes
    }

    fn idle_render_worker() -> RenderWorker {
        let file = unique_temp_path(".pdf");
        fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");
        fs::remove_file(&file).expect("test pdf should be removed");
        let shared: SharedPdfBackend = Arc::new(doc);
        RenderWorker::spawn(shared, 1)
    }

    fn test_pdf_backend() -> SharedPdfBackend {
        let file = unique_temp_path(".pdf");
        fs::write(&file, build_pdf(&["page"])).expect("test pdf should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");
        fs::remove_file(&file).expect("test pdf should be removed");
        Arc::new(doc)
    }

    #[test]
    fn resolve_command_uses_short_edge_fifth_for_default_pan_step() {
        let app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let session = StubSession::new(80, 24);
        let short_edge_cells = 23_u16;
        let expected_step = i32::from((short_edge_cells / 5).max(1));

        let resolved = app.resolve_command(
            &session,
            Command::Pan {
                direction: PanDirection::Right,
                amount: PanAmount::DefaultStep,
            },
        );

        assert_eq!(
            resolved,
            Command::Pan {
                direction: PanDirection::Right,
                amount: PanAmount::Cells(expected_step),
            }
        );
    }

    #[test]
    fn resolve_command_clamps_default_pan_step_to_at_least_one_cell() {
        let app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let session = StubSession::new(20, 5);

        let resolved = app.resolve_command(
            &session,
            Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            },
        );

        assert_eq!(
            resolved,
            Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::Cells(1),
            }
        );
    }

    #[test]
    fn resolve_command_request_preserves_explicit_pan_amounts() {
        let app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let session = StubSession::new(80, 24);

        let resolved = app.resolve_command_request(
            &session,
            CommandRequest::new(
                Command::Pan {
                    direction: PanDirection::Left,
                    amount: PanAmount::Cells(3),
                },
                crate::command::CommandInvocationSource::CommandPaletteInput,
            ),
        );

        assert_eq!(
            resolved.command,
            Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::Cells(3),
            }
        );
    }

    #[test]
    fn cold_start_initial_preview_plan_is_disabled_after_first_image() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(true, true, 7, slots, PageLayoutMode::Single, 1.0, 0);

        assert_eq!(preview, None);
    }

    #[test]
    fn cold_start_initial_preview_plan_is_available_before_first_image() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(true, false, 7, slots, PageLayoutMode::Single, 1.0, 0);

        assert!(preview.is_some());
    }

    #[test]
    fn cold_start_initial_preview_plan_stays_available_until_current_frame_is_cached() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(true, false, 7, slots, PageLayoutMode::Single, 1.0, 0);

        assert!(preview.is_some());
    }

    #[test]
    fn cold_start_initial_preview_plan_is_disabled_after_navigation_begins() {
        let slots = VisiblePageSlots {
            anchor_page: 0,
            trailing_page: None,
            left_page: Some(0),
            right_page: None,
        };

        let preview =
            cold_start_initial_preview_plan(false, false, 7, slots, PageLayoutMode::Single, 1.0, 0);

        assert_eq!(preview, None);
    }

    #[test]
    fn wait_next_event_maps_presenter_event_then_eof_to_wake() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let mut render_worker = idle_render_worker();
        let mut presenter = StubPresenter::with_events([
            Some(PresenterBackgroundEvent::EncodeComplete {
                redraw_requested: true,
            }),
            None,
        ]);
        let (_tx, mut loop_event_rx) = unbounded_channel();
        runtime.block_on(async {
            let mut prefetch_tick = tokio::time::interval(Duration::from_secs(60));
            let mut redraw_tick = tokio::time::interval(Duration::from_secs(60));

            let first = wait_next_event(
                &mut loop_event_rx,
                &mut render_worker,
                &mut presenter,
                &mut prefetch_tick,
                &mut redraw_tick,
                false,
                Duration::from_secs(60),
            )
            .await;
            assert!(matches!(
                first,
                WaitEvent::Event(DomainEvent::EncodeComplete(
                    PresenterBackgroundEvent::EncodeComplete {
                        redraw_requested: true
                    }
                ))
            ));

            let second = wait_next_event(
                &mut loop_event_rx,
                &mut render_worker,
                &mut presenter,
                &mut prefetch_tick,
                &mut redraw_tick,
                false,
                Duration::from_secs(60),
            )
            .await;
            assert!(matches!(second, WaitEvent::Event(DomainEvent::Wake)));
        });
    }

    #[test]
    fn wake_timeout_clears_terminal_when_sequence_dispatch_requests_it() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::OpenHelp)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        app.interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);

        app.interaction
            .handle_key_event(
                &mut app.state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&pdf),
                pdf.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Wake),
                &mut runtime,
                Arc::clone(&pdf),
            )
            .expect("wake should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert_eq!(runtime.session.clear_count, 1);
        assert!(matches!(
            runtime.loop_event_rx.try_recv(),
            Ok(DomainEvent::Command(request))
                if request
                    == CommandRequest::new(Command::OpenHelp, CommandInvocationSource::Keymap)
        ));
    }

    #[test]
    fn input_outcome_queues_commands_before_quit_event() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('q')], Command::Quit)
            .expect("single-key binding should register");
        app.interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);

        app.interaction
            .handle_key_event(
                &mut app.state,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            )
            .expect("first key should be captured");

        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&pdf),
                pdf.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                    KeyCode::Char('q'),
                    KeyModifiers::NONE,
                )))),
                &mut runtime,
                Arc::clone(&pdf),
            )
            .expect("input should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert!(matches!(
            runtime.loop_event_rx.try_recv(),
            Ok(DomainEvent::Command(request))
                if request
                    == CommandRequest::new(Command::FirstPage, CommandInvocationSource::Keymap)
        ));
        assert!(matches!(
            runtime.loop_event_rx.try_recv(),
            Ok(DomainEvent::Quit)
        ));
    }

    #[test]
    fn command_error_becomes_notice_and_loop_continues() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&pdf),
                pdf.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                    Command::GotoPage { page: 999 },
                    CommandInvocationSource::Keymap,
                ))),
                &mut runtime,
                Arc::clone(&pdf),
            )
            .expect("command error should be handled as a notice");

        assert!(matches!(control, super::LoopControl::Continue));
        let notice = app.state.notice.expect("command error should set a notice");
        assert_eq!(notice.level, crate::app::NoticeLevel::Warning);
        assert_eq!(notice.message, "page 999 is out of range (1-1)");
        assert!(runtime.loop_event_rx.try_recv().is_err());
    }
}
