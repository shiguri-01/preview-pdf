use crate::command::CommandRequest;
use crate::event::DomainEvent;
use crate::perf::RedrawReason;

#[derive(Default)]
pub(super) struct LoopEffects {
    commands: Vec<CommandRequest>,
    events: Vec<DomainEvent>,
    redraws: Vec<RedrawReason>,
    quit_requested: bool,
}

impl LoopEffects {
    pub(super) fn none() -> Self {
        Self::default()
    }

    pub(super) fn from_commands(commands: Vec<CommandRequest>, quit_requested: bool) -> Self {
        Self {
            commands,
            quit_requested,
            ..Self::default()
        }
    }

    pub(super) fn push_event(&mut self, event: DomainEvent) {
        self.events.push(event);
    }

    pub(super) fn request_redraw(&mut self, reason: RedrawReason) {
        self.redraws.push(reason);
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        Vec<CommandRequest>,
        Vec<DomainEvent>,
        Vec<RedrawReason>,
        bool,
    ) {
        (
            self.commands,
            self.events,
            self.redraws,
            self.quit_requested,
        )
    }
}
