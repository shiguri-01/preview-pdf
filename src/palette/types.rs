use super::kind::PaletteKind;
use crate::app::AppState;
use crate::command::Command;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteInputMode {
    FilterCandidates,
    FreeText,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PalettePayload {
    None,
    Opaque(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCandidate {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub payload: PalettePayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PalettePostAction {
    Close,
    Reopen {
        kind: PaletteKind,
        seed: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaletteSubmitEffect {
    Close,
    Reopen {
        kind: PaletteKind,
        seed: Option<String>,
    },
    Dispatch {
        command: Command,
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

pub struct PaletteContext<'a> {
    pub app: &'a AppState,
    pub kind: PaletteKind,
    pub input: &'a str,
    pub seed: Option<&'a str>,
}

pub trait PaletteProvider: Send + Sync {
    fn kind(&self) -> PaletteKind;
    fn title(&self, ctx: &PaletteContext<'_>) -> String;
    fn input_mode(&self) -> PaletteInputMode;
    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>>;
    fn on_tab(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteTabEffect> {
        Ok(PaletteTabEffect::Noop)
    }
    fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect>;
    fn assistive_text(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        None
    }
    /// Returns the initial input text when the palette opens.
    ///
    /// Defaults to the seed value. Override to decouple seed (data) from
    /// the visible input field.
    fn initial_input(&self, seed: Option<&str>) -> String {
        seed.unwrap_or("").to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItemView {
    pub label: String,
    pub detail: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteView {
    pub title: String,
    pub kind: PaletteKind,
    pub input: String,
    pub cursor: usize,
    pub assistive_text: Option<String>,
    pub items: Vec<PaletteItemView>,
    /// Index of the selected item within `items` (manager-authoritative).
    pub selected_idx: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteSubmitAction {
    pub session_id: u64,
    pub effect: PaletteSubmitEffect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaletteKeyResult {
    Consumed { redraw: bool },
    CloseRequested { session_id: u64 },
    Submit(PaletteSubmitAction),
}
