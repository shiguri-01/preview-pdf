use std::collections::VecDeque;

use crate::app::{AppState, NoticeAction, PaletteRequest};
use crate::command::CommandOutcome;
use crate::error::{AppError, AppResult};
use crate::event::{AppEvent, HistoryOp, NavReason, PageGotoKind};
use crate::history::palette::{HistoryPaletteEntry, HistoryPaletteReason, HistoryUiSnapshot};
use crate::palette::{PageIndex, PaletteCandidateId, PaletteKind, PaletteOpenOptions};

const HISTORY_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
struct HistoryEntry {
    page: usize,
    reason: Option<NavReason>,
}

#[derive(Default)]
pub struct HistoryState {
    back_stack: VecDeque<HistoryEntry>,
    forward_stack: VecDeque<HistoryEntry>,
    current_reason: Option<NavReason>,
    suppress_next_record: bool,
}

pub struct HistoryCommandPort<'a> {
    state: &'a mut HistoryState,
}

impl<'a> HistoryCommandPort<'a> {
    pub(crate) fn new(state: &'a mut HistoryState) -> Self {
        Self { state }
    }

    pub(crate) fn back(
        &mut self,
        app: &mut AppState,
        page_count: usize,
    ) -> (CommandOutcome, NoticeAction) {
        self.state.back(app, page_count)
    }

    pub(crate) fn forward(
        &mut self,
        app: &mut AppState,
        page_count: usize,
    ) -> (CommandOutcome, NoticeAction) {
        self.state.forward(app, page_count)
    }

    pub(crate) fn goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        self.state.goto(app, page_count, page)
    }

    pub(crate) fn open_palette(&self, app: &AppState) -> PaletteRequest {
        self.state.open_palette(app)
    }
}

impl HistoryState {
    pub fn back(
        &mut self,
        app: &mut AppState,
        page_count: usize,
    ) -> (CommandOutcome, NoticeAction) {
        let Some(target) = self.back_stack.pop_back() else {
            return (CommandOutcome::Noop, NoticeAction::Clear);
        };

        self.push_forward(HistoryEntry {
            page: app.current_page,
            reason: self.current_reason.clone(),
        });
        let normalized_target = app.normalize_page_for_layout(target.page, page_count);
        self.suppress_next_record = app.current_page != normalized_target;
        app.current_page = normalized_target;
        self.current_reason = target.reason;
        (CommandOutcome::Applied, NoticeAction::Clear)
    }

    pub fn forward(
        &mut self,
        app: &mut AppState,
        page_count: usize,
    ) -> (CommandOutcome, NoticeAction) {
        let Some(target) = self.forward_stack.pop_back() else {
            return (CommandOutcome::Noop, NoticeAction::Clear);
        };

        self.push_back(HistoryEntry {
            page: app.current_page,
            reason: self.current_reason.clone(),
        });
        let normalized_target = app.normalize_page_for_layout(target.page, page_count);
        self.suppress_next_record = app.current_page != normalized_target;
        app.current_page = normalized_target;
        self.current_reason = target.reason;
        (CommandOutcome::Applied, NoticeAction::Clear)
    }

    pub fn goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<(CommandOutcome, NoticeAction)> {
        if page < 1 {
            return Err(AppError::invalid_argument("page number must be >= 1"));
        }
        if page > page_count {
            return Err(AppError::page_out_of_range(page, page_count));
        }

        let target = app.normalize_page_for_layout(page - 1, page_count);
        if app.current_page == target {
            return Ok((CommandOutcome::Noop, NoticeAction::Clear));
        }
        let target_reason = self.find_reason_for_page(target);

        self.push_back(HistoryEntry {
            page: app.current_page,
            reason: self.current_reason.clone(),
        });
        self.suppress_next_record = true;
        app.current_page = target;
        self.current_reason = target_reason;
        Ok((CommandOutcome::Applied, NoticeAction::Clear))
    }

    pub fn open_palette(&self, app: &AppState) -> PaletteRequest {
        let _ = app;
        PaletteRequest::Open {
            kind: PaletteKind::History,
            options: PaletteOpenOptions::default(),
        }
    }

    pub fn ui_snapshot(&self, app: &AppState) -> HistoryUiSnapshot {
        let mut entries = Vec::new();
        for (i, entry) in self.forward_stack.iter().rev().enumerate().rev() {
            entries.push(history_palette_entry(entry, (i as isize) + 1, false));
        }
        entries.push(HistoryPaletteEntry {
            id: PaletteCandidateId::new(format!("current-{}", app.current_page)),
            display_index: 0,
            page: PageIndex::zero_based(app.current_page),
            reason: self
                .current_reason
                .as_ref()
                .map(history_palette_reason)
                .unwrap_or(HistoryPaletteReason::PageOnly),
            is_current: true,
        });
        for (i, entry) in self.back_stack.iter().rev().enumerate() {
            entries.push(history_palette_entry(entry, -((i as isize) + 1), false));
        }
        HistoryUiSnapshot {
            entries: entries.into(),
        }
    }

    pub fn on_event(&mut self, event: &AppEvent) {
        let AppEvent::PageChanged {
            from,
            to: _,
            reason,
            ..
        } = event
        else {
            return;
        };

        if self.suppress_next_record {
            self.suppress_next_record = false;
            return;
        }

        match record_policy(reason) {
            RecordPolicy::Record => {
                self.materialize_departed_page(*from, true);
                self.current_reason = Some(reason.clone());
                self.forward_stack.clear();
            }
            RecordPolicy::SkipAndClearForward => {
                self.materialize_departed_page(*from, false);
                self.current_reason = None;
                self.forward_stack.clear();
            }
            RecordPolicy::SkipAndKeepStacks => {
                self.current_reason = Some(reason.clone());
            }
        }
    }

    fn push_back(&mut self, entry: HistoryEntry) {
        if self.back_stack.len() >= HISTORY_CAPACITY {
            self.back_stack.pop_front();
        }
        self.back_stack.push_back(entry);
    }

    fn push_forward(&mut self, entry: HistoryEntry) {
        if self.forward_stack.len() >= HISTORY_CAPACITY {
            self.forward_stack.pop_front();
        }
        self.forward_stack.push_back(entry);
    }

    fn materialize_departed_page(&mut self, page: usize, include_unreasoned: bool) {
        let departed_reason = self.current_reason.clone();
        if departed_reason.is_none() && !include_unreasoned {
            return;
        }

        if let Some(last) = self.back_stack.back_mut()
            && last.page == page
        {
            if let Some(reason) = departed_reason {
                last.reason = Some(reason);
            }
            return;
        }

        self.push_back(HistoryEntry {
            page,
            reason: departed_reason,
        });
    }

    fn find_reason_for_page(&self, page: usize) -> Option<NavReason> {
        self.back_stack
            .iter()
            .rev()
            .find(|entry| entry.page == page)
            .and_then(|entry| entry.reason.clone())
            .or_else(|| {
                self.forward_stack
                    .iter()
                    .rev()
                    .find(|entry| entry.page == page)
                    .and_then(|entry| entry.reason.clone())
            })
    }
}

fn history_palette_entry(
    entry: &HistoryEntry,
    display_index: isize,
    is_current: bool,
) -> HistoryPaletteEntry {
    HistoryPaletteEntry {
        id: PaletteCandidateId::new(format!("history-{}-{display_index}", entry.page)),
        display_index,
        page: PageIndex::zero_based(entry.page),
        reason: entry
            .reason
            .as_ref()
            .map(history_palette_reason)
            .unwrap_or(HistoryPaletteReason::PageOnly),
        is_current,
    }
}

fn history_palette_reason(reason: &NavReason) -> HistoryPaletteReason {
    match reason {
        NavReason::Step => HistoryPaletteReason::PageOnly,
        NavReason::PageGoto(kind) => match kind {
            PageGotoKind::First => HistoryPaletteReason::Goto("first-page".to_string()),
            PageGotoKind::Last => HistoryPaletteReason::Goto("last-page".to_string()),
            PageGotoKind::Specific => HistoryPaletteReason::PageOnly,
        },
        NavReason::Search { query } if query.is_empty() => HistoryPaletteReason::PageOnly,
        NavReason::Search { query } => HistoryPaletteReason::Search(query.clone()),
        NavReason::History(op) => match op {
            HistoryOp::Back | HistoryOp::Forward | HistoryOp::Goto => {
                HistoryPaletteReason::PageOnly
            }
        },
        NavReason::Outline { title } if title.is_empty() => HistoryPaletteReason::PageOnly,
        NavReason::Outline { title } => HistoryPaletteReason::Outline(title.clone()),
        NavReason::LayoutNormalize => HistoryPaletteReason::PageOnly,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordPolicy {
    Record,
    SkipAndClearForward,
    SkipAndKeepStacks,
}

fn record_policy(reason: &NavReason) -> RecordPolicy {
    match reason {
        NavReason::PageGoto(_) | NavReason::Search { .. } | NavReason::Outline { .. } => {
            RecordPolicy::Record
        }
        NavReason::Step | NavReason::LayoutNormalize => RecordPolicy::SkipAndClearForward,
        NavReason::History(_) => RecordPolicy::SkipAndKeepStacks,
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryEntry, HistoryState};
    use crate::app::{AppState, PageLayoutMode};
    use crate::event::{AppEvent, NavReason, PageGotoKind};
    use crate::history::palette::HistoryPaletteReason;

    #[test]
    fn destination_reason_is_stored_on_the_destination_page() {
        let mut state = HistoryState::default();

        state.on_event(&AppEvent::PageChanged {
            from: 0,
            to: 5,
            reason: NavReason::Search {
                query: "needle".to_string(),
            },
        });
        assert_eq!(state.back_stack.len(), 1);
        let first = state.back_stack.back().expect("origin should be stored");
        assert_eq!(first.page, 0);
        assert!(first.reason.is_none());

        state.on_event(&AppEvent::PageChanged {
            from: 5,
            to: 7,
            reason: NavReason::PageGoto(PageGotoKind::Specific),
        });

        let last = state
            .back_stack
            .back()
            .expect("destination should be stored");
        assert_eq!(last.page, 5);
        assert!(matches!(
            last.reason.as_ref(),
            Some(NavReason::Search { query }) if query == "needle"
        ));
    }

    #[test]
    fn record_policy_dedupes_origin_page_index() {
        let mut state = HistoryState::default();
        state.back_stack.push_back(HistoryEntry {
            page: 3,
            reason: None,
        });

        state.on_event(&AppEvent::PageChanged {
            from: 3,
            to: 4,
            reason: NavReason::PageGoto(PageGotoKind::Specific),
        });

        assert_eq!(state.back_stack.len(), 1);
        assert_eq!(state.back_stack.back().expect("entry exists").page, 3);
    }

    #[test]
    fn same_page_search_records_current_reason_without_duplicate_origin() {
        let mut state = HistoryState::default();
        state.back_stack.push_back(HistoryEntry {
            page: 3,
            reason: None,
        });

        state.on_event(&AppEvent::PageChanged {
            from: 3,
            to: 3,
            reason: NavReason::Search {
                query: "needle".to_string(),
            },
        });

        assert_eq!(state.back_stack.len(), 1);
        let last = state.back_stack.back().expect("entry exists");
        assert_eq!(last.page, 3);
        assert!(last.reason.is_none());
        assert!(matches!(
            state.current_reason.as_ref(),
            Some(NavReason::Search { query }) if query == "needle"
        ));
    }

    #[test]
    fn same_page_search_refreshes_reason_on_deduped_page() {
        let mut state = HistoryState::default();

        state.on_event(&AppEvent::PageChanged {
            from: 1,
            to: 3,
            reason: NavReason::PageGoto(PageGotoKind::Specific),
        });
        state.on_event(&AppEvent::PageChanged {
            from: 3,
            to: 3,
            reason: NavReason::Search {
                query: "needle".to_string(),
            },
        });
        state.on_event(&AppEvent::PageChanged {
            from: 3,
            to: 8,
            reason: NavReason::PageGoto(PageGotoKind::Specific),
        });

        let last = state.back_stack.back().expect("deduped page should exist");
        assert_eq!(last.page, 3);
        assert!(matches!(
            last.reason.as_ref(),
            Some(NavReason::Search { query }) if query == "needle"
        ));
    }

    #[test]
    fn outline_navigation_is_recorded() {
        let mut state = HistoryState::default();

        state.on_event(&AppEvent::PageChanged {
            from: 2,
            to: 6,
            reason: NavReason::Outline {
                title: "Section".to_string(),
            },
        });

        assert_eq!(state.back_stack.len(), 1);
        assert_eq!(state.back_stack.back().expect("entry exists").page, 2);
        assert!(matches!(
            state.current_reason,
            Some(NavReason::Outline { title }) if title == "Section"
        ));
    }

    #[test]
    fn ui_snapshot_preserves_outline_titles_without_encoding() {
        let state = HistoryState {
            current_reason: Some(NavReason::Outline {
                title: "A | B; C".to_string(),
            }),
            ..HistoryState::default()
        };
        let app = AppState {
            current_page: 6,
            ..AppState::default()
        };

        let snapshot = state.ui_snapshot(&app);
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(
            snapshot.entries[0].reason,
            HistoryPaletteReason::Outline("A | B; C".to_string())
        );
    }

    #[test]
    fn back_without_movement_does_not_suppress_next_real_page_change() {
        let mut state = HistoryState {
            current_reason: Some(NavReason::Search {
                query: "needle".to_string(),
            }),
            ..HistoryState::default()
        };
        state.back_stack.push_back(HistoryEntry {
            page: 3,
            reason: None,
        });
        let mut app = AppState {
            current_page: 2,
            page_layout_mode: PageLayoutMode::Spread,
            ..AppState::default()
        };

        let (outcome, _) = state.back(&mut app, 8);
        assert!(matches!(outcome, crate::command::CommandOutcome::Applied));
        assert_eq!(app.current_page, 2);
        assert!(state.current_reason.is_none());

        state.on_event(&AppEvent::PageChanged {
            from: 2,
            to: 4,
            reason: NavReason::PageGoto(PageGotoKind::Specific),
        });

        assert!(state.forward_stack.is_empty());
        assert_eq!(state.back_stack.len(), 1);
        let last = state.back_stack.back().expect("entry should be recorded");
        assert_eq!(last.page, 2);
        assert!(last.reason.is_none());
    }

    #[test]
    fn back_and_forward_preserve_destination_reasons() {
        let mut state = HistoryState {
            current_reason: Some(NavReason::PageGoto(PageGotoKind::Specific)),
            ..HistoryState::default()
        };
        state.back_stack.push_back(HistoryEntry {
            page: 0,
            reason: None,
        });
        state.back_stack.push_back(HistoryEntry {
            page: 5,
            reason: Some(NavReason::Search {
                query: "needle".to_string(),
            }),
        });
        let mut app = AppState {
            current_page: 7,
            ..AppState::default()
        };

        let (outcome, _) = state.back(&mut app, 10);
        assert!(matches!(outcome, crate::command::CommandOutcome::Applied));
        assert_eq!(app.current_page, 5);
        assert!(matches!(
            state.current_reason.as_ref(),
            Some(NavReason::Search { query }) if query == "needle"
        ));

        let (outcome, _) = state.forward(&mut app, 10);
        assert!(matches!(outcome, crate::command::CommandOutcome::Applied));
        assert_eq!(app.current_page, 7);
        assert!(matches!(
            state.current_reason.as_ref(),
            Some(NavReason::PageGoto(PageGotoKind::Specific))
        ));
    }
}
