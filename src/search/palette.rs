use crate::app::{PageLayoutMode, SpreadCoverPolicy};
use crate::command::{Command, SearchMatcherKind};
use crate::error::AppResult;
use crate::input::InputHistoryRecord;
use crate::input::shortcut::{
    ShortcutKey, format_shortcut_alternatives_tight, format_shortcut_key,
};
use crate::palette::{
    PageIndex, PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PaletteOpenOptions,
    PalettePostAction, PaletteProvider, PaletteRow, PaletteSubmitEffect, PaletteTextPart,
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

    fn list(&self, _ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        Ok(vec![
            PaletteRow::new(SearchMatcherKind::ContainsInsensitive.id())
                .label_matchable_text("Contains (case insensitive)")
                .into_candidate(),
            PaletteRow::new(SearchMatcherKind::ContainsSensitive.id())
                .label_matchable_text("Contains (case sensitive)")
                .into_candidate(),
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
                options: PaletteOpenOptions::default(),
            });
        }

        let matcher = selected
            .and_then(|c| SearchMatcherKind::parse(c.id().as_str()))
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
        let cover_solo = ctx.app.page_layout_mode == PageLayoutMode::Spread
            && ctx.app.spread_cover_policy == SpreadCoverPolicy::Cover
            && ctx.app.current_page == 0;
        let trailing_page = if ctx.app.page_layout_mode == PageLayoutMode::Spread && !cover_solo {
            Some(primary_page + 1)
        } else {
            None
        };
        candidates.iter().position(|candidate| {
            search_result_entry_for_candidate(ctx, candidate).is_some_and(|entry| {
                let page = entry.page + 1;
                page == primary_page || Some(page) == trailing_page
            })
        })
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let entries = ctx.extensions.search.results_entries.as_ref();
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
        let Some(entry) = search_result_entry_for_candidate(_ctx, candidate) else {
            return Ok(PaletteSubmitEffect::Close);
        };

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::SearchResultGoto {
                page: entry.page + 1,
            },
            history_record: None,
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        if ctx.extensions.search.results_entries.is_empty() {
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
    let mut parts = vec![
        PaletteTextPart::primary(entry.index.to_string()),
        PaletteTextPart::primary("  "),
    ];
    parts.extend(snippet_parts(entry));
    PaletteRow::new(format!("result-{}", entry.index))
        .label_matchable_parts(parts)
        .detail_page(PageIndex::zero_based(entry.page))
        .into_candidate()
}

fn search_result_entry_for_candidate<'a>(
    ctx: &'a PaletteContext<'_>,
    candidate: &PaletteCandidate,
) -> Option<&'a SearchPaletteEntry> {
    let index = candidate
        .id()
        .as_str()
        .strip_prefix("result-")?
        .parse::<usize>()
        .ok()?;
    ctx.extensions
        .search
        .results_entries
        .iter()
        .find(|entry| entry.index == index)
}

#[cfg(test)]
mod tests {
    use crate::{
        app::AppState,
        app::{PageLayoutMode, SpreadCoverPolicy},
        extension::ExtensionUiSnapshot,
        input::InputHistorySnapshot,
        palette::{
            PaletteAppSnapshot, PaletteContext, PaletteKind, PaletteOpenOptions, PaletteProvider,
            PaletteRegistry, PaletteSessionController,
        },
        search::state::SearchPaletteEntry,
    };

    use super::SearchResultsPaletteProvider;

    fn history_snapshot(entries: &[&str]) -> InputHistorySnapshot {
        InputHistorySnapshot::from_entries(entries)
    }

    #[test]
    fn search_session_keeps_selected_matcher_when_typing_query() {
        let registry = PaletteRegistry::default();
        let mut session = PaletteSessionController::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        session
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Search,
                PaletteOpenOptions::default(),
                Some(history_snapshot(&["needle"])),
            )
            .expect("search palette should open");

        assert!(session.select_next_item());
        let selected_view = session.view().expect("palette should be visible");
        assert_eq!(selected_view.selected_idx, 1);

        session
            .insert_text(&registry, &app, &extensions, "a")
            .expect("typing should succeed");
        let updated_view = session.view().expect("palette should be visible");
        assert_eq!(updated_view.selected_idx, 1);
        assert_eq!(updated_view.input, "a");
    }

    #[test]
    fn results_list_shows_index_snippet_and_page() {
        let provider = SearchResultsPaletteProvider;
        let app = PaletteAppSnapshot::default();
        let extensions = ExtensionUiSnapshot {
            search: crate::search::SearchUiSnapshot {
                results_entries: vec![SearchPaletteEntry {
                    index: 1,
                    page: 4,
                    snippet: "…foo needle bar…".to_string(),
                    snippet_match_start: Some(7),
                    snippet_match_end: Some(13),
                }]
                .into(),
                ..Default::default()
            },
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
        };

        let list = provider.list(&ctx).expect("results list should build");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label()[0].text, "1");
        assert_eq!(list[0].label()[2].text, "…foo ");
        assert_eq!(list[0].label()[3].text, "needle");
        assert_eq!(list[0].label()[4].text, " bar…");
        assert_eq!(list[0].detail()[0].text, "p.5");
    }

    #[test]
    fn results_initial_selection_accepts_right_page_of_visible_spread() {
        let provider = SearchResultsPaletteProvider;
        let app = PaletteAppSnapshot {
            current_page: 4,
            page_layout_mode: PageLayoutMode::Spread,
            spread_cover_policy: SpreadCoverPolicy::Paired,
            ..PaletteAppSnapshot::default()
        };
        let extensions = ExtensionUiSnapshot {
            search: crate::search::SearchUiSnapshot {
                results_entries: vec![
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
                ..Default::default()
            },
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
        };
        let candidates = provider.list(&ctx).expect("results list should build");

        assert_eq!(
            provider.initial_selected_candidate(&ctx, &candidates),
            Some(1)
        );
    }

    #[test]
    fn results_initial_selection_does_not_treat_cover_solo_as_spread() {
        let provider = SearchResultsPaletteProvider;
        let app = PaletteAppSnapshot {
            current_page: 0,
            page_layout_mode: PageLayoutMode::Spread,
            spread_cover_policy: SpreadCoverPolicy::Cover,
            ..PaletteAppSnapshot::default()
        };
        let extensions = ExtensionUiSnapshot {
            search: crate::search::SearchUiSnapshot {
                results_entries: vec![
                    SearchPaletteEntry {
                        index: 1,
                        page: 0,
                        snippet: "cover".to_string(),
                        snippet_match_start: None,
                        snippet_match_end: None,
                    },
                    SearchPaletteEntry {
                        index: 2,
                        page: 1,
                        snippet: "next".to_string(),
                        snippet_match_start: None,
                        snippet_match_end: None,
                    },
                ]
                .into(),
                ..Default::default()
            },
            ..ExtensionUiSnapshot::default()
        };
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
        };
        let candidates = provider.list(&ctx).expect("results list should build");

        assert_eq!(
            provider.initial_selected_candidate(&ctx, &candidates),
            Some(0)
        );
    }

    #[test]
    fn results_assistive_text_shows_zero_hits() {
        let provider = SearchResultsPaletteProvider;
        let app = PaletteAppSnapshot::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::SearchResults,
            input: "",
        };

        assert_eq!(
            provider.assistive_text(&ctx, None),
            Some("0 hits".to_string())
        );
    }
}
