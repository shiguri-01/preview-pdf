use crossterm::event::Event;

use crate::app::Mode;
use crate::command::{ActionId, Command, CommandOutcome};
use crate::render::worker::RenderWorkerResult;

/// Describes *why* a page navigation occurred.
///
/// Defined in core; extensions consume this for recording/display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavReason {
    /// Incremental movement (next-page, prev-page).
    Step,
    /// Direct jump (first-page, last-page, goto-page).
    Jump,
    /// Search-driven navigation. Carries the query string.
    Search(String),
    /// History traversal (history-back, history-forward, history-goto).
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    CommandExecuted {
        id: ActionId,
        outcome: CommandOutcome,
    },
    PageChanged {
        from: usize,
        to: usize,
        reason: NavReason,
    },
    ModeChanged {
        from: Mode,
        to: Mode,
    },
}

#[derive(Debug)]
pub(crate) enum DomainEvent {
    Input(Event),
    InputError(String),
    Command(Command),
    App(AppEvent),
    RenderComplete(RenderWorkerResult),
    PrefetchTick,
    RedrawTick,
    Wake,
}
