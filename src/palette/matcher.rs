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
            let text = candidate.match_text().to_ascii_lowercase();
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
    use crate::palette::{PaletteCandidate, PaletteRow, PaletteTextPart, PaletteTextTone};

    use super::{CandidateMatcher, ContainsMatcher};

    fn candidate(label: &str) -> PaletteCandidate {
        PaletteRow::new(label)
            .label_matchable_text(label)
            .into_candidate()
    }

    #[test]
    fn contains_matcher_prioritizes_prefix_hits() {
        let matcher = ContainsMatcher;
        let all = vec![candidate("zoom-in"), candidate("inbox"), candidate("pan")];

        let selected = matcher.select("in", &all);
        assert_eq!(selected, vec![1, 0]);
    }

    #[test]
    fn contains_matcher_uses_structured_match_texts() {
        let matcher = ContainsMatcher;
        let all = vec![
            PaletteRow::new("alpha")
                .label_matchable_parts(vec![PaletteTextPart {
                    text: "structured-match".to_string(),
                    tone: PaletteTextTone::Primary,
                }])
                .into_candidate(),
            candidate("beta"),
        ];

        let selected = matcher.select("structured", &all);
        assert_eq!(selected, vec![0]);
    }
}
