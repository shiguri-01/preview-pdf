use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{self, MissedTickBehavior};

use crate::backend::{PdfBackend, SharedPdfBackend};
use crate::error::{AppError, AppResult};
use crate::event::DomainEvent;
use crate::presenter::ImagePresenter;
use crate::render::worker::RenderWorker;

use super::actors::{InputActor, RenderActor, UiActor};
use super::core::{App, RunOptions};
use super::event_bus::EventBusRuntime;
use super::loop_driver::{
    InteractiveLoopDriver, LoopDriver, LoopDriverDecision, LoopDriverHandle, LoopEventMode,
    LoopMetricsSnapshot, LoopObservation,
};
use super::loop_runtime::{ActiveDocument, LoopControl, LoopRuntime, LoopStep, WaitEvent};
use super::render_ops::PrefetchDispatchPlan;
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
        self.run_loop(
            pdf,
            session,
            LoopEventMode::Interactive {
                watch: options.watch,
            },
            InteractiveLoopDriver,
        )
        .await
    }

    pub(crate) async fn run_loop<S, D>(
        &mut self,
        pdf: SharedPdfBackend,
        session: S,
        event_mode: LoopEventMode,
        mut driver: D,
    ) -> AppResult<D::Output>
    where
        S: TerminalSession,
        D: LoopDriver,
    {
        let session = RestoringSession::new(session);
        let page_count = pdf.page_count();
        if page_count == 0 {
            return Err(AppError::invalid_argument("pdf has no pages"));
        }

        let mut document = ActiveDocument::new(pdf);
        let (loop_event_tx, loop_event_rx, loop_event_runtime) = match event_mode {
            LoopEventMode::Interactive { .. } => EventBusRuntime::spawn_interactive(),
            LoopEventMode::Headless => EventBusRuntime::spawn_headless(),
        };
        let mut runtime = self.initialize_loop_runtime(
            Arc::clone(&document.pdf),
            page_count,
            session,
            loop_event_tx,
            loop_event_rx,
            loop_event_runtime,
        )?;
        if let LoopEventMode::Interactive { watch } = event_mode {
            runtime
                .loop_event_runtime
                .start_input(runtime.loop_event_tx.clone());
            if watch {
                runtime.loop_event_runtime.start_file_watch(
                    document.path.clone(),
                    self.watch_policy.poll_interval,
                    self.watch_policy.settle_delay,
                    runtime.loop_event_tx.clone(),
                );
            }
        }

        let result = self
            .run_driver_loop(&mut runtime, &mut document, &mut driver)
            .await;
        runtime.loop_event_runtime.shutdown();
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
        let pending_redraw_interval = self.event_loop_policy.pending_redraw_interval;
        let input_actor = InputActor::new(loop_started_at);
        let ui_actor = UiActor::new(loop_started_at, pending_redraw_interval);
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
            reload_in_flight: false,
            pending_reload: None,
            reload_retry_attempts: 0,
            reload_generation: 0,
        })
    }

    async fn run_driver_loop<S, D>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        document: &mut ActiveDocument,
        driver: &mut D,
    ) -> AppResult<D::Output>
    where
        S: TerminalSession,
        D: LoopDriver,
    {
        loop {
            let step = self.process_loop_iteration(runtime, document.pdf.as_ref())?;
            let observation = self.loop_observation(runtime, &step);
            let mut handle = LoopDriverHandle::new(&runtime.loop_event_tx);
            match driver.on_iteration(observation, &mut handle)? {
                LoopDriverDecision::Continue => {}
                LoopDriverDecision::Finish => {
                    return driver.on_finish(observation, self.loop_metrics_snapshot());
                }
            }

            match self
                .wait_and_handle_next_event(runtime, &step, document)
                .await?
            {
                LoopControl::Continue => {}
                LoopControl::Break => return driver.on_loop_break(),
            }
        }
    }

    fn loop_observation<S>(&self, runtime: &LoopRuntime<S>, step: &LoopStep) -> LoopObservation {
        let render_in_flight = runtime.render_worker.in_flight_len();
        let presenter_pending = self.render.presenter.has_pending_work();
        let redraw_pending = runtime.ui_actor.needs_redraw();
        let event_queue_empty = runtime.loop_event_rx.is_empty();
        LoopObservation {
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

    fn loop_metrics_snapshot(&self) -> LoopMetricsSnapshot {
        LoopMetricsSnapshot {
            runtime: self.render.runtime.perf_stats.clone(),
            presenter: self.render.presenter.perf_snapshot().unwrap_or_default(),
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
            self.build_loop_step(
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
        runtime: &mut LoopRuntime<S>,
        step: &LoopStep,
        document: &mut ActiveDocument,
    ) -> AppResult<LoopControl>
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
            &mut runtime.loop_event_rx,
            &mut runtime.render_worker,
            &mut *self.render.presenter,
            &mut runtime.prefetch_tick,
            &mut runtime.redraw_tick,
            wait_for_pending_redraw,
            wake_timeout,
        )
        .await;
        self.handle_waited_event(waited, runtime, document)
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

struct RestoringSession<S: TerminalSession> {
    session: S,
    active: bool,
}

impl<S: TerminalSession> RestoringSession<S> {
    fn new(session: S) -> Self {
        Self {
            session,
            active: true,
        }
    }
}

impl<S: TerminalSession> TerminalSurface for RestoringSession<S> {
    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        self.session.size()
    }

    fn draw<F>(&mut self, render: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.session.draw(render)
    }
}

impl<S: TerminalSession> TerminalSession for RestoringSession<S> {
    fn restore(&mut self) -> std::io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.session.restore()?;
        self.active = false;
        Ok(())
    }
}

impl<S: TerminalSession> Drop for RestoringSession<S> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::future::Future;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Size;
    use tokio::runtime::Builder;
    use tokio::sync::mpsc::unbounded_channel;

    use super::{WaitEvent, wait_next_event};
    use crate::app::App;
    use crate::app::core::InteractionSubsystem;
    use crate::app::loop_runtime::ActiveDocument;
    use crate::app::terminal_session::{TerminalSession, TerminalSurface};
    use crate::app::{
        LoopDriver, LoopDriverDecision, LoopDriverHandle, LoopMetricsSnapshot, LoopObservation,
    };
    use crate::backend::test_support::{build_pdf, unique_temp_path};
    use crate::backend::{OutlineNode, PdfBackend, PdfDoc, RgbaFrame, SharedPdfBackend, TextPage};
    use crate::command::{
        Command, CommandInvocationSource, CommandRequest, PanAmount, PanDirection,
    };
    use crate::condition::ConditionExpr;
    use crate::config::Config;
    use crate::error::{AppError, AppResult};
    use crate::event::{
        DocumentReloadReason, DocumentReloadRequest, DocumentReloadResult, DomainEvent,
    };
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;
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
        restore_count: Option<Arc<AtomicUsize>>,
    }

    impl StubSession {
        fn new(width: u16, height: u16) -> Self {
            Self {
                size: Size::new(width, height),
                restore_count: None,
            }
        }

        fn with_restore_count(width: u16, height: u16, restore_count: Arc<AtomicUsize>) -> Self {
            Self {
                size: Size::new(width, height),
                restore_count: Some(restore_count),
            }
        }
    }

    impl TerminalSurface for StubSession {
        fn size(&self) -> io::Result<Size> {
            Ok(self.size)
        }

        fn draw<F>(&mut self, _render: F) -> io::Result<()>
        where
            F: FnOnce(&mut ratatui::Frame<'_>),
        {
            Ok(())
        }
    }

    impl TerminalSession for StubSession {
        fn restore(&mut self) -> io::Result<()> {
            if let Some(count) = &self.restore_count {
                count.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    #[derive(Debug)]
    struct RestoreProbeDriver;

    impl LoopDriver for RestoreProbeDriver {
        type Output = ();

        fn on_iteration(
            &mut self,
            _observation: LoopObservation,
            _handle: &mut LoopDriverHandle<'_>,
        ) -> AppResult<LoopDriverDecision> {
            Ok(LoopDriverDecision::Finish)
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

    struct EmptyPdfBackend {
        path: PathBuf,
    }

    impl EmptyPdfBackend {
        fn new() -> Self {
            Self {
                path: PathBuf::from("empty.pdf"),
            }
        }
    }

    impl PdfBackend for EmptyPdfBackend {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            0
        }

        fn page_count(&self) -> usize {
            0
        }

        fn page_dimensions(&self, _page: usize) -> AppResult<(f32, f32)> {
            Err(AppError::invalid_argument("empty pdf"))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> AppResult<RgbaFrame> {
            Err(AppError::invalid_argument("empty pdf"))
        }

        fn extract_text_page(&self, _page: usize) -> AppResult<TextPage> {
            Err(AppError::invalid_argument("empty pdf"))
        }

        fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
            Ok(Vec::new())
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

    #[test]
    fn run_loop_restores_session_when_pdf_has_no_pages() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let restore_count = Arc::new(AtomicUsize::new(0));
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let session = StubSession::with_restore_count(80, 24, Arc::clone(&restore_count));
        let pdf: SharedPdfBackend = Arc::new(EmptyPdfBackend::new());
        let driver = RestoreProbeDriver;

        let err = tokio_runtime
            .block_on(app.run_loop(pdf, session, super::LoopEventMode::Headless, driver))
            .expect_err("empty pdf should fail before the loop starts");

        assert!(err.to_string().contains("pdf has no pages"));
        assert_eq!(restore_count.load(Ordering::Relaxed), 1);
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
    fn wake_timeout_applies_expired_sequence_command() {
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
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::OpenHelp,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Wake),
                &mut runtime,
                &mut document,
            )
            .expect("wake should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert_eq!(app.state.mode, crate::app::Mode::Help);
        assert!(!matches!(
            runtime.loop_event_rx.try_recv(),
            Ok(DomainEvent::Command(_))
        ));
    }

    #[test]
    fn input_outcome_applies_expired_command_before_latest_command() {
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
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g')],
                Command::DebugStatusShow,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x')],
                Command::DebugStatusHide,
            )
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                    KeyCode::Char('x'),
                    KeyModifiers::NONE,
                )))),
                &mut runtime,
                &mut document,
            )
            .expect("input should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert!(
            !app.state.debug_status_visible,
            "DebugStatusShow must be applied before DebugStatusHide"
        );
        assert!(!matches!(
            runtime.loop_event_rx.try_recv(),
            Ok(DomainEvent::Command(_))
        ));
    }

    #[test]
    fn focus_changing_timeout_drops_waited_input() {
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
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x')],
                Command::OpenHelp,
            )
            .expect("single-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('x'), ShortcutKey::char('x')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_exact(
                ConditionExpr::Always,
                &[ShortcutKey::char('j')],
                Command::HelpScrollDown,
            )
            .expect("help binding should register");
        app.interaction =
            InteractionSubsystem::with_sequence_registry_and_timeout(registry, Duration::ZERO);

        app.interaction
            .handle_key_event(
                &mut app.state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                    KeyCode::Char('j'),
                    KeyModifiers::NONE,
                )))),
                &mut runtime,
                &mut document,
            )
            .expect("input should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert_eq!(app.state.mode, crate::app::Mode::Help);
        assert_eq!(app.state.help_scroll, 0);
    }

    #[test]
    fn palette_close_from_input_applies_before_queued_input() {
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
            .push_back(crate::app::PaletteRequest::Open {
                kind: crate::palette::PaletteKind::Command,
                payload: None,
            });
        assert!(app.interaction.apply_palette_requests(&mut app.state));
        assert_eq!(app.state.mode, crate::app::Mode::Palette);
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
        runtime
            .loop_event_tx
            .send(DomainEvent::Input(Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE,
            ))))
            .expect("queued input should be accepted");
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Input(Event::Key(KeyEvent::new(
                    KeyCode::Esc,
                    KeyModifiers::NONE,
                )))),
                &mut runtime,
                &mut document,
            )
            .expect("palette close input should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert_eq!(app.state.mode, crate::app::Mode::Normal);
        assert!(matches!(
            runtime.loop_event_rx.try_recv(),
            Ok(DomainEvent::Input(Event::Key(key)))
                if key.code == KeyCode::Char('x')
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                    Command::GotoPage { page: 999 },
                    CommandInvocationSource::Binding,
                ))),
                &mut runtime,
                &mut document,
            )
            .expect("command error should be handled as a notice");

        assert!(matches!(control, super::LoopControl::Continue));
        let notice = app.state.notice.expect("command error should set a notice");
        assert_eq!(notice.level, crate::app::NoticeLevel::Warning);
        assert_eq!(notice.message, "page 999 is out of range (1-1)");
        assert!(runtime.loop_event_rx.try_recv().is_err());
    }

    #[test]
    fn reload_document_command_starts_reload_without_blocking_loop() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
                    Command::ReloadDocument,
                    CommandInvocationSource::CommandPaletteInput,
                ))),
                &mut runtime,
                &mut document,
            )
            .expect("reload command should be handled");

        assert!(matches!(control, super::LoopControl::Continue));
        assert!(runtime.reload_in_flight);
    }

    #[test]
    fn document_reload_success_replaces_active_document_and_clamps_page() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let file = unique_temp_path("reload_success.pdf");
        fs::write(&file, build_pdf(&["one", "two", "three"]))
            .expect("first test pdf should be created");
        let first =
            Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
        let old_doc_id = first.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&first));
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        app.state.current_page = 2;
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&first),
                first.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");
        runtime.ui_actor.clear_redraw();

        fs::write(&file, build_pdf(&["new one", "new two"]))
            .expect("second test pdf should replace first");
        let second =
            Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;
        assert_ne!(old_doc_id, second.doc_id());

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::Manual,
                generation: 0,
                result: Ok(second),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload result should be handled");

        assert_ne!(document.pdf.doc_id(), old_doc_id);
        assert_eq!(runtime.page_count, 2);
        assert_eq!(app.state.current_page, 1);
        assert!(runtime.ui_actor.needs_redraw());
        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn document_reload_success_applies_even_when_doc_id_matches() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let file = unique_temp_path("reload_same_doc_id.pdf");
        fs::write(&file, build_pdf(&["same content"])).expect("test pdf should be created");
        let first =
            Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
        let second =
            Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;
        assert_eq!(first.doc_id(), second.doc_id());
        assert!(!Arc::ptr_eq(&first, &second));

        let mut document = ActiveDocument::new(Arc::clone(&first));
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&first),
                first.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");
        runtime.ui_actor.clear_redraw();
        runtime.reload_in_flight = true;
        runtime.reload_retry_attempts = 2;

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::Manual,
                generation: 0,
                result: Ok(Arc::clone(&second)),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload result should be handled");

        assert!(Arc::ptr_eq(&document.pdf, &second));
        assert_eq!(runtime.reload_retry_attempts, 0);
        assert!(runtime.ui_actor.needs_redraw());
        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn document_reload_success_clears_previous_reload_notice() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let file = unique_temp_path("reload_clears_notice.pdf");
        fs::write(&file, build_pdf(&["one"])).expect("first test pdf should be created");
        let first =
            Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
        let mut document = ActiveDocument::new(Arc::clone(&first));
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        app.state
            .set_warning_notice("Could not reload changed document: still invalid");
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&first),
                first.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");
        runtime.reload_in_flight = true;

        fs::write(&file, build_pdf(&["two"])).expect("second test pdf should replace first");
        let second =
            Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::FileChanged,
                generation: 0,
                result: Ok(second),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload result should be handled");

        assert!(app.state.notice.is_none());
        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn manual_document_reload_failure_keeps_previous_document() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let old_doc_id = pdf.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
        runtime.reload_in_flight = true;

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::Manual,
                generation: 0,
                result: Err("still being written".to_string()),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload failure should be handled");

        assert_eq!(document.pdf.doc_id(), old_doc_id);
        assert!(!runtime.reload_in_flight);
        let notice = app.state.notice.expect("reload failure should set notice");
        assert_eq!(notice.level, crate::app::NoticeLevel::Error);
        assert!(notice.message.contains("still being written"));
    }

    #[test]
    fn file_reload_failure_keeps_previous_document_and_retries_quietly() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let old_doc_id = pdf.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
        runtime.reload_in_flight = true;

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::FileChanged,
                generation: 0,
                result: Err("still being written".to_string()),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload failure should be handled");

        assert_eq!(document.pdf.doc_id(), old_doc_id);
        assert!(!runtime.reload_in_flight);
        assert_eq!(runtime.reload_retry_attempts, 1);
        assert!(app.state.notice.is_none());

        let retry = tokio_runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(1), runtime.loop_event_rx.recv()).await
            })
            .expect("retry event should arrive")
            .expect("loop event channel should stay open");
        assert!(matches!(
            retry,
            DomainEvent::ReloadDocument(DocumentReloadRequest {
                reason: DocumentReloadReason::FileChanged,
                retry: true,
                ..
            })
        ));
    }

    #[test]
    fn file_reload_failure_after_retry_budget_shows_warning() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let old_doc_id = pdf.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
        runtime.reload_in_flight = true;
        runtime.reload_retry_attempts = 5;

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::FileChanged,
                generation: 0,
                result: Err("still invalid after retries".to_string()),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload failure should be handled");

        assert_eq!(document.pdf.doc_id(), old_doc_id);
        assert!(!runtime.reload_in_flight);
        assert_eq!(runtime.reload_retry_attempts, 5);
        assert!(runtime.ui_actor.needs_redraw());
        let notice = app.state.notice.expect("reload failure should set notice");
        assert_eq!(notice.level, crate::app::NoticeLevel::Warning);
        assert!(notice.message.contains("still invalid after retries"));
        assert!(runtime.loop_event_rx.try_recv().is_err());
    }

    #[test]
    fn file_reload_success_after_retries_replaces_document_and_resets_retry_count() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let file = unique_temp_path("reload_retry_success.pdf");
        fs::write(&file, build_pdf(&["one", "two", "three"]))
            .expect("first test pdf should be created");
        let first =
            Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
        let old_doc_id = first.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&first));
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        app.state.current_page = 2;
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&first),
                first.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");
        runtime.ui_actor.clear_redraw();
        runtime.reload_in_flight = true;
        runtime.reload_retry_attempts = 3;

        fs::write(&file, build_pdf(&["new one", "new two"]))
            .expect("second test pdf should replace first");
        let second =
            Arc::new(PdfDoc::open(&file).expect("second pdf should open")) as SharedPdfBackend;
        assert_ne!(old_doc_id, second.doc_id());

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::FileChanged,
                generation: 0,
                result: Ok(second),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload result should be handled");

        assert_ne!(document.pdf.doc_id(), old_doc_id);
        assert_eq!(runtime.page_count, 2);
        assert_eq!(app.state.current_page, 1);
        assert_eq!(runtime.reload_retry_attempts, 0);
        assert!(app.state.notice.is_none());
        assert!(runtime.ui_actor.needs_redraw());
        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn stale_file_reload_failure_yields_to_pending_fresh_reload() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let old_doc_id = pdf.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
        runtime.reload_in_flight = true;
        runtime.reload_retry_attempts = 2;
        runtime.pending_reload = Some(DocumentReloadRequest::new(
            DocumentReloadReason::FileChanged,
        ));

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::FileChanged,
                generation: 0,
                result: Err("stale failure".to_string()),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload failure should be handled");

        assert_eq!(document.pdf.doc_id(), old_doc_id);
        assert!(runtime.reload_in_flight);
        assert!(runtime.pending_reload.is_none());
        assert_eq!(runtime.reload_retry_attempts, 0);
        assert!(app.state.notice.is_none());
        assert!(!runtime.ui_actor.needs_redraw());
    }

    #[test]
    fn stale_file_reload_success_yields_to_pending_fresh_reload() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let file = unique_temp_path("reload_stale_success.pdf");
        fs::write(&file, build_pdf(&["one", "two", "three"]))
            .expect("first test pdf should be created");
        let first =
            Arc::new(PdfDoc::open(&file).expect("first pdf should open")) as SharedPdfBackend;
        let old_doc_id = first.doc_id();
        let mut document = ActiveDocument::new(Arc::clone(&first));
        let mut app =
            App::new_with_config(PresenterKind::RatatuiImage, Config::default()).expect("app init");
        let (loop_event_tx, loop_event_rx, loop_event_runtime) =
            crate::app::event_bus::EventBusRuntime::spawn_headless();
        let session = StubSession::new(80, 24);
        let mut runtime = app
            .initialize_loop_runtime(
                Arc::clone(&first),
                first.page_count(),
                session,
                loop_event_tx,
                loop_event_rx,
                loop_event_runtime,
            )
            .expect("runtime should initialize");
        runtime.ui_actor.clear_redraw();
        runtime.reload_in_flight = true;
        runtime.reload_retry_attempts = 2;
        runtime.pending_reload = Some(DocumentReloadRequest::new(
            DocumentReloadReason::FileChanged,
        ));

        fs::write(&file, build_pdf(&["stale one", "stale two"]))
            .expect("second test pdf should replace first");
        let stale =
            Arc::new(PdfDoc::open(&file).expect("stale pdf should open")) as SharedPdfBackend;
        assert_ne!(old_doc_id, stale.doc_id());

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::DocumentReloaded(DocumentReloadResult {
                reason: DocumentReloadReason::FileChanged,
                generation: 0,
                result: Ok(stale),
            })),
            &mut runtime,
            &mut document,
        )
        .expect("reload result should be handled");

        assert_eq!(document.pdf.doc_id(), old_doc_id);
        assert!(runtime.reload_in_flight);
        assert!(runtime.pending_reload.is_none());
        assert_eq!(runtime.reload_retry_attempts, 0);
        assert!(app.state.notice.is_none());
        assert!(!runtime.ui_actor.needs_redraw());

        runtime.loop_event_runtime.shutdown();
        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn old_delayed_retry_after_newer_reload_is_ignored() {
        let tokio_runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");
        let _guard = tokio_runtime.enter();
        let pdf = test_pdf_backend();
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
        runtime.reload_generation = 2;
        runtime.reload_retry_attempts = 3;
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::ReloadDocument(DocumentReloadRequest::retry(
                DocumentReloadReason::FileChanged,
                1,
            ))),
            &mut runtime,
            &mut document,
        )
        .expect("stale retry should be handled");

        assert!(!runtime.reload_in_flight);
        assert!(runtime.pending_reload.is_none());
        assert_eq!(runtime.reload_generation, 2);
        assert_eq!(runtime.reload_retry_attempts, 3);
        assert!(app.state.notice.is_none());
        assert!(!runtime.ui_actor.needs_redraw());
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        let control = app
            .handle_waited_event(
                WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                    Command::NextPage,
                    CommandInvocationSource::Binding,
                ))),
                &mut runtime,
                &mut document,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::EncodeComplete(
                PresenterBackgroundEvent::EncodeComplete {
                    redraw_requested: false,
                },
            )),
            &mut runtime,
            &mut document,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
        assert!(runtime.render_actor.take_prefetch_due());
        assert!(!runtime.render_actor.take_prefetch_due());
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::PrefetchTick),
            &mut runtime,
            &mut document,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
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
            &mut document,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::PrevPage,
                CommandInvocationSource::Binding,
            ))),
            &mut runtime,
            &mut document,
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
            &mut document,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::NextSearchHit,
                CommandInvocationSource::Binding,
            ))),
            &mut runtime,
            &mut document,
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
        let mut document = ActiveDocument::new(Arc::clone(&pdf));
        runtime.ui_actor.clear_redraw();

        app.handle_waited_event(
            WaitEvent::Event(DomainEvent::Command(CommandRequest::new(
                Command::PrevPage,
                CommandInvocationSource::Binding,
            ))),
            &mut runtime,
            &mut document,
        )
        .expect("command should be handled");

        assert!(app.state.notice.is_none());
        assert!(runtime.ui_actor.needs_redraw());
    }
}
