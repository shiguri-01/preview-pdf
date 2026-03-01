use crate::render::scheduler::{NavDirection, NavIntent};

#[derive(Debug, Clone, Copy)]
pub(crate) struct NavTracker {
    dir: NavDirection,
    streak: usize,
    generation: u64,
}

impl Default for NavTracker {
    fn default() -> Self {
        Self {
            dir: NavDirection::Forward,
            streak: 0,
            generation: 0,
        }
    }
}

impl NavTracker {
    pub(crate) fn intent(&self) -> NavIntent {
        NavIntent {
            dir: self.dir,
            streak: self.streak,
            generation: self.generation,
        }
    }

    pub(crate) fn on_zoom_change(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.streak = 0;
    }

    pub(crate) fn on_scale_change(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.streak = 0;
    }

    pub(crate) fn on_page_change(&mut self, prev_page: usize, next_page: usize) {
        if prev_page == next_page {
            return;
        }

        self.generation = self.generation.saturating_add(1);

        let direction = if next_page > prev_page {
            NavDirection::Forward
        } else {
            NavDirection::Backward
        };
        let is_jump = next_page.abs_diff(prev_page) > 1;

        if is_jump {
            self.dir = direction;
            self.streak = 1;
            return;
        }

        if self.dir == direction {
            self.streak = if self.streak == 0 {
                1
            } else {
                self.streak.saturating_add(1)
            };
        } else {
            self.dir = direction;
            self.streak = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NavTracker;
    use crate::render::scheduler::NavDirection;

    #[test]
    fn nav_tracker_bumps_generation_for_each_page_step() {
        let mut tracker = NavTracker::default();
        assert_eq!(tracker.intent().generation, 0);

        tracker.on_page_change(0, 1);
        let first = tracker.intent();
        assert_eq!(first.generation, 1);
        assert_eq!(first.streak, 1);
        assert_eq!(first.dir, NavDirection::Forward);

        tracker.on_page_change(1, 2);
        let second = tracker.intent();
        assert_eq!(second.generation, 2);
        assert_eq!(second.streak, 2);
        assert_eq!(second.dir, NavDirection::Forward);

        tracker.on_page_change(2, 1);
        let third = tracker.intent();
        assert_eq!(third.generation, 3);
        assert_eq!(third.streak, 1);
        assert_eq!(third.dir, NavDirection::Backward);
    }
}
