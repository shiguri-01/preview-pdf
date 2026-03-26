use crate::command::Command;
use crate::error::AppResult;
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PalettePayload,
    PalettePostAction, PaletteProvider, PaletteSubmitEffect,
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
        PaletteInputMode::FilterCandidates
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        Ok(ctx
            .extensions
            .outline_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| PaletteCandidate {
                id: format!("outline-{index}"),
                label: format!("{}{}", "  ".repeat(entry.depth), entry.title),
                detail: Some(format_outline_page_detail(entry.page)),
                payload: PalettePayload::Opaque(encode_payload(entry.page, &entry.title)),
            })
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

fn format_outline_page_detail(page: usize) -> String {
    format!("p.{}", page + 1)
}

fn decode_payload(payload: &PalettePayload) -> Option<(usize, String)> {
    let PalettePayload::Opaque(payload) = payload else {
        return None;
    };
    let mut parts = payload.splitn(2, PAYLOAD_SEP);
    let page = parts.next()?;
    let title = parts.next()?;
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
        assert_eq!(items[0].detail.as_deref(), Some("p.12"));
    }
}
