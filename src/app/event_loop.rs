use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{self, MissedTickBehavior};

use crate::backend::{PdfBackend, SharedPdfBackend};
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::perf::{PerfIterationSnapshot, PerfScenarioId, PerfScenarioParameters};
use crate::presenter::ImagePresenter;
use crate::render::worker::RenderWorker;

use super::actors::{InputActor, RenderActor, UiActor};
use super::core::App;
use super::event_bus::EventBusRuntime;
use super::loop_runtime::{LoopControl, LoopRuntime, LoopStep, SessionRestore, WaitEvent};
use super::perf_runner::{
    HeadlessTerminalSession, PERF_HEADLESS_HEIGHT, PERF_HEADLESS_WIDTH, PerfLoopDriver,
};
use super::render_ops::PrefetchDispatchPlan;
use super::scale::select_input_poll_timeout;
use super::terminal_session::{InteractiveTerminalSession, TerminalSurface};

impl App {
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
        parameters: PerfScenarioParameters,
        cold_started_at: Instant,
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
        let result = self
            .run_perf_loop(&mut runtime, pdf, scenario, parameters, cold_started_at)
            .await;
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
        self.interaction.prewarm_search_text(Arc::clone(&pdf));

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
        parameters: PerfScenarioParameters,
        cold_started_at: Instant,
    ) -> AppResult<PerfIterationSnapshot> {
        let mut perf_driver = PerfLoopDriver::new(scenario, parameters, cold_started_at);

        loop {
            let step = self.process_loop_iteration(runtime, pdf.as_ref())?;
            let system_idle = step.current_cached
                && runtime.render_worker.in_flight_len() == 0
                && !self.render.presenter.has_pending_work()
                && !runtime.ui_actor.needs_redraw()
                && runtime.loop_event_rx.is_empty();
            if perf_driver.advance(
                &self.state,
                runtime.page_count,
                system_idle,
                &runtime.loop_event_tx,
            ) {
                return Ok(PerfIterationSnapshot {
                    runtime: self.render.runtime.perf_stats.clone(),
                    presenter: self.render.presenter.perf_snapshot().unwrap_or_default(),
                    wall_time: perf_driver.measured_elapsed(),
                    final_page: self.state.current_page,
                    visited_steps: perf_driver.visited_steps(),
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
        let pre_sync_step = self.build_loop_step(
            &runtime.session,
            pdf,
            &runtime.input_actor,
            runtime.render_actor.generation(),
            runtime.prefetch_pause_after_input,
            self.config.render.prefetch_dispatch_budget_per_tick,
        );
        let changed = runtime.render_actor.drain_background_and_sync_navigation(
            &mut self.render,
            &mut self.interaction,
            &mut self.state,
            pdf,
            pre_sync_step.current_scale,
        );
        let step = if changed {
            self.build_loop_step(
                &runtime.session,
                pdf,
                &runtime.input_actor,
                runtime.render_actor.generation(),
                runtime.prefetch_pause_after_input,
                self.config.render.prefetch_dispatch_budget_per_tick,
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
        render_generation: u64,
        prefetch_pause_after_input: Duration,
        prefetch_dispatch_budget: usize,
    ) -> LoopStep {
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
            .highlight_overlay_for(current_view.visible_pages.existing_pages())
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

        LoopStep {
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
        runtime.ui_actor.update_and_render_frame(
            &mut self.render,
            &self.interaction,
            &mut self.state,
            &self.config,
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
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Size;
    use tokio::runtime::Builder;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{WaitEvent, wait_next_event};
    use crate::app::core::InteractionSubsystem;
    use crate::app::terminal_session::TerminalSurface;
    use crate::app::{App, Mode, PaletteRequest};
    use crate::backend::test_support::{build_pdf, unique_temp_path};
    use crate::backend::{PdfDoc, SharedPdfBackend};
    use crate::command::{
        Command, CommandInvocationSource, CommandRequest, PanAmount, PanDirection,
    };
    use crate::config::Config;
    use crate::event::DomainEvent;
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;
    use crate::palette::PaletteKind;
    use crate::presenter::PresenterKind;
    use crate::presenter::{
        ImagePresenter, PresenterBackgroundEvent, PresenterCaps, PresenterFeedback,
        PresenterRenderOutcome, PresenterRenderSlot, PresenterRuntimeInfo, PresenterSlot,
    };
    use crate::render::cache::RenderedPageKey;
    use crate::render::worker::RenderWorker;
    use crate::render::worker::RenderWorkerResult;
    use crate::work::WorkClass;

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
        fn prepare_slots(&mut self, _slots: &[PresenterSlot<'_>]) -> crate::error::AppResult<()> {
            Ok(())
        }

        fn render_slots(
            &mut self,
            _frame: &mut ratatui::Frame<'_>,
            _slots: &[PresenterRenderSlot],
        ) -> crate::error::AppResult<PresenterRenderOutcome> {
            Ok(PresenterRenderOutcome {
                drew_image: false,
                feedback: PresenterFeedback::None,
                used_stale_fallback: false,
                slots: Vec::new(),
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

    fn failed_render_result(
        key: RenderedPageKey,
        class: WorkClass,
        generation: u64,
    ) -> RenderWorkerResult {
        RenderWorkerResult {
            key,
            class,
            generation,
            result: Err(crate::error::AppError::pdf_render(
                key.page,
                crate::error::AppError::invalid_argument("render failed"),
            )),
            queue_wait: Duration::from_millis(1),
            elapsed: Duration::from_millis(2),
        }
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
    fn cancel_command_closes_help_before_clear_and_redraw() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        app.state.mode = Mode::Help;

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
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::Cancel,
                CommandInvocationSource::Keymap,
            ))),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("cancel command should be handled");

        assert_eq!(app.state.mode, Mode::Normal);
        assert_eq!(runtime.session.clear_count, 1);
        assert!(runtime.ui_actor.needs_redraw());
    }

    #[test]
    fn cancel_command_closes_palette_before_clear_and_redraw() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        app.interaction
            .palette
            .pending_requests
            .push_back(PaletteRequest::Open {
                kind: PaletteKind::Command,
                payload: None,
            });
        assert!(app.interaction.apply_palette_requests(&mut app.state));
        assert_eq!(app.state.mode, Mode::Palette);

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
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::Cancel,
                CommandInvocationSource::Keymap,
            ))),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("cancel command should be handled");

        assert_eq!(app.state.mode, Mode::Normal);
        assert_eq!(runtime.session.clear_count, 1);
        assert!(runtime.ui_actor.needs_redraw());
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

    #[test]
    fn command_event_returns_break_when_effect_channel_is_closed() {
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
        runtime.loop_event_rx.close();

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                    Command::NextPage,
                    CommandInvocationSource::Keymap,
                ))),
                &mut runtime,
                Arc::clone(&pdf),
            )
            .expect("command should be handled");

        assert!(matches!(control, super::LoopControl::Break));
    }

    #[test]
    fn encode_complete_without_redraw_request_does_not_redraw() {
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
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::EncodeComplete(
                PresenterBackgroundEvent::EncodeComplete {
                    redraw_requested: false,
                },
            )),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("encode completion should be handled");

        assert!(!runtime.ui_actor.needs_redraw());
    }

    #[test]
    fn prefetch_tick_only_marks_prefetch_due() {
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
        assert!(runtime.render_actor.take_prefetch_due());
        assert!(!runtime.render_actor.take_prefetch_due());
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::PrefetchTick),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("prefetch tick should be handled");

        assert!(runtime.render_actor.take_prefetch_due());
        assert!(!runtime.ui_actor.needs_redraw());
        assert!(runtime.loop_event_rx.try_recv().is_err());
    }

    #[test]
    fn non_current_render_complete_does_not_redraw() {
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
        runtime.ui_actor.clear_redraw();
        let viewport = App::current_viewport(&runtime.session, app.state.debug_status_visible);
        let current_scale =
            app.compute_current_scale(pdf.as_ref(), app.state.current_page, viewport);
        let non_current_key = RenderedPageKey::new(pdf.doc_id(), 42, current_scale);

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::RenderComplete(failed_render_result(
                non_current_key,
                WorkClass::Background,
                runtime.render_actor.generation(),
            ))),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("render completion should be handled");

        assert!(!runtime.ui_actor.needs_redraw());
        assert!(app.state.notice.is_none());
    }

    #[test]
    fn noop_navigation_command_without_state_change_does_not_redraw() {
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
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::PrevPage,
                CommandInvocationSource::Keymap,
            ))),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("command should be handled");

        assert!(!runtime.ui_actor.needs_redraw());
        let event = match runtime.loop_event_rx.try_recv() {
            Ok(DomainEvent::App(event)) => event,
            other => panic!("expected command event, got {other:?}"),
        };
        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::App(event)),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("app event should be handled");
        assert!(!runtime.ui_actor.needs_redraw());
    }

    #[test]
    fn unavailable_search_navigation_without_notice_change_does_not_redraw() {
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
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::NextSearchHit,
                CommandInvocationSource::Keymap,
            ))),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("command should be handled");

        assert!(!runtime.ui_actor.needs_redraw());
    }

    #[test]
    fn noop_command_redraws_when_it_changes_visible_notice() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        app.state.set_warning_notice("old warning");
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
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::PrevPage,
                CommandInvocationSource::Keymap,
            ))),
            &mut runtime,
            Arc::clone(&pdf),
        )
        .expect("command should be handled");

        assert!(app.state.notice.is_none());
        assert!(runtime.ui_actor.needs_redraw());
    }
}
