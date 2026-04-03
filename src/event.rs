use crossterm::event::Event;

use crate::app::Mode;
use crate::command::{ActionId, CommandOutcome, CommandRequest};
use crate::presenter::PresenterBackgroundEvent;
use crate::render::worker::RenderWorkerResult;

/// Describes *why* a page navigation occurred.
///
/// Defined in core; extensions consume this for recording/display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavReason {
    /// Incremental movement (next-page, prev-page).
    Step,
    /// Direct goto-style movement (first-page, last-page, goto-page).
    Goto(GotoKind),
    /// Search-driven navigation. Carries the query string.
    Search { query: String },
    /// History traversal (history-back, history-forward, history-goto).
    History(HistoryOp),
    /// Navigation initiated from the PDF outline.
    Outline { title: String },
    /// Layout-change normalization moved the anchor page.
    LayoutNormalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GotoKind {
    FirstPage,
    LastPage,
    SpecificPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    Back,
    Forward,
    Goto,
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
    Command(CommandRequest),
    Quit,
    App(AppEvent),
    RenderComplete(RenderWorkerResult),
    EncodeComplete(PresenterBackgroundEvent),
    PrefetchTick,
    RedrawTick,
    Wake,
}
