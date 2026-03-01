use crate::command::{Command, SearchMatcherKind};
use crate::error::AppResult;
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PalettePayload,
    PalettePostAction, PaletteProvider, PaletteSubmitEffect,
};

pub struct SearchPaletteProvider;

impl PaletteProvider for SearchPaletteProvider {
    fn kind(&self) -> PaletteKind {
        PaletteKind::Search
    }

    fn title(&self, _ctx: &PaletteContext<'_>) -> String {
        "Search".to_string()
    }

    fn input_mode(&self) -> PaletteInputMode {
        PaletteInputMode::FreeText
    }

    fn list(&self, _ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        Ok(vec![
            PaletteCandidate {
                id: SearchMatcherKind::ContainsInsensitive.id().to_string(),
                label: "Contains (case insensitive)".to_string(),
                detail: None,
                payload: PalettePayload::Opaque(
                    SearchMatcherKind::ContainsInsensitive.id().to_string(),
                ),
            },
            PaletteCandidate {
                id: SearchMatcherKind::ContainsSensitive.id().to_string(),
                label: "Contains (case sensitive)".to_string(),
                detail: None,
                payload: PalettePayload::Opaque(
                    SearchMatcherKind::ContainsSensitive.id().to_string(),
                ),
            },
        ])
    }

    fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        let query = ctx.input.trim();
        if query.is_empty() {
            return Ok(PaletteSubmitEffect::Reopen {
                kind: self.kind(),
                seed: None,
            });
        }

        let matcher = selected
            .and_then(|c| match &c.payload {
                PalettePayload::Opaque(id) => SearchMatcherKind::parse(id),
                PalettePayload::None => None,
            })
            .unwrap_or(SearchMatcherKind::ContainsInsensitive);

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::SubmitSearch {
                query: query.to_string(),
                matcher,
            },
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        Some("Enter: search  [up/down]: select matcher".to_string())
    }
}
