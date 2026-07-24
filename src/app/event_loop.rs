use std::sync::Arc;
use std::time::Duration;

use crate::backend::{PdfBackend, SharedPdfBackend};
use crate::error::{AppError, AppResult};

use super::actors::InputActor;
use super::core::{App, RunOptions};
use super::event_bus::EventBusRuntime;
use super::render_ops::PrefetchDispatchPlan;
use super::runtime::{
    ActiveDocument, AppRuntime, IterationStep, RestoringSession, RuntimeControl,
    RuntimeEventSources, wait_next_event,
};
use super::runtime_driver::{
    InteractiveRuntimeDriver, RuntimeDriver, RuntimeDriverDecision, RuntimeDriverHandle,
    RuntimeMetricsSnapshot, RuntimeMode, RuntimeObservation,
};
use super::scale::select_input_poll_timeout;
use super::terminal_session::{InteractiveTerminalSession, TerminalSession, TerminalSurface};

impl App {
    pub async fn run(&mut self, pdf: SharedPdfBackend) -> AppResult<()> {
        self.run_with_options(pdf, self.run_options()).await
    }

    pub async fn run_with_options(
        &mut self,
        pdf: SharedPdfBackend,
        options: RunOptions,
    ) -> AppResult<()> {
        let session = InteractiveTerminalSession::enter()?;
        self.run_event_runtime(
            pdf,
            session,
            RuntimeMode::Interactive {
                watch: options.watch,
            },
            InteractiveRuntimeDriver,
        )
        .await
    }

    pub(crate) async fn run_event_runtime<S, D>(
        &mut self,
        pdf: SharedPdfBackend,
        session: S,
        event_mode: RuntimeMode,
        mut driver: D,
    ) -> AppResult<D::Output>
    where
        S: TerminalSession,
        D: RuntimeDriver,
    {
        let session = RestoringSession::new(session);
        let page_count = pdf.page_count();
        if page_count == 0 {
            return Err(AppError::invalid_argument("pdf has no pages"));
        }

        let mut document = ActiveDocument::new(pdf);
        let (event_tx, event_rx, event_bus) = match event_mode {
            RuntimeMode::Interactive { .. } => EventBusRuntime::spawn_interactive(),
            RuntimeMode::Headless => EventBusRuntime::spawn_headless(),
        };
        let mut runtime = self.initialize_runtime(
            Arc::clone(&document.pdf),
            page_count,
            session,
            event_tx,
            event_rx,
            event_bus,
        )?;
        if let RuntimeMode::Interactive { watch } = event_mode {
            runtime.event_bus.start_input(runtime.event_tx.clone());
            if watch {
                runtime.event_bus.start_file_watch(
                    document.path.clone(),
                    self.watch_policy.poll_interval,
                    self.watch_policy.settle_delay,
                    runtime.event_tx.clone(),
                );
            }
        }

        let result = self
            .drive_runtime(&mut runtime, &mut document, &mut driver)
            .await;
        runtime.event_bus.shutdown();
        let restore_result = runtime.session.restore();
        match (result, restore_result) {
            (Ok(output), Ok(())) => Ok(output),
            (Ok(_), Err(source)) => Err(AppError::io_with_context(
                source,
                "restoring terminal session",
            )),
            (Err(err), _) => Err(err),
        }
    }

    async fn drive_runtime<S, D>(
        &mut self,
        runtime: &mut AppRuntime<S>,
        document: &mut ActiveDocument,
        driver: &mut D,
    ) -> AppResult<D::Output>
    where
        S: TerminalSession,
        D: RuntimeDriver,
    {
        loop {
            let step = self.process_runtime_iteration(runtime, document.pdf.as_ref())?;
            let observation = self.driver_observation(runtime, &step);
            let mut handle = RuntimeDriverHandle::new(&runtime.event_tx);
            match driver.on_iteration(observation, &mut handle)? {
                RuntimeDriverDecision::Continue => {}
                RuntimeDriverDecision::Finish => {
                    return driver.on_finish(observation, self.driver_metrics_snapshot());
                }
            }

            match self
                .wait_and_handle_next_event(runtime, &step, document)
                .await?
            {
                RuntimeControl::Continue => {}
                RuntimeControl::Break => return driver.on_break(),
            }
        }
    }

    fn driver_observation<S>(
        &self,
        runtime: &AppRuntime<S>,
        step: &IterationStep,
    ) -> RuntimeObservation {
        let render_in_flight = runtime.render_worker.in_flight_len();
        let presenter_pending = self.render.presenter.has_pending_work();
        let redraw_pending = runtime.ui_actor.needs_redraw();
        let event_queue_empty =
            runtime.event_rx.is_empty() && runtime.extension_worker_rx.is_empty();
        RuntimeObservation {
            page_count: runtime.page_count,
            current_page: self.state.current_page,
            current_cached: step.current_cached,
            render_in_flight,
            presenter_pending,
            redraw_pending,
            event_queue_empty,
            system_idle: step.current_cached
                && render_in_flight == 0
                && !presenter_pending
                && !redraw_pending
                && event_queue_empty,
        }
    }

    fn driver_metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        RuntimeMetricsSnapshot {
            runtime: self.render.runtime.perf_stats.clone(),
            presenter: self.render.presenter.perf_snapshot().unwrap_or_default(),
        }
    }

    fn process_runtime_iteration<S>(
        &mut self,
        runtime: &mut AppRuntime<S>,
        pdf: &dyn PdfBackend,
    ) -> AppResult<IterationStep>
    where
        S: TerminalSurface,
    {
        let pre_sync_step = self.build_iteration_step(
            &runtime.session,
            pdf,
            &runtime.input_actor,
            runtime.render_actor.generation(),
            runtime.prefetch_pause_after_input,
            self.event_loop_policy.prefetch_dispatch_budget_per_tick,
        );
        let changed = runtime.render_actor.drain_background_and_sync_navigation(
            &mut self.render,
            &mut self.interaction,
            &mut self.state,
            pdf,
            pre_sync_step.current_scale,
        );
        let step = if changed {
            self.build_iteration_step(
                &runtime.session,
                pdf,
                &runtime.input_actor,
                runtime.render_actor.generation(),
                runtime.prefetch_pause_after_input,
                self.event_loop_policy.prefetch_dispatch_budget_per_tick,
            )
        } else {
            pre_sync_step
        };
        runtime.render_actor.ensure_iteration_work(
            &mut self.render,
            &mut self.state,
            pdf,
            &mut runtime.render_worker,
            &step,
        );
        self.update_ui_and_render_frame(runtime, pdf, changed, &step)?;
        Ok(step)
    }

    async fn wait_and_handle_next_event<S>(
        &mut self,
        runtime: &mut AppRuntime<S>,
        step: &IterationStep,
        document: &mut ActiveDocument,
    ) -> AppResult<RuntimeControl>
    where
        S: TerminalSession,
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
            RuntimeEventSources {
                event_rx: &mut runtime.event_rx,
                extension_worker_rx: &mut runtime.extension_worker_rx,
                render_worker: &mut runtime.render_worker,
                presenter: &mut *self.render.presenter,
                prefetch_tick: &mut runtime.prefetch_tick,
                redraw_tick: &mut runtime.redraw_tick,
            },
            wait_for_pending_redraw,
            wake_timeout,
        )
        .await;
        self.handle_waited_event(waited, runtime, document)
    }

    fn build_iteration_step(
        &mut self,
        session: &impl TerminalSurface,
        pdf: &dyn PdfBackend,
        input_actor: &InputActor,
        render_generation: u64,
        prefetch_pause_after_input: Duration,
        prefetch_dispatch_budget: usize,
    ) -> IterationStep {
        let prefetch_viewport = Self::current_viewport(session, self.state.debug_status_visible);
        let visible_pages = self.state.visible_page_slots(pdf.page_count());
        let current_scale =
            self.compute_current_scale(pdf, visible_pages.anchor_page, prefetch_viewport);
        let current_view = self.render.build_current_render_view(
            &self.state,
            pdf,
            visible_pages,
            current_scale,
            render_generation == 0,
        );
        let overlay_stamp = self
            .interaction
            .extensions
            .host
            .render_snapshot(current_view.visible_pages.existing_pages())
            .highlight_overlay
            .stamp;
        let base_pan = self.current_pan();
        let interactive = input_actor.is_interactive(prefetch_pause_after_input);
        let prefetch_dispatch = current_view.prefetch_dispatch_context(
            &self.state,
            PrefetchDispatchPlan {
                overlay_stamp,
                prefetch_viewport,
                base_pan,
                interactive,
                dispatch_budget: prefetch_dispatch_budget,
            },
        );

        IterationStep {
            current_scale: current_view.current_scale,
            visible_pages: current_view.visible_pages,
            required: current_view.required,
            current_interest_keys: current_view.current_interest_keys,
            initial_preview_tasks: current_view.preview_tasks(render_generation),
            prefetch_dispatch,
            initial_preview: current_view.initial_preview,
            presenter_key: current_view.presenter_key,
            current_cached: current_view.current_cached,
        }
    }

    fn update_ui_and_render_frame<S>(
        &mut self,
        runtime: &mut AppRuntime<S>,
        pdf: &dyn PdfBackend,
        changed: bool,
        step: &IterationStep,
    ) -> AppResult<()>
    where
        S: TerminalSurface,
    {
        let render_busy = runtime.render_worker.in_flight_len() > 0;
        let presenter_busy = self.render.presenter.has_pending_work();
        runtime.ui_actor.update_and_render_frame(
            &mut self.render,
            &self.interaction,
            &mut self.state,
            &mut runtime.session,
            pdf,
            runtime.page_count,
            runtime.render_actor.generation(),
            runtime.render_actor.nav_streak(),
            render_busy,
            presenter_busy,
            changed,
            step,
        )
    }
}
