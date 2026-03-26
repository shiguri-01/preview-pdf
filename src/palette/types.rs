use super::kind::PaletteKind;
use crate::app::AppState;
use crate::command::Command;
use crate::error::AppResult;
use crate::extension::ExtensionUiSnapshot;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTextTone {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSearchText {
    pub text: String,
}

impl PaletteSearchText {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteTextPart {
    pub text: String,
    pub tone: PaletteTextTone,
}

impl PaletteTextPart {
    pub fn primary(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: PaletteTextTone::Primary,
        }
    }

    pub fn secondary(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: PaletteTextTone::Secondary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCandidate {
    pub id: String,
    pub left: Vec<PaletteTextPart>,
    pub right: Vec<PaletteTextPart>,
    pub search_texts: Vec<PaletteSearchText>,
    pub payload: PalettePayload,
}

impl PaletteCandidate {
    pub fn plain_left_text(&self) -> String {
        join_palette_text_parts(&self.left)
    }

    pub fn plain_right_text(&self) -> String {
        join_palette_text_parts(&self.right)
    }

    pub fn plain_text(&self) -> String {
        let left = self.plain_left_text();
        let right = self.plain_right_text();
        if left.is_empty() {
            right
        } else if right.is_empty() {
            left
        } else {
            format!("{left} {right}")
        }
    }

    pub fn search_text(&self) -> String {
        let search = join_palette_search_text_parts(&self.search_texts);
        if search.is_empty() {
            self.plain_text()
        } else {
            search
        }
    }
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
    pub extensions: &'a ExtensionUiSnapshot,
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
    /// Returns whether a changed input should reset the selected candidate to the first item.
    ///
    /// Defaults to `false` so providers that do not filter or reorder candidates by input keep
    /// their selection stable while typing.
    fn reset_selection_on_input_change(&self) -> bool {
        false
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
    pub left: Vec<PaletteTextPart>,
    pub right: Vec<PaletteTextPart>,
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

fn join_palette_text_parts(parts: &[PaletteTextPart]) -> String {
    let mut text = String::new();
    for part in parts {
        text.push_str(&part.text);
    }
    text
}

fn join_palette_search_text_parts(parts: &[PaletteSearchText]) -> String {
    let mut text = String::new();
    for part in parts {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&part.text);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{
        PaletteCandidate, PalettePayload, PaletteSearchText, PaletteTextPart, PaletteTextTone,
    };

    fn candidate(
        left: Vec<PaletteTextPart>,
        right: Vec<PaletteTextPart>,
        search_texts: Vec<PaletteSearchText>,
    ) -> PaletteCandidate {
        PaletteCandidate {
            id: "id".to_string(),
            left,
            right,
            search_texts,
            payload: PalettePayload::None,
        }
    }

    #[test]
    fn plain_text_joins_left_and_right_segments() {
        let candidate = candidate(
            vec![
                PaletteTextPart::primary("open"),
                PaletteTextPart::secondary(" now"),
            ],
            vec![PaletteTextPart::secondary("Command")],
            Vec::new(),
        );

        assert_eq!(candidate.plain_left_text(), "open now");
        assert_eq!(candidate.plain_right_text(), "Command");
        assert_eq!(candidate.plain_text(), "open now Command");
    }

    #[test]
    fn plain_text_preserves_internal_spacing_in_parts() {
        let candidate = candidate(
            vec![
                PaletteTextPart::primary("open"),
                PaletteTextPart::primary(" "),
            ],
            vec![PaletteTextPart::secondary("Command")],
            Vec::new(),
        );

        assert_eq!(candidate.plain_left_text(), "open ");
        assert_eq!(candidate.plain_text(), "open  Command");
    }

    #[test]
    fn search_text_falls_back_to_plain_text_when_empty() {
        let candidate = candidate(
            vec![PaletteTextPart::primary("page")],
            vec![PaletteTextPart::secondary("12")],
            Vec::new(),
        );

        assert_eq!(candidate.search_text(), "page 12");
    }

    #[test]
    fn search_text_joins_search_segments_with_spaces() {
        let candidate = candidate(
            vec![PaletteTextPart::primary("page")],
            Vec::new(),
            vec![
                PaletteSearchText::new("page 12"),
                PaletteSearchText::new("current"),
            ],
        );

        assert_eq!(candidate.search_text(), "page 12 current");
    }

    #[test]
    fn constructors_set_expected_tones() {
        assert_eq!(PaletteTextPart::primary("a").tone, PaletteTextTone::Primary);
        assert_eq!(
            PaletteTextPart::secondary("b").tone,
            PaletteTextTone::Secondary
        );
    }
}
