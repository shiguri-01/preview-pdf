use super::text::{PaletteTextPart, join_palette_text_parts};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PaletteSearchText {
    text: String,
}

impl PaletteSearchText {
    pub(super) fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
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
    pub(super) fn from_row(
        id: PaletteCandidateId,
        label: Vec<PaletteTextPart>,
        detail: Vec<PaletteTextPart>,
        match_texts: Vec<PaletteSearchText>,
    ) -> Self {
        Self {
            id,
            label,
            detail,
            match_texts,
        }
    }

    pub fn id(&self) -> &PaletteCandidateId {
        &self.id
    }

    pub fn label(&self) -> &[PaletteTextPart] {
        &self.label
    }

    pub fn detail(&self) -> &[PaletteTextPart] {
        &self.detail
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
