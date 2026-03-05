use std::collections::VecDeque;

use crate::app::{AppState, PaletteRequest};
use crate::command::{ActionId, CommandOutcome};
use crate::error::{AppError, AppResult};
use crate::event::{AppEvent, GotoKind, HistoryOp, NavReason};
use crate::palette::PaletteKind;

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

impl HistoryState {
    pub fn back(&mut self, app: &mut AppState, page_count: usize) -> CommandOutcome {
        let Some(target) = self.back_stack.pop_back() else {
            app.status.last_action_id = Some(ActionId::HistoryBack);
            app.status.message = "history back is empty".to_string();
            return CommandOutcome::Noop;
        };

        self.push_forward(HistoryEntry {
            page: app.current_page,
            reason: self.current_reason.clone(),
        });
        let normalized_target = app.normalize_page_for_layout(target.page, page_count);
        self.suppress_next_record = app.current_page != normalized_target;
        app.current_page = normalized_target;
        self.current_reason = target.reason;
        app.status.last_action_id = Some(ActionId::HistoryBack);
        app.status.message = format!("history back -> page {}", app.current_page + 1);
        CommandOutcome::Applied
    }

    pub fn forward(&mut self, app: &mut AppState, page_count: usize) -> CommandOutcome {
        let Some(target) = self.forward_stack.pop_back() else {
            app.status.last_action_id = Some(ActionId::HistoryForward);
            app.status.message = "history forward is empty".to_string();
            return CommandOutcome::Noop;
        };

        self.push_back(HistoryEntry {
            page: app.current_page,
            reason: self.current_reason.clone(),
        });
        let normalized_target = app.normalize_page_for_layout(target.page, page_count);
        self.suppress_next_record = app.current_page != normalized_target;
        app.current_page = normalized_target;
        self.current_reason = target.reason;
        app.status.last_action_id = Some(ActionId::HistoryForward);
        app.status.message = format!("history forward -> page {}", app.current_page + 1);
        CommandOutcome::Applied
    }

    pub fn goto(
        &mut self,
        app: &mut AppState,
        page_count: usize,
        page: usize,
    ) -> AppResult<CommandOutcome> {
        if page < 1 {
            return Err(AppError::invalid_argument("page number must be >= 1"));
        }
        if page > page_count {
            return Err(AppError::invalid_argument(
                "page number exceeds document length",
            ));
        }

        let target = app.normalize_page_for_layout(page - 1, page_count);
        app.status.last_action_id = Some(ActionId::HistoryGoto);
        if app.current_page == target {
            app.status.message = format!("already at page {page}");
            return Ok(CommandOutcome::Noop);
        }
        let target_reason = self.find_reason_for_page(target);

        self.push_back(HistoryEntry {
            page: app.current_page,
            reason: self.current_reason.clone(),
        });
        self.suppress_next_record = true;
        app.current_page = target;
        self.current_reason = target_reason;
        app.status.message = format!("history goto -> page {}", app.current_page + 1);
        Ok(CommandOutcome::Applied)
    }

    pub fn open_palette(
        &self,
        app: &mut AppState,
        palette_requests: &mut VecDeque<PaletteRequest>,
    ) -> CommandOutcome {
        let seed = self.serialize_seed(app.current_page);
        palette_requests.push_back(PaletteRequest::Open {
            kind: PaletteKind::History,
            seed: Some(seed),
        });
        app.status.last_action_id = Some(ActionId::History);
        app.status.message = "opening history palette".to_string();
        CommandOutcome::Applied
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

    fn serialize_seed(&self, current_page: usize) -> String {
        let mut buf = String::new();
        buf.push_str("b:");
        for (i, entry) in self.back_stack.iter().enumerate() {
            if i > 0 {
                buf.push(';');
            }
            buf.push_str(&entry.page.to_string());
            if let Some(reason) = entry.reason.as_ref() {
                buf.push(',');
                buf.push_str(&format_reason(reason));
            }
        }
        buf.push_str("|c:");
        buf.push_str(&current_page.to_string());
        if let Some(reason) = self.current_reason.as_ref() {
            buf.push(',');
            buf.push_str(&format_reason(reason));
        }
        buf.push_str("|f:");
        for (i, entry) in self.forward_stack.iter().rev().enumerate() {
            if i > 0 {
                buf.push(';');
            }
            buf.push_str(&entry.page.to_string());
            if let Some(reason) = entry.reason.as_ref() {
                buf.push(',');
                buf.push_str(&format_reason(reason));
            }
        }
        buf
    }

    fn materialize_departed_page(&mut self, page: usize, include_unreasoned: bool) {
        let departed_reason = self.current_reason.clone();
        if departed_reason.is_none() && !include_unreasoned {
            return;
        }

        if let Some(last) = self.back_stack.back_mut()
            && last.page == page
        {
            if last.reason.is_none() && departed_reason.is_some() {
                last.reason = departed_reason;
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

fn format_reason(reason: &NavReason) -> String {
    match reason {
        NavReason::Step => "Step".to_string(),
        NavReason::Goto(kind) => match kind {
            GotoKind::FirstPage => "Goto:first-page".to_string(),
            GotoKind::LastPage => "Goto:last-page".to_string(),
            GotoKind::SpecificPage => "Goto:goto-page".to_string(),
        },
        NavReason::Search { query } if query.is_empty() => "Search".to_string(),
        NavReason::Search { query } => format!("Search: {query}"),
        NavReason::History(op) => match op {
            HistoryOp::Back => "History:back".to_string(),
            HistoryOp::Forward => "History:forward".to_string(),
            HistoryOp::Goto => "History:goto".to_string(),
        },
        NavReason::LayoutNormalize => "LayoutNormalize".to_string(),
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
        NavReason::Goto(_) | NavReason::Search { .. } => RecordPolicy::Record,
        NavReason::Step | NavReason::LayoutNormalize => RecordPolicy::SkipAndClearForward,
        NavReason::History(_) => RecordPolicy::SkipAndKeepStacks,
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryEntry, HistoryState};
    use crate::app::{AppState, PageLayoutMode};
    use crate::event::{AppEvent, GotoKind, NavReason};

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
            reason: NavReason::Goto(GotoKind::SpecificPage),
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
            reason: NavReason::Goto(GotoKind::SpecificPage),
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

        let outcome = state.back(&mut app, 8);
        assert!(matches!(outcome, crate::command::CommandOutcome::Applied));
        assert_eq!(app.current_page, 2);
        assert!(state.current_reason.is_none());

        state.on_event(&AppEvent::PageChanged {
            from: 2,
            to: 4,
            reason: NavReason::Goto(GotoKind::SpecificPage),
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
            current_reason: Some(NavReason::Goto(GotoKind::SpecificPage)),
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

        let outcome = state.back(&mut app, 10);
        assert!(matches!(outcome, crate::command::CommandOutcome::Applied));
        assert_eq!(app.current_page, 5);
        assert!(matches!(
            state.current_reason.as_ref(),
            Some(NavReason::Search { query }) if query == "needle"
        ));

        let outcome = state.forward(&mut app, 10);
        assert!(matches!(outcome, crate::command::CommandOutcome::Applied));
        assert_eq!(app.current_page, 7);
        assert!(matches!(
            state.current_reason.as_ref(),
            Some(NavReason::Goto(GotoKind::SpecificPage))
        ));
    }
}
