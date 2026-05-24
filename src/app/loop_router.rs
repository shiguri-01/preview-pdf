use std::sync::Arc;

use crate::backend::SharedPdfBackend;
use crate::command::{Command, CommandOutcome, CommandRequest, PanAmount};
use crate::error::AppResult;
use crate::event::{AppEvent, DomainEvent};
use crate::perf::RedrawReason;
use crate::presenter::PresenterBackgroundEvent;

use super::actors::RenderCompleteContext;
use super::core::App;
use super::loop_effects::LoopEffects;
use super::loop_runtime::{
    LoopControl, LoopRuntime, SessionRestore, WaitEvent, terminate_process_now,
};
use super::state::notice_action_for_error;
use super::terminal_session::TerminalSurface;

impl App {
    pub(super) fn apply_loop_effects<S>(
        &mut self,
        runtime: &mut LoopRuntime<S>,
        effects: LoopEffects,
    ) -> LoopControl
    where
        S: TerminalSurface,
    {
        let (commands, events, redraws, quit_requested) = effects.into_parts();
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
        if quit_requested && runtime.loop_event_tx.send(DomainEvent::Quit).is_err() {
            return LoopControl::Break;
        }
        LoopControl::Continue
    }

    pub(super) fn handle_waited_event<S>(
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
        let timeout_effects = runtime.input_actor.handle_timeout(
            &mut self.interaction,
            &mut self.state,
            &mut runtime.session,
        )?;
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
                    &mut runtime.session,
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
                    self.handle_command_event(request, runtime, pdf)?,
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
                        config: &self.config,
                        session: &runtime.session,
                        pdf: pdf.as_ref(),
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
            WaitEvent::Event(DomainEvent::Quit) => {
                terminate_process_now(runtime);
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
        pdf: SharedPdfBackend,
    ) -> AppResult<LoopControl>
    where
        S: TerminalSurface + SessionRestore,
    {
        let request = self.resolve_command_request(&runtime.session, request);
        let state_before_command = self.state.clone();
        let previous_page = self.state.current_page;
        let dispatch =
            match self
                .interaction
                .dispatch_command(&mut self.state, request, Arc::clone(&pdf))
            {
                Ok(dispatch) => dispatch,
                Err(err) => {
                    self.state.apply_notice_action(notice_action_for_error(err));
                    self.request_redraw(runtime, RedrawReason::Command);
                    return Ok(LoopControl::Continue);
                }
            };
        let mut effects = LoopEffects::none();
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
                .sync_search_after_page_change(Arc::clone(&pdf), self.state.current_page);
        }
        match dispatch.outcome {
            CommandOutcome::QuitRequested => {
                terminate_process_now(runtime);
            }
            CommandOutcome::Applied => self.request_redraw(runtime, RedrawReason::Command),
            CommandOutcome::Noop => {
                if !palette_changed && self.state != state_before_command {
                    self.request_redraw(runtime, RedrawReason::Command);
                }
            }
        }
        Ok(LoopControl::Continue)
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
