use std::collections::VecDeque;

use crate::app::{AppState, PaletteRequest};
use crate::command::{ActionId, CommandOutcome};
use crate::error::{AppError, AppResult};
use crate::event::{AppEvent, NavReason};
use crate::palette::PaletteKind;

const HISTORY_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
struct HistoryEntry {
    page: usize,
    reason: NavReason,
}

#[derive(Default)]
pub struct HistoryState {
    back_stack: VecDeque<HistoryEntry>,
    forward_stack: VecDeque<HistoryEntry>,
    suppress_next_record: bool,
}

impl HistoryState {
    pub fn back(&mut self, app: &mut AppState) -> CommandOutcome {
        let Some(target) = self.back_stack.pop_back() else {
            app.status.last_action_id = Some(ActionId::HistoryBack);
            app.status.message = "history back is empty".to_string();
            return CommandOutcome::Noop;
        };

        self.push_forward(HistoryEntry {
            page: app.current_page,
            reason: NavReason::History,
        });
        self.suppress_next_record = true;
        app.current_page = target.page;
        app.status.last_action_id = Some(ActionId::HistoryBack);
        app.status.message = format!("history back -> page {}", app.current_page + 1);
        CommandOutcome::Applied
    }

    pub fn forward(&mut self, app: &mut AppState) -> CommandOutcome {
        let Some(target) = self.forward_stack.pop_back() else {
            app.status.last_action_id = Some(ActionId::HistoryForward);
            app.status.message = "history forward is empty".to_string();
            return CommandOutcome::Noop;
        };

        self.push_back(HistoryEntry {
            page: app.current_page,
            reason: NavReason::History,
        });
        self.suppress_next_record = true;
        app.current_page = target.page;
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

        let target = page - 1;
        app.status.last_action_id = Some(ActionId::HistoryGoto);
        if app.current_page == target {
            app.status.message = format!("already at page {page}");
            return Ok(CommandOutcome::Noop);
        }

        self.push_back(HistoryEntry {
            page: app.current_page,
            reason: NavReason::History,
        });
        self.suppress_next_record = true;
        app.current_page = target;
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
            from, to, reason, ..
        } = event
        else {
            return;
        };
        if from == to {
            return;
        }

        if self.suppress_next_record {
            self.suppress_next_record = false;
            return;
        }

        let is_jump = from.abs_diff(*to) > 1;
        if is_jump {
            self.push_back(HistoryEntry {
                page: *from,
                reason: reason.clone(),
            });
            self.forward_stack.clear();
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
            buf.push(',');
            buf.push_str(&format_reason(&entry.reason));
        }
        buf.push_str(&format!("|c:{current_page}|f:"));
        for (i, entry) in self.forward_stack.iter().rev().enumerate() {
            if i > 0 {
                buf.push(';');
            }
            buf.push_str(&entry.page.to_string());
            buf.push(',');
            buf.push_str(&format_reason(&entry.reason));
        }
        buf
    }
}

fn format_reason(reason: &NavReason) -> String {
    match reason {
        NavReason::Step => "Step".to_string(),
        NavReason::Jump => "Jump".to_string(),
        NavReason::Search(query) if query.is_empty() => "Search".to_string(),
        NavReason::Search(query) => format!("Search: {query}"),
        NavReason::History => "History".to_string(),
    }
}
