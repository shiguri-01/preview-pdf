use crate::app::{NoticeAction, PaletteRequest};
use crate::event::AppEvent;
use crate::input::InputHistoryRecord;

use super::catalog::CommandRequest;
use super::types::CommandOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLifecycleEffect {
    None,
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandEffects {
    pub notice: NoticeAction,
    pub events: Vec<AppEvent>,
    pub palette_requests: Vec<PaletteRequest>,
    pub input_history_records: Vec<InputHistoryRecord>,
    pub follow_up_commands: Vec<CommandRequest>,
    pub lifecycle: CommandLifecycleEffect,
}

impl CommandEffects {
    pub fn new(notice: NoticeAction) -> Self {
        Self {
            notice,
            events: Vec::new(),
            palette_requests: Vec::new(),
            input_history_records: Vec::new(),
            follow_up_commands: Vec::new(),
            lifecycle: CommandLifecycleEffect::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CommandExecution {
    pub outcome: CommandOutcome,
    pub effects: CommandEffects,
}

impl CommandExecution {
    pub(super) fn from_notice_result((outcome, notice): (CommandOutcome, NoticeAction)) -> Self {
        Self {
            outcome,
            effects: CommandEffects::new(notice),
        }
    }

    pub(super) fn applied() -> Self {
        Self::from_notice_result((CommandOutcome::Applied, NoticeAction::Clear))
    }

    pub(super) fn noop() -> Self {
        Self::from_notice_result((CommandOutcome::Noop, NoticeAction::Clear))
    }

    pub(super) fn with_palette_request(mut self, request: PaletteRequest) -> Self {
        self.effects.palette_requests.push(request);
        self
    }

    pub(super) fn with_input_history_record(mut self, record: InputHistoryRecord) -> Self {
        self.effects.input_history_records.push(record);
        self
    }

    pub(super) fn with_follow_up(mut self, request: CommandRequest) -> Self {
        self.effects.follow_up_commands.push(request);
        self
    }

    pub(super) fn with_lifecycle(mut self, lifecycle: CommandLifecycleEffect) -> Self {
        self.effects.lifecycle = lifecycle;
        self
    }
}
