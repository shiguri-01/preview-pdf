use crate::app::PageLayoutMode;
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

use super::state::SearchPaletteEntry;

pub struct SearchPaletteProvider;
pub struct SearchResultsPaletteProvider;

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

impl PaletteProvider for SearchResultsPaletteProvider {
    fn kind(&self) -> PaletteKind {
        PaletteKind::SearchResults
    }

    fn title(&self, _ctx: &PaletteContext<'_>) -> String {
        "Search Results".to_string()
    }

    fn input_mode(&self) -> PaletteInputMode {
        PaletteInputMode::Custom
    }

    fn reset_selection_on_input_change(&self) -> bool {
        true
    }

    fn initial_selected_candidate(
        &self,
        ctx: &PaletteContext<'_>,
        candidates: &[PaletteCandidate],
    ) -> Option<usize> {
        let primary_page = ctx.app.current_page + 1;
        let trailing_page = if ctx.app.page_layout_mode == PageLayoutMode::Spread {
            Some(primary_page + 1)
        } else {
            None
        };
        candidates
            .iter()
            .position(|candidate| match &candidate.payload {
                PalettePayload::Opaque(page) => page
                    .parse::<usize>()
                    .ok()
                    .is_some_and(|page| page == primary_page || Some(page) == trailing_page),
                PalettePayload::None => false,
            })
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let entries = ctx.extensions.search_results_entries.as_ref();
        let query = ctx.input.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Ok(build_result_candidates(entries));
        }

        let mut index_matches = Vec::new();
        let mut snippet_matches = Vec::new();
        let mut page_matches = Vec::new();
        for entry in entries.iter() {
            let candidate = result_candidate(entry);
            match result_match_bucket(entry, &query) {
                Some(SearchResultMatchBucket::Index) => index_matches.push(candidate),
                Some(SearchResultMatchBucket::Snippet) => snippet_matches.push(candidate),
                Some(SearchResultMatchBucket::Page) => page_matches.push(candidate),
                None => {}
            }
        }

        Ok(index_matches
            .into_iter()
            .chain(snippet_matches)
            .chain(page_matches)
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
        let page = match &candidate.payload {
            PalettePayload::Opaque(value) => value.parse::<usize>().ok(),
            PalettePayload::None => None,
        };
        let Some(page) = page else {
            return Ok(PaletteSubmitEffect::Close);
        };

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::SearchResultGoto { page },
            history_record: None,
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        if ctx.extensions.search_results_entries.is_empty() {
            return Some("0 hits".to_string());
        }

        let enter = format_shortcut_key(ShortcutKey::key(crossterm::event::KeyCode::Enter));
        Some(format!("{enter} jump to page"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchResultMatchBucket {
    Index,
    Snippet,
    Page,
}

fn result_match_bucket(entry: &SearchPaletteEntry, query: &str) -> Option<SearchResultMatchBucket> {
    if entry.index.to_string().contains(query) {
        return Some(SearchResultMatchBucket::Index);
    }
    if entry.snippet.to_ascii_lowercase().contains(query) {
        return Some(SearchResultMatchBucket::Snippet);
    }
    let page_text = page_label(entry.page);
    if page_text.contains(query) || (entry.page + 1).to_string().contains(query) {
        return Some(SearchResultMatchBucket::Page);
    }
    None
}

fn page_label(page: usize) -> String {
    format!("p.{}", page + 1)
}

fn build_result_candidates(entries: &[SearchPaletteEntry]) -> Vec<PaletteCandidate> {
    entries.iter().map(result_candidate).collect()
}

fn snippet_parts(entry: &SearchPaletteEntry) -> Vec<PaletteTextPart> {
    let snippet = entry.snippet.as_str();
    let (Some(start), Some(end)) = (entry.snippet_match_start, entry.snippet_match_end) else {
        return vec![PaletteTextPart::primary(snippet)];
    };
    if start >= end || end > snippet.len() {
        return vec![PaletteTextPart::primary(snippet)];
    }
    if !snippet.is_char_boundary(start) || !snippet.is_char_boundary(end) {
        return vec![PaletteTextPart::primary(snippet)];
    }

    let before = &snippet[..start];
    let matched = &snippet[start..end];
    let after = &snippet[end..];

    let mut parts = Vec::new();
    if !before.is_empty() {
        parts.push(PaletteTextPart::secondary(before));
    }
    if !matched.is_empty() {
        parts.push(PaletteTextPart::highlight(matched));
    }
    if !after.is_empty() {
        parts.push(PaletteTextPart::secondary(after));
    }
    if parts.is_empty() {
        vec![PaletteTextPart::primary(snippet)]
    } else {
        parts
    }
}

fn result_candidate(entry: &SearchPaletteEntry) -> PaletteCandidate {
    let page = page_label(entry.page);
    PaletteCandidate {
        id: format!("result-{}", entry.index),
        left: {
            let mut parts = vec![
                PaletteTextPart::primary(entry.index.to_string()),
                PaletteTextPart::primary("  "),
            ];
            parts.extend(snippet_parts(entry));
            parts
        },
        right: vec![PaletteTextPart::secondary(page.clone())],
        search_texts: vec![
            PaletteSearchText::new(entry.index.to_string()),
            PaletteSearchText::new(entry.snippet.clone()),
            PaletteSearchText::new(page.clone()),
            PaletteSearchText::new(format!("page {}", entry.page + 1)),
            PaletteSearchText::new((entry.page + 1).to_string()),
        ],
        payload: PalettePayload::Opaque((entry.page + 1).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app::PageLayoutMode,
        command::SearchMatcherKind,
        extension::ExtensionUiSnapshot,
        palette::{PaletteContext, PaletteKind, PaletteOpenPayload, PaletteProvider},
        search::SearchPaletteEntry,
    };

    use super::{SearchPaletteProvider, SearchResultsPaletteProvider};

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

    #[test]
    fn results_list_shows_index_snippet_and_page() {
        let provider = SearchResultsPaletteProvider;
        let app = crate::app::AppState::default();
        let extensions = ExtensionUiSnapshot {
            search_results_entries: vec![SearchPaletteEntry {
                index: 1,
                page: 4,
                snippet: "…foo needle bar…".to_string(),
                snippet_match_start: Some(7),
                snippet_match_end: Some(13),
            }]
            .into(),
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
            open_payload: None,
        };

        let list = provider.list(&ctx).expect("results list should build");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].left[0].text, "1");
        assert_eq!(list[0].left[2].text, "…foo ");
        assert_eq!(list[0].left[3].text, "needle");
        assert_eq!(list[0].left[4].text, " bar…");
        assert_eq!(list[0].right[0].text, "p.5");
    }

    #[test]
    fn results_initial_selection_accepts_right_page_of_visible_spread() {
        let provider = SearchResultsPaletteProvider;
        let app = crate::app::AppState {
            current_page: 4,
            page_layout_mode: PageLayoutMode::Spread,
            ..crate::app::AppState::default()
        };
        let extensions = ExtensionUiSnapshot {
            search_results_entries: vec![
                SearchPaletteEntry {
                    index: 1,
                    page: 8,
                    snippet: "other".to_string(),
                    snippet_match_start: None,
                    snippet_match_end: None,
                },
                SearchPaletteEntry {
                    index: 2,
                    page: 5,
                    snippet: "right".to_string(),
                    snippet_match_start: None,
                    snippet_match_end: None,
                },
            ]
            .into(),
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
            open_payload: None,
        };
        let candidates = provider.list(&ctx).expect("results list should build");

        assert_eq!(
            provider.initial_selected_candidate(&ctx, &candidates),
            Some(1)
        );
    }

    #[test]
    fn results_assistive_text_shows_zero_hits() {
        let provider = SearchResultsPaletteProvider;
        let app = crate::app::AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
            open_payload: None,
        };

        assert_eq!(
            provider.assistive_text(&ctx, None),
            Some("0 hits".to_string())
        );
    }
}
