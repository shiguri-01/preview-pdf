use std::sync::Arc;
use std::time::Duration;

use crate::backend::SharedPdfBackend;
use crate::command::{Command, CommandLifecycleEffect, CommandOutcome, CommandRequest, PanAmount};
use crate::error::{AppError, AppResult};
use crate::event::{
    AppEvent, DocumentReloadReason, DocumentReloadRequest, DocumentReloadResult, DomainEvent,
};
use crate::perf::RedrawReason;
use crate::presenter::PresenterBackgroundEvent;
use crate::render::worker::RenderWorker;

use super::actors::RenderCompleteContext;
use super::core::App;
use super::loop_effects::LoopEffects;
use super::loop_runtime::{
    ActiveDocument, LoopControl, LoopRuntime, SessionRestore, WaitEvent, terminate_process_now,
};
use super::state::notice_action_for_error;
use super::terminal_session::TerminalSurface;

const FILE_RELOAD_RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_millis(1_000),
    Duration::from_millis(1_500),
    Duration::from_millis(2_000),
];

impl App {
    pub(super) fn apply_loop_effects<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        effects: LoopEffects,
    ) -> LoopControl
    where
        S: TerminalSurface,
    {
        let (commands, events, redraws) = effects.into_parts();
        for reason in redraws {
            self.request_redraw(runtime, reason);
        }
        for request in commands {
            if runtime
                .loop_event_tx
                .send(DomainEvent::Command(request))
                .is_err()
            {
                return LoopControl::Break;
            }
        }
        for event in events {
            if runtime.loop_event_tx.send(event).is_err() {
                return LoopControl::Break;
            }
        }
        LoopControl::Continue
    }

    pub(super) fn handle_waited_event<S>(
        &mut self,
        waited: WaitEvent,
        runtime: &mut LoopRuntime<S>,
        document: &mut ActiveDocument,
    ) -> AppResult<LoopControl>
    where
        S: TerminalSurface + SessionRestore,
    {
        // Wake events are not guaranteed to arrive before the next input event, so the
        // loop checks for timed-out sequences at the start of every iteration as well.
        let timeout_effects = runtime
            .input_actor
            .handle_timeout(&mut self.interaction, &mut self.state)?;
        if matches!(
            self.apply_loop_effects(runtime, timeout_effects),
            LoopControl::Break
        ) {
            return Ok(LoopControl::Break);
        }

        match waited {
            WaitEvent::Event(DomainEvent::Input(event)) => {
                let effects = runtime.input_actor.handle_terminal_event(
                    event,
                    &mut self.interaction,
                    &mut self.state,
                )?;
                if matches!(
                    self.apply_loop_effects(runtime, effects),
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
                if matches!(
                    self.handle_command_event(request, runtime, document)?,
                    LoopControl::Break
                ) {
                    return Ok(LoopControl::Break);
                }
            }
            WaitEvent::Event(DomainEvent::App(event)) => {
                let needs_redraw = !matches!(event, AppEvent::CommandExecuted { .. });
                self.interaction.handle_app_event(&mut self.state, &event);
                if needs_redraw {
                    self.request_redraw(runtime, RedrawReason::AppEvent);
                }
            }
            WaitEvent::Event(DomainEvent::RenderComplete(completed)) => {
                if runtime.render_actor.handle_render_complete(
                    &mut self.render,
                    &mut self.state,
                    completed,
                    RenderCompleteContext {
                        render_policy: &self.render_policy,
                        session: &runtime.session,
                        pdf: document.pdf.as_ref(),
                        input_actor: &runtime.input_actor,
                        prefetch_pause_after_input: runtime.prefetch_pause_after_input,
                        in_flight_len: runtime.render_worker.in_flight_len(),
                    },
                ) {
                    self.request_redraw(runtime, RedrawReason::RenderComplete);
                }
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
            WaitEvent::Event(DomainEvent::ReloadDocument(request)) => {
                self.request_document_reload(runtime, document, request);
            }
            WaitEvent::Event(DomainEvent::DocumentReloaded(result)) => {
                self.handle_document_reload_result(runtime, document, result)?;
            }
            WaitEvent::Event(DomainEvent::Wake) => {}
            WaitEvent::Closed => return Ok(LoopControl::Break),
        }
        Ok(LoopControl::Continue)
    }

    pub(super) fn resolve_command_request<S: TerminalSurface>(
        &self,
        session: &S,
        request: CommandRequest,
    ) -> CommandRequest {
        CommandRequest {
            command: self.resolve_command(session, request.command),
            source: request.source,
        }
    }

    pub(super) fn resolve_command<S: TerminalSurface>(
        &self,
        session: &S,
        command: Command,
    ) -> Command {
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

    fn handle_command_event<S>(
        &mut self,
        request: CommandRequest,
        runtime: &mut LoopRuntime<S>,
        document: &mut ActiveDocument,
    ) -> AppResult<LoopControl>
    where
        S: TerminalSurface + SessionRestore,
    {
        let request = self.resolve_command_request(&runtime.session, request);
        if matches!(request.command, Command::ReloadDocument) {
            self.request_document_reload(
                runtime,
                document,
                DocumentReloadRequest::new(DocumentReloadReason::Manual),
            );
            if runtime
                .loop_event_tx
                .send(DomainEvent::App(AppEvent::CommandExecuted {
                    id: request.command.command_id(),
                    outcome: CommandOutcome::Applied,
                }))
                .is_err()
            {
                return Ok(LoopControl::Break);
            }
            return Ok(LoopControl::Continue);
        }
        let state_before_command = self.state.clone();
        let previous_page = self.state.current_page;
        let view_policy = self.view_policy;
        let dispatch = match self.interaction.dispatch_command(
            &mut self.state,
            view_policy,
            request,
            Arc::clone(&document.pdf),
        ) {
            Ok(dispatch) => dispatch,
            Err(err) => {
                self.state.apply_notice_action(notice_action_for_error(err));
                self.request_redraw(runtime, RedrawReason::Command);
                return Ok(LoopControl::Continue);
            }
        };
        let mut effects = LoopEffects::from_commands(dispatch.follow_up_commands.clone());
        for event in dispatch.emitted_events {
            effects.push_event(DomainEvent::App(event));
        }
        if matches!(
            self.apply_loop_effects(runtime, effects),
            LoopControl::Break
        ) {
            return Ok(LoopControl::Break);
        }
        let palette_changed = self.interaction.apply_palette_requests(&mut self.state);
        if palette_changed {
            self.request_redraw(runtime, RedrawReason::StateChanged);
        }
        if self.state.current_page != previous_page {
            self.interaction
                .sync_search_after_page_change(Arc::clone(&document.pdf), self.state.current_page);
        }
        if dispatch.lifecycle == CommandLifecycleEffect::Quit {
            terminate_process_now(runtime);
        }
        match dispatch.outcome {
            CommandOutcome::Applied => self.request_redraw(runtime, RedrawReason::Command),
            CommandOutcome::Noop => {
                if !palette_changed && self.state != state_before_command {
                    self.request_redraw(runtime, RedrawReason::Command);
                }
            }
        }
        Ok(LoopControl::Continue)
    }

    fn request_document_reload<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        document: &ActiveDocument,
        mut request: DocumentReloadRequest,
    ) where
        S: TerminalSurface,
    {
        if request.retry && request.generation < runtime.reload_generation {
            return;
        }
        if request.generation == 0 {
            runtime.reload_generation += 1;
            request = request.with_generation(runtime.reload_generation);
        }
        if !request.retry || matches!(request.reason, DocumentReloadReason::Manual) {
            runtime.reload_retry_attempts = 0;
        }
        if runtime.reload_in_flight {
            runtime.pending_reload = Some(request);
            return;
        }

        runtime.reload_in_flight = true;
        runtime.loop_event_runtime.start_document_reload(
            document.path.clone(),
            request,
            runtime.loop_event_tx.clone(),
        );
    }

    fn handle_document_reload_result<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        document: &mut ActiveDocument,
        reload: DocumentReloadResult,
    ) -> AppResult<()>
    where
        S: TerminalSurface,
    {
        runtime.reload_in_flight = false;
        if self.start_pending_document_reload(runtime, document) {
            return Ok(());
        }

        match reload.result {
            Ok(pdf) => {
                if let Err(err) = self.apply_document_reload(runtime, document, pdf) {
                    self.state
                        .set_error_notice(format!("Could not reload document: {err}"));
                    self.request_redraw(runtime, RedrawReason::AppEvent);
                }
            }
            Err(message) => {
                if matches!(reload.reason, DocumentReloadReason::Manual) {
                    self.state
                        .set_error_notice(format!("Could not reload document: {message}"));
                    self.request_redraw(runtime, RedrawReason::AppEvent);
                } else if self.schedule_file_reload_retry(runtime, reload.generation) {
                    return Ok(());
                } else {
                    self.state.set_warning_notice(format!(
                        "Could not reload changed document: {message}"
                    ));
                    self.request_redraw(runtime, RedrawReason::AppEvent);
                }
            }
        }

        self.start_pending_document_reload(runtime, document);
        Ok(())
    }

    fn start_pending_document_reload<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        document: &ActiveDocument,
    ) -> bool
    where
        S: TerminalSurface,
    {
        if let Some(request) = runtime.pending_reload.take() {
            self.request_document_reload(runtime, document, request);
            return true;
        }
        false
    }

    fn schedule_file_reload_retry<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        generation: u64,
    ) -> bool
    where
        S: TerminalSurface,
    {
        if generation < runtime.reload_generation {
            return true;
        }
        let Some(delay) = FILE_RELOAD_RETRY_DELAYS.get(usize::from(runtime.reload_retry_attempts))
        else {
            return false;
        };
        runtime.reload_retry_attempts += 1;
        runtime.loop_event_runtime.start_delayed_document_reload(
            DocumentReloadRequest::retry(DocumentReloadReason::FileChanged, generation),
            *delay,
            runtime.loop_event_tx.clone(),
        );
        true
    }

    fn apply_document_reload<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        document: &mut ActiveDocument,
        pdf: SharedPdfBackend,
    ) -> AppResult<()>
    where
        S: TerminalSurface,
    {
        if pdf.page_count() == 0 {
            return Err(AppError::invalid_argument("reloaded pdf has no pages"));
        }
        let old_doc_id = document.pdf.doc_id();
        runtime.reload_retry_attempts = 0;
        document.replace(Arc::clone(&pdf));
        runtime.page_count = pdf.page_count();
        self.state.current_page = self.state.current_page.min(runtime.page_count - 1);
        self.state.normalize_current_page(runtime.page_count);
        self.state.clear_reload_notice();
        self.state.clear_render_notice();

        self.render.runtime.l1_cache.remove_doc(old_doc_id);
        self.render.presenter.reset_terminal_state();
        self.render.viewer_has_image = false;
        self.render.image_occluded_last_frame = false;
        runtime.render_worker =
            RenderWorker::spawn(Arc::clone(&pdf), self.render_policy.worker_threads);

        let viewport = Self::current_viewport(&runtime.session, self.state.debug_status_visible);
        let visible_pages = self.state.visible_page_slots(runtime.page_count);
        let tracked_scale =
            self.compute_current_scale(pdf.as_ref(), visible_pages.anchor_page, viewport);
        let mut render_actor = super::actors::RenderActor::new(
            visible_pages.anchor_page,
            self.state.zoom,
            tracked_scale,
        );
        self.render.runtime.reset_prefetch(
            pdf.as_ref(),
            visible_pages.anchor_page,
            render_actor.nav_mut().intent(),
            tracked_scale,
        );
        runtime.render_actor = render_actor;
        self.interaction
            .reset_extensions_for_document_reload(&mut self.state, Arc::clone(&pdf));
        self.request_redraw(runtime, RedrawReason::StateChanged);
        Ok(())
    }

    pub(super) fn request_redraw<S>(&mut self, runtime: &mut LoopRuntime<S>, reason: RedrawReason)
    where
        S: TerminalSurface,
    {
        runtime
            .ui_actor
            .request_redraw(&mut self.render.runtime.perf_stats, reason);
    }
}
