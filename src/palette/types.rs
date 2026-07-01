use super::kind::PaletteKind;
use crate::app::{AppState, Mode, PageLayoutMode, SpreadCoverPolicy};
use crate::command::Command;
use crate::error::AppResult;
use crate::extension::ExtensionUiSnapshot;
use crate::input::InputHistoryRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteInputMode {
    FilterCandidates,
    FreeText,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCandidateId(String);

impl PaletteCandidateId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PaletteCandidateId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for PaletteCandidateId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaletteOpenOptions {
    pub initial_input: String,
    pub initial_selection_id: Option<PaletteCandidateId>,
}

impl PaletteOpenOptions {
    pub fn input(input: impl Into<String>) -> Self {
        Self {
            initial_input: input.into(),
            initial_selection_id: None,
        }
    }

    pub fn input_with_selection(
        input: impl Into<String>,
        selection_id: impl Into<PaletteCandidateId>,
    ) -> Self {
        Self {
            initial_input: input.into(),
            initial_selection_id: Some(selection_id.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTextTone {
    Primary,
    Secondary,
    Highlight,
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

    pub fn highlight(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: PaletteTextTone::Highlight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCandidate {
    id: PaletteCandidateId,
    label: Vec<PaletteTextPart>,
    detail: Vec<PaletteTextPart>,
    match_texts: Vec<PaletteSearchText>,
}

impl PaletteCandidate {
    pub fn id(&self) -> &PaletteCandidateId {
        &self.id
    }

    pub fn label(&self) -> &[PaletteTextPart] {
        &self.label
    }

    pub fn detail(&self) -> &[PaletteTextPart] {
        &self.detail
    }

    pub fn match_texts(&self) -> &[PaletteSearchText] {
        &self.match_texts
    }

    pub fn plain_label_text(&self) -> String {
        join_palette_text_parts(&self.label)
    }

    pub fn plain_detail_text(&self) -> String {
        join_palette_text_parts(&self.detail)
    }

    pub fn plain_text(&self) -> String {
        let label = self.plain_label_text();
        let detail = self.plain_detail_text();
        if label.is_empty() {
            detail
        } else if detail.is_empty() {
            label
        } else {
            format!("{label} {detail}")
        }
    }

    pub fn match_text(&self) -> String {
        let text = join_palette_search_text_parts(&self.match_texts);
        if text.is_empty() {
            self.plain_text()
        } else {
            text
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageIndex {
    zero_based: usize,
}

impl PageIndex {
    pub fn zero_based(page: usize) -> Self {
        Self { zero_based: page }
    }

    pub fn zero_based_value(&self) -> usize {
        self.zero_based
    }

    pub fn display_number(&self) -> usize {
        self.zero_based + 1
    }

    pub fn label(&self) -> String {
        format!("p.{}", self.display_number())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteCellValue {
    Text(String),
    Parts(Vec<PaletteTextPart>),
    Page(PageIndex),
}

impl PaletteCellValue {
    fn display_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(parts) => join_palette_text_parts(parts),
            Self::Page(page) => page.label(),
        }
    }

    fn into_parts(self, tone: PaletteTextTone) -> Vec<PaletteTextPart> {
        match self {
            Self::Text(text) => vec![PaletteTextPart { text, tone }],
            Self::Parts(parts) => parts,
            Self::Page(page) => vec![PaletteTextPart {
                text: page.label(),
                tone,
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCell {
    value: PaletteCellValue,
    tone: PaletteTextTone,
    matchable: bool,
}

impl PaletteCell {
    pub fn matchable(value: PaletteCellValue, tone: PaletteTextTone) -> Self {
        Self {
            value,
            tone,
            matchable: true,
        }
    }

    pub fn decoration(value: impl Into<String>, tone: PaletteTextTone) -> Self {
        Self {
            value: PaletteCellValue::Text(value.into()),
            tone,
            matchable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteRow {
    id: PaletteCandidateId,
    label: Vec<PaletteCell>,
    detail: Vec<PaletteCell>,
}

impl PaletteRow {
    pub fn new(id: impl Into<PaletteCandidateId>) -> Self {
        Self {
            id: id.into(),
            label: Vec::new(),
            detail: Vec::new(),
        }
    }

    pub fn label_cell(mut self, cell: PaletteCell) -> Self {
        self.label.push(cell);
        self
    }

    pub fn detail_cell(mut self, cell: PaletteCell) -> Self {
        self.detail.push(cell);
        self
    }

    pub fn label_matchable_text(self, text: impl Into<String>) -> Self {
        self.label_cell(PaletteCell::matchable(
            PaletteCellValue::Text(text.into()),
            PaletteTextTone::Primary,
        ))
    }

    pub fn label_matchable_parts(self, parts: Vec<PaletteTextPart>) -> Self {
        self.label_cell(PaletteCell::matchable(
            PaletteCellValue::Parts(parts),
            PaletteTextTone::Primary,
        ))
    }

    pub fn label_decoration(self, text: impl Into<String>) -> Self {
        self.label_cell(PaletteCell::decoration(text, PaletteTextTone::Primary))
    }

    pub fn detail_matchable_text(self, text: impl Into<String>) -> Self {
        self.detail_cell(PaletteCell::matchable(
            PaletteCellValue::Text(text.into()),
            PaletteTextTone::Secondary,
        ))
    }

    pub fn detail_page(self, page: PageIndex) -> Self {
        self.detail_cell(PaletteCell::matchable(
            PaletteCellValue::Page(page),
            PaletteTextTone::Secondary,
        ))
    }

    pub fn into_candidate(self) -> PaletteCandidate {
        let match_texts = self
            .label
            .iter()
            .chain(self.detail.iter())
            .filter(|cell| cell.matchable)
            .map(|cell| PaletteSearchText::new(cell.value.display_text()))
            .collect();
        PaletteCandidate {
            id: self.id,
            label: render_cells(self.label),
            detail: render_cells(self.detail),
            match_texts,
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteAppSnapshot {
    pub current_page: usize,
    pub mode: Mode,
    pub page_layout_mode: PageLayoutMode,
    pub spread_cover_policy: SpreadCoverPolicy,
}

impl Default for PaletteAppSnapshot {
    fn default() -> Self {
        Self {
            current_page: 0,
            mode: Mode::Normal,
            page_layout_mode: PageLayoutMode::default(),
            spread_cover_policy: SpreadCoverPolicy::default(),
        }
    }
}

impl From<&AppState> for PaletteAppSnapshot {
    fn from(app: &AppState) -> Self {
        Self {
            current_page: app.current_page,
            mode: app.mode,
            page_layout_mode: app.page_layout_mode,
            spread_cover_policy: app.spread_cover_policy,
        }
    }
}

pub struct PaletteContext<'a> {
    pub app: PaletteAppSnapshot,
    pub extensions: &'a ExtensionUiSnapshot,
    pub kind: PaletteKind,
    pub input: &'a str,
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
    /// Returns the initially selected candidate index within `candidates`.
    ///
    /// Defaults to `None`, which keeps the manager's normal first-item selection.
    fn initial_selected_candidate(
        &self,
        _ctx: &PaletteContext<'_>,
        _candidates: &[PaletteCandidate],
    ) -> Option<usize> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItemView {
    pub label: Vec<PaletteTextPart>,
    pub detail: Vec<PaletteTextPart>,
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

fn render_cells(cells: Vec<PaletteCell>) -> Vec<PaletteTextPart> {
    cells
        .into_iter()
        .flat_map(|cell| cell.value.into_parts(cell.tone))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PageIndex, PaletteRow, PaletteTextPart, PaletteTextTone};

    #[test]
    fn plain_text_joins_left_and_right_segments() {
        let candidate = PaletteRow::new("id")
            .label_matchable_parts(vec![
                PaletteTextPart::primary("open"),
                PaletteTextPart::secondary(" now"),
            ])
            .detail_matchable_text("Command")
            .into_candidate();

        assert_eq!(candidate.plain_label_text(), "open now");
        assert_eq!(candidate.plain_detail_text(), "Command");
        assert_eq!(candidate.plain_text(), "open now Command");
    }

    #[test]
    fn plain_text_preserves_internal_spacing_in_parts() {
        let candidate = PaletteRow::new("id")
            .label_matchable_parts(vec![
                PaletteTextPart::primary("open"),
                PaletteTextPart::primary(" "),
            ])
            .detail_matchable_text("Command")
            .into_candidate();

        assert_eq!(candidate.plain_label_text(), "open ");
        assert_eq!(candidate.plain_text(), "open  Command");
    }

    #[test]
    fn match_text_comes_from_matchable_cells() {
        let candidate = PaletteRow::new("id")
            .label_matchable_text("page")
            .label_decoration(" ")
            .detail_page(PageIndex::zero_based(11))
            .into_candidate();

        assert_eq!(candidate.match_text(), "page p.12");
    }

    #[test]
    fn plain_text_uses_rendered_page_label() {
        let candidate = PaletteRow::new("id")
            .label_matchable_text("current")
            .detail_page(PageIndex::zero_based(11))
            .into_candidate();

        assert_eq!(candidate.plain_text(), "current p.12");
        assert_eq!(candidate.match_text(), "current p.12");
    }

    #[test]
    fn constructors_set_expected_tones() {
        assert_eq!(PaletteTextPart::primary("a").tone, PaletteTextTone::Primary);
        assert_eq!(
            PaletteTextPart::secondary("b").tone,
            PaletteTextTone::Secondary
        );
        assert_eq!(
            PaletteTextPart::highlight("c").tone,
            PaletteTextTone::Highlight
        );
    }
}
