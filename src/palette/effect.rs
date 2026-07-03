use super::kind::PaletteKind;
use super::request::PaletteOpenOptions;
use crate::command::Command;
use crate::input::InputHistoryRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PalettePostAction {
    Close,
    Reopen {
        kind: PaletteKind,
        options: PaletteOpenOptions,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaletteSubmitEffect {
    Close,
    Reopen {
        kind: PaletteKind,
        options: PaletteOpenOptions,
    },
    Dispatch {
        command: Command,
        history_record: Option<InputHistoryRecord>,
        next: PalettePostAction,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteTabEffect {
    Noop,
    SetInput {
        value: String,
        move_cursor_to_end: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteSubmitAction {
    pub session_id: u64,
    pub effect: PaletteSubmitEffect,
}
