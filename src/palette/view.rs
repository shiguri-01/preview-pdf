use super::kind::PaletteKind;
use super::text::PaletteTextPart;

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
