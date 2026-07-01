#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTextTone {
    Primary,
    Secondary,
    Highlight,
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

pub(super) fn join_palette_text_parts(parts: &[PaletteTextPart]) -> String {
    let mut text = String::new();
    for part in parts {
        text.push_str(&part.text);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{PaletteTextPart, PaletteTextTone};

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
