use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEventKind};

use crate::backend::PdfBackend;
use crate::config::RenderPolicy;
use crate::error::AppResult;
use crate::perf::{PerfStats, RedrawReason};
use crate::presenter::PanOffset;
use crate::render::worker::{RenderWorker, RenderWorkerResult};

use super::core::{InteractionSubsystem, RenderSubsystem};
use super::loop_effects::LoopEffects;
use super::loop_runtime::LoopStep;
use super::nav::NavTracker;
use super::render_ops::CurrentTaskContext;
use super::state::AppState;
use super::terminal_session::TerminalSurface;
use super::view_ops::{
    RenderFramePlan, compute_current_scale_for_state, current_viewport_for_session,
};

pub(crate) struct RenderNavSyncParts<'a> {
    pub(crate) nav: &'a mut NavTracker,
    pub(crate) tracked_page: &'a mut usize,
    pub(crate) tracked_zoom: &'a mut f32,
    pub(crate) tracked_scale: &'a mut f32,
}

pub(super) struct RenderCompleteContext<'a, S> {
    pub(super) render_policy: &'a RenderPolicy,
    pub(super) session: &'a S,
    pub(super) pdf: &'a dyn PdfBackend,
    pub(super) input_actor: &'a InputActor,
    pub(super) prefetch_pause_after_input: Duration,
    pub(super) in_flight_len: usize,
}

pub(crate) struct InputActor {
    last_input_at: Instant,
}

impl InputActor {
    pub(crate) fn new(now: Instant) -> Self {
        Self { last_input_at: now }
    }

    pub(crate) fn is_interactive(&self, pause_after_input: Duration) -> bool {
        self.last_input_at.elapsed() < pause_after_input
    }

    pub(super) fn handle_timeout(
        &mut self,
        interaction: &mut InteractionSubsystem,
        state: &mut AppState,
    ) -> AppResult<LoopEffects> {
        let timeout_outcome = interaction.flush_sequence_timeout(state.mode);
        let mut effects = LoopEffects::from_commands(timeout_outcome.commands);
        if timeout_outcome.redraw {
            effects.request_redraw(RedrawReason::Input);
        }
        Ok(effects)
    }

    pub(super) fn handle_terminal_event(
        &mut self,
        event: Event,
        interaction: &mut InteractionSubsystem,
        state: &mut AppState,
    ) -> AppResult<LoopEffects> {
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                self.last_input_at = Instant::now();
                let outcome = interaction.handle_key_event(state, key)?;
                let mut effects = LoopEffects::from_commands(outcome.commands);
                if outcome.redraw {
                    effects.request_redraw(RedrawReason::Input);
                }
                Ok(effects)
            }
            Event::Resize(_, _) => {
                self.last_input_at = Instant::now();
                let mut effects = LoopEffects::none();
                effects.request_redraw(RedrawReason::Input);
                Ok(effects)
            }
            _ => Ok(LoopEffects::none()),
        }
    }
}

pub(crate) struct RenderActor {
    nav: NavTracker,
    tracked_page: usize,
    tracked_zoom: f32,
    tracked_scale: f32,
    prefetch_due: bool,
}

impl RenderActor {
    pub(crate) fn new(initial_page: usize, initial_zoom: f32, initial_scale: f32) -> Self {
        Self {
            nav: NavTracker::default(),
            tracked_page: initial_page,
            tracked_zoom: initial_zoom,
            tracked_scale: initial_scale,
            prefetch_due: true,
        }
    }

    pub(crate) fn nav_mut(&mut self) -> &mut NavTracker {
        &mut self.nav
    }

    pub(crate) fn nav_sync_parts_mut(&mut self) -> RenderNavSyncParts<'_> {
        RenderNavSyncParts {
            nav: &mut self.nav,
            tracked_page: &mut self.tracked_page,
            tracked_zoom: &mut self.tracked_zoom,
            tracked_scale: &mut self.tracked_scale,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.nav.intent().generation
    }

    pub(crate) fn nav_streak(&self) -> usize {
        self.nav.intent().streak
    }

    pub(crate) fn mark_prefetch_due(&mut self) {
        self.prefetch_due = true;
    }

    pub(crate) fn take_prefetch_due(&mut self) -> bool {
        let due = self.prefetch_due;
        self.prefetch_due = false;
        due
    }

    pub(super) fn drain_background_and_sync_navigation(
        &mut self,
        render: &mut RenderSubsystem,
        interaction: &mut InteractionSubsystem,
        state: &mut AppState,
        pdf: &dyn PdfBackend,
        current_scale: f32,
    ) -> bool {
        let mut changed = false;
        let previous_page = state.current_page;
        state.normalize_current_page(pdf.page_count());
        if state.current_page != previous_page {
            changed = true;
        }
        if interaction.drain_background_events(state) {
            changed = true;
        }
        if render.presenter.drain_background_events() {
            changed = true;
        }
        if interaction.apply_palette_requests(state) {
            changed = true;
        }

        let mut nav_sync_parts = self.nav_sync_parts_mut();
        if render.sync_navigation_state(state, pdf, &mut nav_sync_parts, current_scale) {
            changed = true;
        }
        changed
    }

    pub(super) fn ensure_iteration_work(
        &mut self,
        render: &mut RenderSubsystem,
        state: &mut AppState,
        pdf: &dyn PdfBackend,
        render_worker: &mut RenderWorker,
        step: &LoopStep,
    ) {
        render.ensure_current_task_enqueued(
            state,
            pdf,
            self,
            render_worker,
            CurrentTaskContext {
                current_scale: step.current_scale,
                required: step.required,
                current_interest_keys: step.current_interest_keys,
                current_cached: step.current_cached,
                preview_tasks: step.initial_preview_tasks.clone(),
            },
        );
        render.dispatch_prefetch_if_due(state, self, render_worker, step.prefetch_dispatch);
    }

    pub(super) fn handle_render_complete<S>(
        &self,
        render: &mut RenderSubsystem,
        state: &mut AppState,
        completed: RenderWorkerResult,
        ctx: RenderCompleteContext<'_, S>,
    ) -> bool
    where
        S: TerminalSurface,
    {
        let viewport = current_viewport_for_session(ctx.session, state.debug_status_visible);
        let visible_pages = state.visible_page_slots(ctx.pdf.page_count());
        let current_scale = compute_current_scale_for_state(
            state,
            render,
            ctx.render_policy,
            ctx.pdf,
            visible_pages.anchor_page,
            viewport,
        );
        let current_view = render.build_current_render_view(
            state,
            ctx.pdf,
            visible_pages,
            current_scale,
            self.generation() == 0,
        );
        let pan = PanOffset {
            cells_x: state.pan_x,
            cells_y: state.pan_y,
        };
        let enable_crop = state.zoom > 1.0;
        let interactive = ctx
            .input_actor
            .is_interactive(ctx.prefetch_pause_after_input);
        let redraw = render.process_render_result(
            state,
            completed,
            current_view.current_interest_keys.as_slice(),
            viewport,
            pan,
            enable_crop,
            interactive,
        );
        render
            .runtime
            .set_queue_depth_with_inflight(ctx.in_flight_len);
        redraw
    }
}

pub(crate) struct UiActor {
    needs_redraw: bool,
    last_pending_redraw: Instant,
    pending_redraw_interval: Duration,
}

impl UiActor {
    pub(crate) fn new(now: Instant, pending_redraw_interval: Duration) -> Self {
        Self {
            needs_redraw: true,
            last_pending_redraw: now,
            pending_redraw_interval,
        }
    }

    pub(crate) fn mark_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub(super) fn request_redraw(&mut self, perf_stats: &mut PerfStats, reason: RedrawReason) {
        self.mark_redraw();
        perf_stats.record_redraw(reason);
    }

    pub(crate) fn clear_redraw(&mut self) {
        self.needs_redraw = false;
    }

    pub(crate) fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub(crate) fn should_request_pending_redraw(
        &self,
        current_cached: bool,
        render_busy: bool,
        presenter_busy: bool,
    ) -> bool {
        !current_cached
            && (render_busy || presenter_busy)
            && self.last_pending_redraw.elapsed() >= self.pending_redraw_interval
    }

    pub(crate) fn should_wait_for_pending_redraw(
        &self,
        current_cached: bool,
        render_busy: bool,
        presenter_busy: bool,
    ) -> bool {
        !current_cached && (render_busy || presenter_busy)
    }

    pub(crate) fn on_drawn_non_cached_page(&mut self) {
        self.last_pending_redraw = Instant::now();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update_and_render_frame<S>(
        &mut self,
        render: &mut RenderSubsystem,
        interaction: &InteractionSubsystem,
        state: &mut AppState,
        session: &mut S,
        pdf: &dyn PdfBackend,
        page_count: usize,
        render_generation: u64,
        nav_streak: usize,
        render_busy: bool,
        presenter_busy: bool,
        changed: bool,
        step: &LoopStep,
    ) -> AppResult<()>
    where
        S: TerminalSurface,
    {
        if self.should_request_pending_redraw(step.current_cached, render_busy, presenter_busy) {
            self.request_redraw(&mut render.runtime.perf_stats, RedrawReason::PendingWork);
        }

        if changed {
            self.request_redraw(&mut render.runtime.perf_stats, RedrawReason::StateChanged);
        }

        if self.needs_redraw() {
            let palette_view = interaction.palette_view();
            let mut status_bar_segments = interaction.extensions.host.status_bar_segments(state);
            if let Some(pending_sequence) = interaction.pending_sequence_status() {
                status_bar_segments.push(pending_sequence);
            }
            render.render_frame(
                state,
                session,
                pdf,
                RenderFramePlan {
                    palette_view,
                    help_keymap: interaction.sequences.resolver.snapshot(),
                    status_bar_segments,
                    page_count,
                    visible_pages: step.visible_pages,
                    current_scale: step.current_scale,
                    initial_preview: step.initial_preview.clone(),
                    presenter_key: step.presenter_key,
                    highlight_overlay: interaction
                        .extensions
                        .host
                        .highlight_overlay_for(step.visible_pages.existing_pages()),
                    generation: render_generation,
                    nav_streak,
                },
            )?;
            self.clear_redraw();
            if !step.current_cached {
                self.on_drawn_non_cached_page();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{RenderActor, UiActor};

    #[test]
    fn render_actor_prefetch_due_is_consumed_once() {
        let mut actor = RenderActor::new(0, 1.0, 1.0);
        assert!(actor.take_prefetch_due());
        assert!(!actor.take_prefetch_due());
        actor.mark_prefetch_due();
        assert!(actor.take_prefetch_due());
        assert!(!actor.take_prefetch_due());
    }

    #[test]
    fn ui_actor_redraw_flag_roundtrip() {
        let now = Instant::now();
        let mut actor = UiActor::new(now, Duration::from_millis(33));
        assert!(actor.needs_redraw());
        actor.clear_redraw();
        assert!(!actor.needs_redraw());
        actor.mark_redraw();
        assert!(actor.needs_redraw());
    }

    #[test]
    fn ui_actor_waits_for_pending_redraw_only_while_busy_without_cached_frame() {
        let actor = UiActor::new(Instant::now(), Duration::from_millis(33));

        assert!(actor.should_wait_for_pending_redraw(false, true, false));
        assert!(actor.should_wait_for_pending_redraw(false, false, true));
        assert!(!actor.should_wait_for_pending_redraw(true, true, true));
        assert!(!actor.should_wait_for_pending_redraw(false, false, false));
    }
}
