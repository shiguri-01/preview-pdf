use crossterm::event::Event;
use std::fmt;

use crate::app::Mode;
use crate::backend::SharedPdfBackend;
use crate::command::{CommandId, CommandOutcome, CommandRequest};
use crate::presenter::PresenterBackgroundEvent;
use crate::render::worker::RenderWorkerResult;

/// Describes *why* a page navigation occurred.
///
/// Defined in core; extensions consume this for recording/display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavReason {
    /// Incremental movement (next-page, prev-page).
    Step,
    /// Direct page goto-style movement (first-page, last-page, goto-page).
    PageGoto(PageGotoKind),
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
pub enum PageGotoKind {
    First,
    Last,
    Specific,
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
        id: CommandId,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentReloadReason {
    Manual,
    FileChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocumentReloadRequest {
    pub(crate) reason: DocumentReloadReason,
    pub(crate) retry: bool,
    pub(crate) generation: u64,
}

impl DocumentReloadRequest {
    pub(crate) fn new(reason: DocumentReloadReason) -> Self {
        Self {
            reason,
            retry: false,
            generation: 0,
        }
    }

    pub(crate) fn retry(reason: DocumentReloadReason, generation: u64) -> Self {
        Self {
            reason,
            retry: true,
            generation,
        }
    }

    pub(crate) fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
}

pub(crate) struct DocumentReloadResult {
    pub(crate) reason: DocumentReloadReason,
    pub(crate) generation: u64,
    pub(crate) result: Result<SharedPdfBackend, String>,
}

impl fmt::Debug for DocumentReloadResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let result = match &self.result {
            Ok(pdf) => format!("Ok(doc_id: {}, pages: {})", pdf.doc_id(), pdf.page_count()),
            Err(message) => format!("Err({message:?})"),
        };
        f.debug_struct("DocumentReloadResult")
            .field("reason", &self.reason)
            .field("generation", &self.generation)
            .field("result", &result)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) enum DomainEvent {
    Input(Event),
    InputError(String),
    Command(CommandRequest),
    App(AppEvent),
    ReloadDocument(DocumentReloadRequest),
    DocumentReloaded(DocumentReloadResult),
    RenderComplete(RenderWorkerResult),
    EncodeComplete(PresenterBackgroundEvent),
    PrefetchTick,
    RedrawTick,
    Wake,
}
