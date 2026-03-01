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
            let label = candidate.label.to_ascii_lowercase();
            if label.starts_with(&query) {
                prefix.push(idx);
            } else if label.contains(&query) {
                contains.push(idx);
            }
        }

        prefix.extend(contains);
        prefix
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::{PaletteCandidate, PalettePayload};

    use super::{CandidateMatcher, ContainsMatcher};

    fn candidate(label: &str) -> PaletteCandidate {
        PaletteCandidate {
            id: label.to_string(),
            label: label.to_string(),
            detail: None,
            payload: PalettePayload::None,
        }
    }

    #[test]
    fn contains_matcher_prioritizes_prefix_hits() {
        let matcher = ContainsMatcher;
        let all = vec![
            candidate("zoom-in"),
            candidate("inbox"),
            candidate("scroll"),
        ];

        let selected = matcher.select("in", &all);
        assert_eq!(selected, vec![1, 0]);
    }
}
