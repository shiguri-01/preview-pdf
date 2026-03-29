use crate::palette::PaletteCandidate;

pub trait CandidateMatcher: Send + Sync {
    fn select(&self, input: &str, candidates: &[PaletteCandidate]) -> Vec<usize>;
}

#[derive(Debug, Default)]
pub struct ContainsMatcher;

impl CandidateMatcher for ContainsMatcher {
    fn select(&self, input: &str, candidates: &[PaletteCandidate]) -> Vec<usize> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return (0..candidates.len()).collect();
        }

        let query = trimmed.to_ascii_lowercase();
        let mut prefix = Vec::new();
        let mut contains = Vec::new();

        for (idx, candidate) in candidates.iter().enumerate() {
            let text = candidate.search_text().to_ascii_lowercase();
            if text.starts_with(&query) {
                prefix.push(idx);
            } else if text.contains(&query) {
                contains.push(idx);
            }
        }

        prefix.extend(contains);
        prefix
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::{
        PaletteCandidate, PalettePayload, PaletteSearchText, PaletteTextPart, PaletteTextTone,
    };

    use super::{CandidateMatcher, ContainsMatcher};

    fn candidate(label: &str) -> PaletteCandidate {
        PaletteCandidate {
            id: label.to_string(),
            left: vec![PaletteTextPart {
                text: label.to_string(),
                tone: PaletteTextTone::Primary,
            }],
            right: Vec::new(),
            search_texts: vec![PaletteSearchText {
                text: label.to_string(),
            }],
            payload: PalettePayload::None,
        }
    }

    #[test]
    fn contains_matcher_prioritizes_prefix_hits() {
        let matcher = ContainsMatcher;
        let all = vec![candidate("zoom-in"), candidate("inbox"), candidate("pan")];

        let selected = matcher.select("in", &all);
        assert_eq!(selected, vec![1, 0]);
    }

    #[test]
    fn contains_matcher_uses_structured_search_texts() {
        let matcher = ContainsMatcher;
        let all = vec![
            PaletteCandidate {
                id: "alpha".to_string(),
                left: vec![PaletteTextPart {
                    text: "visible".to_string(),
                    tone: PaletteTextTone::Primary,
                }],
                right: Vec::new(),
                search_texts: vec![PaletteSearchText {
                    text: "structured-match".to_string(),
                }],
                payload: PalettePayload::None,
            },
            candidate("beta"),
        ];

        let selected = matcher.select("structured", &all);
        assert_eq!(selected, vec![0]);
    }
}
