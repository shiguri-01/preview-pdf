use super::candidate::{PaletteCandidate, PaletteCandidateId, PaletteSearchText};
use super::text::{PaletteTextPart, PaletteTextTone, join_palette_text_parts};

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
enum PaletteCellValue {
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
struct PaletteCell {
    value: PaletteCellValue,
    tone: PaletteTextTone,
    matchable: bool,
}

impl PaletteCell {
    fn matchable(value: PaletteCellValue, tone: PaletteTextTone) -> Self {
        Self {
            value,
            tone,
            matchable: true,
        }
    }

    fn decoration(value: impl Into<String>, tone: PaletteTextTone) -> Self {
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

    pub fn label_matchable_text(mut self, text: impl Into<String>) -> Self {
        self.label.push(PaletteCell::matchable(
            PaletteCellValue::Text(text.into()),
            PaletteTextTone::Primary,
        ));
        self
    }

    pub fn label_matchable_parts(mut self, parts: Vec<PaletteTextPart>) -> Self {
        self.label.push(PaletteCell::matchable(
            PaletteCellValue::Parts(parts),
            PaletteTextTone::Primary,
        ));
        self
    }

    pub fn label_decoration(mut self, text: impl Into<String>) -> Self {
        self.label
            .push(PaletteCell::decoration(text, PaletteTextTone::Primary));
        self
    }

    pub fn detail_matchable_text(mut self, text: impl Into<String>) -> Self {
        self.detail.push(PaletteCell::matchable(
            PaletteCellValue::Text(text.into()),
            PaletteTextTone::Secondary,
        ));
        self
    }

    pub fn detail_page(mut self, page: PageIndex) -> Self {
        self.detail.push(PaletteCell::matchable(
            PaletteCellValue::Page(page),
            PaletteTextTone::Secondary,
        ));
        self
    }

    pub fn into_candidate(self) -> PaletteCandidate {
        let match_texts = self
            .label
            .iter()
            .chain(self.detail.iter())
            .filter(|cell| cell.matchable)
            .map(|cell| PaletteSearchText::new(cell.value.display_text()))
            .collect();
        PaletteCandidate::from_row(
            self.id,
            render_cells(self.label),
            render_cells(self.detail),
            match_texts,
        )
    }
}

fn render_cells(cells: Vec<PaletteCell>) -> Vec<PaletteTextPart> {
    cells
        .into_iter()
        .flat_map(|cell| cell.value.into_parts(cell.tone))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PageIndex, PaletteRow};
    use crate::palette::PaletteTextPart;

    #[test]
    fn plain_text_joins_label_and_detail_segments() {
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
}
