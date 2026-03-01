use std::time::{Duration, Instant};

use super::nav::NavTracker;

pub(crate) struct RenderNavSyncParts<'a> {
    pub(crate) nav: &'a mut NavTracker,
    pub(crate) tracked_page: &'a mut usize,
    pub(crate) tracked_zoom: &'a mut f32,
    pub(crate) tracked_scale: &'a mut f32,
}

pub(crate) struct InputActor {
    last_input_at: Instant,
}

impl InputActor {
    pub(crate) fn new(now: Instant) -> Self {
        Self { last_input_at: now }
    }

    pub(crate) fn last_input_at_mut(&mut self) -> &mut Instant {
        &mut self.last_input_at
    }

    pub(crate) fn is_interactive(&self, pause_after_input: Duration) -> bool {
        self.last_input_at.elapsed() < pause_after_input
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

    pub(crate) fn mark_prefetch_due(&mut self) {
        self.prefetch_due = true;
    }

    pub(crate) fn take_prefetch_due(&mut self) -> bool {
        let due = self.prefetch_due;
        self.prefetch_due = false;
        due
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

    pub(crate) fn clear_redraw(&mut self) {
        self.needs_redraw = false;
    }

    pub(crate) fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub(crate) fn needs_redraw_mut(&mut self) -> &mut bool {
        &mut self.needs_redraw
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

    pub(crate) fn on_drawn_non_cached_page(&mut self) {
        self.last_pending_redraw = Instant::now();
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
}
