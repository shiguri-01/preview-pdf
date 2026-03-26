use crate::command::Command;
use crate::error::AppResult;
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PalettePayload,
    PalettePostAction, PaletteProvider, PaletteSearchText, PaletteSubmitEffect, PaletteTextPart,
};

const PAYLOAD_SEP: char = '\u{1f}';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlinePaletteEntry {
    pub title: String,
    pub page: usize,
    pub depth: usize,
}

pub struct OutlinePaletteProvider;

impl PaletteProvider for OutlinePaletteProvider {
    fn kind(&self) -> PaletteKind {
        PaletteKind::Outline
    }

    fn title(&self, _ctx: &PaletteContext<'_>) -> String {
        "Outline".to_string()
    }

    fn input_mode(&self) -> PaletteInputMode {
        PaletteInputMode::Custom
    }

    fn reset_selection_on_input_change(&self) -> bool {
        true
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let query = ctx.input.trim().to_lowercase();

        let mut text_matches = Vec::new();
        let mut page_text_matches = Vec::new();
        let mut unfiltered = Vec::new();

        for (index, entry) in ctx.extensions.outline_entries.iter().enumerate() {
            let candidate = outline_candidate(index, entry);
            if query.is_empty() {
                unfiltered.push(candidate);
                continue;
            }

            // Keep matching textual: we do not interpret numbers as page lookups here.
            // That makes inputs like `p.1` behave like normal text and match `p.10` / `p.123`.
            if text_contains(&entry.title, &query) {
                text_matches.push((entry.page, index, candidate));
            } else if text_contains(&format_outline_page_detail(entry.page), &query)
                || text_contains(&format!("page {}", entry.page + 1), &query)
                || text_contains(&(entry.page + 1).to_string(), &query)
            {
                page_text_matches.push((entry.page, index, candidate));
            }
        }

        if query.is_empty() {
            return Ok(unfiltered);
        }

        // Keep the result list readable: show text matches first, then page-label matches,
        // and sort each bucket by page so the outline still feels like a document map.
        text_matches.sort_by_key(|(page, index, _)| (*page, *index));
        page_text_matches.sort_by_key(|(page, index, _)| (*page, *index));

        Ok(text_matches
            .into_iter()
            .chain(page_text_matches)
            .map(|(_, _, candidate)| candidate)
            .collect())
    }

    fn on_submit(
        &self,
        _ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        let Some(candidate) = selected else {
            return Ok(PaletteSubmitEffect::Close);
        };

        let Some((page, title)) = decode_payload(&candidate.payload) else {
            return Ok(PaletteSubmitEffect::Close);
        };

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::OutlineGoto { page, title },
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        if ctx.extensions.outline_entries.is_empty() {
            Some("No outline entries in this document".to_string())
        } else {
            Some("Enter: jump to page".to_string())
        }
    }
}

fn encode_payload(page: usize, title: &str) -> String {
    format!("{page}{PAYLOAD_SEP}{title}")
}

fn outline_candidate(index: usize, entry: &OutlinePaletteEntry) -> PaletteCandidate {
    PaletteCandidate {
        id: format!("outline-{index}"),
        left: vec![PaletteTextPart::primary(format!(
            "{}{}",
            "  ".repeat(entry.depth),
            entry.title
        ))],
        right: vec![PaletteTextPart::secondary(format_outline_page_detail(
            entry.page,
        ))],
        search_texts: vec![
            PaletteSearchText::new(entry.title.clone()),
            PaletteSearchText::new(format!("page {}", entry.page + 1)),
            PaletteSearchText::new(format_outline_page_detail(entry.page)),
            PaletteSearchText::new((entry.page + 1).to_string()),
        ],
        payload: PalettePayload::Opaque(encode_payload(entry.page, &entry.title)),
    }
}

fn format_outline_page_detail(page: usize) -> String {
    format!("p.{}", page + 1)
}

fn text_contains(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(query)
}

fn decode_payload(payload: &PalettePayload) -> Option<(usize, String)> {
    let PalettePayload::Opaque(payload) = payload else {
        return None;
    };
    let (page, title) = payload.split_once(PAYLOAD_SEP)?;
    Some((page.parse().ok()?, title.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::{
        extension::ExtensionUiSnapshot,
        palette::{PaletteContext, PaletteKind, PalettePayload, PaletteProvider},
    };

    use super::{
        OutlinePaletteEntry, OutlinePaletteProvider, decode_payload, encode_payload,
        format_outline_page_detail,
    };

    #[test]
    fn page_detail_uses_loading_overlay_format() {
        assert_eq!(format_outline_page_detail(11), "p.12");
    }

    #[test]
    fn payload_round_trip_preserves_page_and_title() {
        let encoded = encode_payload(11, "Section 2");
        let decoded =
            decode_payload(&PalettePayload::Opaque(encoded)).expect("payload should decode");
        assert_eq!(decoded, (11, "Section 2".to_string()));
    }

    #[test]
    fn payload_round_trip_preserves_separator_in_title() {
        let encoded = encode_payload(11, "Section\u{1f}2");
        let decoded =
            decode_payload(&PalettePayload::Opaque(encoded)).expect("payload should decode");
        assert_eq!(decoded, (11, "Section\u{1f}2".to_string()));
    }

    #[test]
    fn list_uses_p_prefixed_page_detail() {
        let provider = OutlinePaletteProvider;
        let entries = vec![OutlinePaletteEntry {
            title: "Intro".to_string(),
            page: 11,
            depth: 0,
        }];
        let extensions = ExtensionUiSnapshot {
            outline_entries: entries.into(),
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app: &Default::default(),
            extensions: &extensions,
            kind: PaletteKind::Outline,
            input: "",
            seed: None,
        };

        let items = provider.list(&ctx).expect("outline list should build");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].right.len(), 1);
        assert_eq!(items[0].right[0].text, "p.12");
    }

    #[test]
    fn list_uses_text_matches_before_page_label_matches() {
        let provider = OutlinePaletteProvider;
        let entries = vec![
            OutlinePaletteEntry {
                title: "Chapter 3 overview".to_string(),
                page: 23,
                depth: 0,
            },
            OutlinePaletteEntry {
                title: "Contents".to_string(),
                page: 2,
                depth: 0,
            },
            OutlinePaletteEntry {
                title: "Other section".to_string(),
                page: 3,
                depth: 0,
            },
        ];
        let extensions = ExtensionUiSnapshot {
            outline_entries: entries.into(),
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app: &Default::default(),
            extensions: &extensions,
            kind: PaletteKind::Outline,
            input: "3",
            seed: None,
        };

        let items = provider.list(&ctx).expect("outline list should build");
        let titles = items
            .iter()
            .map(|item| item.left[0].text.trim().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            titles,
            vec!["Chapter 3 overview".to_string(), "Contents".to_string(),]
        );
    }

    #[test]
    fn list_treats_p_prefixed_queries_as_text() {
        let provider = OutlinePaletteProvider;
        let entries = vec![
            OutlinePaletteEntry {
                title: "Section A".to_string(),
                page: 9,
                depth: 0,
            },
            OutlinePaletteEntry {
                title: "Section B".to_string(),
                page: 122,
                depth: 0,
            },
        ];
        let extensions = ExtensionUiSnapshot {
            outline_entries: entries.into(),
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app: &Default::default(),
            extensions: &extensions,
            kind: PaletteKind::Outline,
            input: "p.1",
            seed: None,
        };

        let items = provider.list(&ctx).expect("outline list should build");
        let titles = items
            .iter()
            .map(|item| item.left[0].text.trim().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            titles,
            vec!["Section A".to_string(), "Section B".to_string(),]
        );
    }

    #[test]
    fn list_matches_unicode_titles_case_insensitively() {
        let provider = OutlinePaletteProvider;
        let entries = vec![OutlinePaletteEntry {
            title: "Überblick".to_string(),
            page: 7,
            depth: 0,
        }];
        let extensions = ExtensionUiSnapshot {
            outline_entries: entries.into(),
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app: &Default::default(),
            extensions: &extensions,
            kind: PaletteKind::Outline,
            input: "ÜBER",
            seed: None,
        };

        let items = provider.list(&ctx).expect("outline list should build");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].left[0].text.trim(), "Überblick");
    }
}
