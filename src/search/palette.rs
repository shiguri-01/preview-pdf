use crate::command::{Command, SearchMatcherKind};
use crate::error::AppResult;
use crate::input::InputHistoryRecord;
use crate::input::shortcut::{
    ShortcutKey, format_shortcut_alternatives_tight, format_shortcut_key,
};
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PaletteOpenPayload,
    PalettePayload, PalettePostAction, PaletteProvider, PaletteSearchText, PaletteSubmitEffect,
    PaletteTextPart,
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

    fn reset_selection_on_input_change(&self) -> bool {
        false
    }

    fn initial_selected_candidate(
        &self,
        ctx: &PaletteContext<'_>,
        candidates: &[PaletteCandidate],
    ) -> Option<usize> {
        let PaletteOpenPayload::Search { matcher, .. } = ctx.open_payload? else {
            return None;
        };

        candidates
            .iter()
            .position(|candidate| match &candidate.payload {
                PalettePayload::Opaque(id) => SearchMatcherKind::parse(id) == Some(*matcher),
                PalettePayload::None => false,
            })
    }

    fn list(&self, _ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        Ok(vec![
            PaletteCandidate {
                id: SearchMatcherKind::ContainsInsensitive.id().to_string(),
                left: vec![PaletteTextPart::primary("Contains (case insensitive)")],
                right: Vec::new(),
                search_texts: vec![
                    PaletteSearchText::new("contains insensitive"),
                    PaletteSearchText::new("contains case insensitive"),
                    PaletteSearchText::new(SearchMatcherKind::ContainsInsensitive.id()),
                ],
                payload: PalettePayload::Opaque(
                    SearchMatcherKind::ContainsInsensitive.id().to_string(),
                ),
            },
            PaletteCandidate {
                id: SearchMatcherKind::ContainsSensitive.id().to_string(),
                left: vec![PaletteTextPart::primary("Contains (case sensitive)")],
                right: Vec::new(),
                search_texts: vec![
                    PaletteSearchText::new("contains sensitive"),
                    PaletteSearchText::new("contains case sensitive"),
                    PaletteSearchText::new(SearchMatcherKind::ContainsSensitive.id()),
                ],
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
                payload: None,
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
            history_record: Some(InputHistoryRecord::SearchQuery(query.to_string())),
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        let enter = format_shortcut_key(ShortcutKey::key(crossterm::event::KeyCode::Enter));
        let history = format_shortcut_alternatives_tight(&[
            ShortcutKey::key(crossterm::event::KeyCode::Up),
            ShortcutKey::key(crossterm::event::KeyCode::Down),
        ]);
        let matcher =
            format_shortcut_alternatives_tight(&[ShortcutKey::ctrl('p'), ShortcutKey::ctrl('n')]);
        Some(format!(
            "{enter} search   {history} history   {matcher} matcher"
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        command::SearchMatcherKind,
        extension::ExtensionUiSnapshot,
        palette::{PaletteContext, PaletteKind, PaletteOpenPayload, PaletteProvider},
    };

    use super::SearchPaletteProvider;

    #[test]
    fn search_payload_prefills_query_input() {
        let provider = SearchPaletteProvider;
        let open_payload = PaletteOpenPayload::Search {
            query: "needle".to_string(),
            matcher: SearchMatcherKind::ContainsSensitive,
        };

        assert_eq!(provider.initial_input(Some(&open_payload)), "needle");
    }

    #[test]
    fn search_payload_selects_current_matcher() {
        let provider = SearchPaletteProvider;
        let app = crate::app::AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let open_payload = PaletteOpenPayload::Search {
            query: "needle".to_string(),
            matcher: SearchMatcherKind::ContainsSensitive,
        };
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Search,
            input: "needle",
            open_payload: Some(&open_payload),
        };
        let candidates = provider.list(&ctx).expect("search list should build");

        assert_eq!(
            provider.initial_selected_candidate(&ctx, &candidates),
            Some(1)
        );
    }

    #[test]
    fn non_search_payload_keeps_default_selection() {
        let provider = SearchPaletteProvider;
        let app = crate::app::AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let open_payload = PaletteOpenPayload::CommandInput("needle".to_string());
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Search,
            input: "needle",
            open_payload: Some(&open_payload),
        };
        let candidates = provider.list(&ctx).expect("search list should build");

        assert_eq!(provider.initial_selected_candidate(&ctx, &candidates), None);
    }
}
