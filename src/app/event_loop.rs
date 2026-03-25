use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{self, MissedTickBehavior};

use crate::backend::{PdfBackend, SharedPdfBackend};
use crate::command::CommandOutcome;
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::perf::{PerfIterationSnapshot, PerfScenarioId, RedrawReason};
use crate::presenter::{PanOffset, Viewport};
use crate::render::cache::RenderedPageKey;
use crate::render::scheduler::{RenderPriority, RenderTask};
use crate::render::worker::RenderWorker;

use super::actors::{InputActor, RenderActor, UiActor};
use super::core::App;
use super::event_bus::EventBusRuntime;
use super::perf_runner::{
    HeadlessTerminalSession, PERF_HEADLESS_HEIGHT, PERF_HEADLESS_WIDTH, PerfLoopDriver,
};
use super::render_ops::{CurrentTaskContext, PrefetchDispatchContext};
use super::scale::select_input_poll_timeout;
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
                                priority: RenderPriority::CriticalCurrent,
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
            let status_bar_segments = self
                .interaction
                .extensions
                .host
                .status_bar_segments(&self.state);
            self.render.render_frame(
                &mut self.state,
                &self.config,
                &mut runtime.session,
                pdf,
                RenderFramePlan {
                    palette_view,
                    status_bar_segments,
                    page_count: runtime.page_count,
                    visible_pages: step.visible_pages,
                    current_scale: step.current_scale,
                    initial_preview: step.initial_preview.clone(),
                    presenter_key: step.presenter_key,
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
                if input_outcome.redraw_requested {
                    self.request_redraw(runtime, RedrawReason::Input);
                }
                if let Some(request) = input_outcome.command {
                    let _ = runtime.loop_event_tx.send(DomainEvent::Command(request));
                }
            }
            WaitEvent::Event(DomainEvent::InputError(message)) => {
                self.state
                    .set_error_notice(format!("input error: {message}"));
                self.request_redraw(runtime, RedrawReason::InputError);
            }
            WaitEvent::Event(DomainEvent::Command(request)) => {
                let dispatch = self.interaction.dispatch_command(
                    &mut self.state,
                    request,
                    Arc::clone(&pdf),
                )?;
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
            WaitEvent::Event(DomainEvent::PrefetchTick) => {
                runtime.render_actor.mark_prefetch_due();
            }
            WaitEvent::Event(DomainEvent::RedrawTick) => {
                self.request_redraw(runtime, RedrawReason::Timer);
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
    prefetch_tick: &mut time::Interval,
    redraw_tick: &mut time::Interval,
    wait_for_pending_redraw: bool,
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
        _ = redraw_tick.tick(), if wait_for_pending_redraw => {
            WaitEvent::Event(DomainEvent::RedrawTick)
        },
        _ = time::sleep(wake_timeout) => {
            WaitEvent::Event(DomainEvent::Wake)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cold_start_initial_preview_plan;
    use crate::app::state::{PageLayoutMode, VisiblePageSlots};

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
}
