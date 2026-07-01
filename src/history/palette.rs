use std::sync::Arc;

use crate::command::Command;
use crate::error::AppResult;
use crate::input::shortcut::format_shortcut_key;
use crate::palette::{
    PageIndex, PaletteCandidate, PaletteCandidateId, PaletteContext, PaletteInputMode, PaletteKind,
    PalettePostAction, PaletteProvider, PaletteRow, PaletteSubmitEffect,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryUiSnapshot {
    pub entries: Arc<[HistoryPaletteEntry]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPaletteEntry {
    pub id: PaletteCandidateId,
    pub display_index: isize,
    pub page: PageIndex,
    pub reason: HistoryPaletteReason,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryPaletteReason {
    Search(String),
    Outline(String),
    Goto(String),
    PageOnly,
}

pub struct HistoryPaletteProvider;

impl PaletteProvider for HistoryPaletteProvider {
    fn kind(&self) -> PaletteKind {
        PaletteKind::History
    }

    fn title(&self, _ctx: &PaletteContext<'_>) -> String {
        "Navigation History".to_string()
    }

    fn input_mode(&self) -> PaletteInputMode {
        PaletteInputMode::Custom
    }

    fn reset_selection_on_input_change(&self) -> bool {
        true
    }

    fn initial_selected_candidate(
        &self,
        _ctx: &PaletteContext<'_>,
        candidates: &[PaletteCandidate],
    ) -> Option<usize> {
        // Open the palette anchored on the current page so the user can keep
        // navigating from their present location; Enter still uses the normal
        // history-jump path for every row.
        candidates.iter().position(|candidate| {
            history_entry_for_candidate(_ctx, candidate).is_some_and(|entry| entry.is_current)
        })
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let query = ctx.input.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Ok(build_history_candidates(
                ctx.extensions.history.entries.as_ref(),
            ));
        }

        let mut index_matches = Vec::new();
        let mut reason_matches = Vec::new();
        let mut page_matches = Vec::new();

        for entry in ctx.extensions.history.entries.iter() {
            let idx = entry.display_index;
            let view = HistoryEntryView::new(entry);
            let bucket = view.match_bucket(idx, entry.page.display_number(), &query);
            let candidate = view.into_candidate(entry);
            match bucket {
                Some(HistoryMatchBucket::Index) => index_matches.push(candidate),
                Some(HistoryMatchBucket::Reason) => reason_matches.push(candidate),
                Some(HistoryMatchBucket::Page) => page_matches.push(candidate),
                None => {}
            }
        }

        Ok(index_matches
            .into_iter()
            .chain(reason_matches)
            .chain(page_matches)
            .collect())
    }

    fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        let Some(candidate) = selected else {
            return Ok(PaletteSubmitEffect::Close);
        };

        let Some(entry) = history_entry_for_candidate(ctx, candidate) else {
            return Ok(PaletteSubmitEffect::Close);
        };

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::HistoryGoto {
                page: entry.page.display_number(),
            },
            history_record: None,
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        let enter = format_shortcut_key(crate::input::shortcut::ShortcutKey::key(
            crossterm::event::KeyCode::Enter,
        ));
        Some(format!("{enter} jump to page"))
    }
}

fn build_history_candidates(entries: &[HistoryPaletteEntry]) -> Vec<PaletteCandidate> {
    entries
        .iter()
        .map(|entry| HistoryEntryView::new(entry).into_candidate(entry))
        .collect()
}

#[derive(Debug, Clone)]
struct HistoryEntryView {
    display_index: isize,
    page: PageIndex,
    reason: HistoryPaletteReason,
}

impl HistoryEntryView {
    fn new(entry: &HistoryPaletteEntry) -> Self {
        Self {
            display_index: entry.display_index,
            page: entry.page.clone(),
            reason: entry.reason.clone(),
        }
    }

    fn into_candidate(self, entry: &HistoryPaletteEntry) -> PaletteCandidate {
        PaletteRow::new(entry.id.clone())
            .label_matchable_text(self.display_index.to_string())
            .label_decoration("  ")
            .label_matchable_text(self.reason.display_text(self.page.display_number()))
            .detail_page(entry.page.clone())
            .into_candidate()
    }

    fn match_bucket(
        &self,
        idx: isize,
        page_1indexed: usize,
        query: &str,
    ) -> Option<HistoryMatchBucket> {
        self.reason.match_bucket(idx, page_1indexed, query)
    }
}

fn page_text(page_1indexed: usize) -> String {
    format!("p.{page_1indexed}")
}

fn history_entry_for_candidate<'a>(
    ctx: &'a PaletteContext<'_>,
    candidate: &PaletteCandidate,
) -> Option<&'a HistoryPaletteEntry> {
    ctx.extensions
        .history
        .entries
        .iter()
        .find(|entry| &entry.id == candidate.id())
}

impl HistoryPaletteReason {
    fn display_text(&self, page_1indexed: usize) -> String {
        match self {
            Self::Search(query) => format!("/{query}"),
            Self::Outline(title) => format!("#{title}"),
            Self::Goto(label) => label.clone(),
            Self::PageOnly => page_text(page_1indexed),
        }
    }

    fn match_bucket(
        &self,
        idx: isize,
        page_1indexed: usize,
        query: &str,
    ) -> Option<HistoryMatchBucket> {
        if idx.to_string().contains(query) {
            return Some(HistoryMatchBucket::Index);
        }

        if self
            .display_text(page_1indexed)
            .to_ascii_lowercase()
            .contains(query)
        {
            return Some(HistoryMatchBucket::Reason);
        }

        if page_text(page_1indexed)
            .to_ascii_lowercase()
            .contains(query)
        {
            return Some(HistoryMatchBucket::Page);
        }

        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryMatchBucket {
    Index,
    Reason,
    Page,
}

#[cfg(test)]
fn history_match_bucket(
    idx: isize,
    page_1indexed: usize,
    reason: HistoryPaletteReason,
    query: &str,
) -> Option<HistoryMatchBucket> {
    reason.match_bucket(idx, page_1indexed, query)
}

#[cfg(test)]
fn history_reason_display_text(page_1indexed: usize, reason: HistoryPaletteReason) -> String {
    reason.display_text(page_1indexed)
}

#[cfg(test)]
mod tests {
    use crate::{
        extension::ExtensionUiSnapshot,
        palette::{
            PageIndex, PaletteAppSnapshot, PaletteCandidateId, PaletteContext, PaletteProvider,
        },
    };

    use super::{
        HistoryMatchBucket, HistoryPaletteEntry, HistoryPaletteProvider, HistoryPaletteReason,
        HistoryUiSnapshot, history_match_bucket, history_reason_display_text,
    };

    fn entry(
        id: &str,
        display_index: isize,
        page: usize,
        reason: HistoryPaletteReason,
        is_current: bool,
    ) -> HistoryPaletteEntry {
        HistoryPaletteEntry {
            id: PaletteCandidateId::new(id),
            display_index,
            page: PageIndex::zero_based(page),
            reason,
            is_current,
        }
    }

    fn context<'a>(extensions: &'a ExtensionUiSnapshot, input: &'a str) -> PaletteContext<'a> {
        PaletteContext {
            app: PaletteAppSnapshot::default(),
            extensions,
            kind: crate::palette::PaletteKind::History,
            input,
        }
    }

    #[test]
    fn list_uses_snapshot_order_and_p_prefixed_page_labels() {
        let provider = HistoryPaletteProvider;
        let extensions = ExtensionUiSnapshot {
            history: HistoryUiSnapshot {
                entries: vec![
                    entry("future", 1, 9, HistoryPaletteReason::PageOnly, false),
                    entry(
                        "current",
                        0,
                        0,
                        HistoryPaletteReason::Search("1".to_string()),
                        true,
                    ),
                    entry("back", -1, 11, HistoryPaletteReason::PageOnly, false),
                ]
                .into(),
            },
            ..ExtensionUiSnapshot::default()
        };
        let ctx = context(&extensions, "");

        let items = provider.list(&ctx).expect("history list should build");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label()[0].text, "1");
        assert_eq!(items[1].label()[0].text, "0");
        assert_eq!(items[2].label()[0].text, "-1");
        assert_eq!(items[2].detail()[0].text, "p.12");
        assert_eq!(items[2].match_text(), "-1 p.12 p.12");
    }

    #[test]
    fn list_orders_matches_by_index_then_reason_then_page() {
        let provider = HistoryPaletteProvider;
        let extensions = ExtensionUiSnapshot {
            history: HistoryUiSnapshot {
                entries: vec![
                    entry("index", 1, 11, HistoryPaletteReason::PageOnly, false),
                    entry(
                        "reason",
                        0,
                        3,
                        HistoryPaletteReason::Search("1".to_string()),
                        true,
                    ),
                    entry("page", -2, 10, HistoryPaletteReason::PageOnly, false),
                ]
                .into(),
            },
            ..ExtensionUiSnapshot::default()
        };
        let ctx = context(&extensions, "1");

        let items = provider.list(&ctx).expect("history list should build");
        let labels: Vec<_> = items
            .iter()
            .map(|item| item.label()[0].text.as_str())
            .collect();
        assert_eq!(labels, vec!["1", "0", "-2"]);
    }

    #[test]
    fn match_bucket_prefers_index_before_reason_before_page() {
        assert_eq!(
            history_match_bucket(
                1,
                12,
                HistoryPaletteReason::Search("needle".to_string()),
                "1"
            ),
            Some(HistoryMatchBucket::Index)
        );
        assert_eq!(
            history_match_bucket(
                -1,
                12,
                HistoryPaletteReason::Search("needle".to_string()),
                "1"
            ),
            Some(HistoryMatchBucket::Index)
        );
        assert_eq!(
            history_match_bucket(
                0,
                12,
                HistoryPaletteReason::Outline("Chapter 1".to_string()),
                "#chap"
            ),
            Some(HistoryMatchBucket::Reason)
        );
        assert_eq!(
            history_match_bucket(
                0,
                12,
                HistoryPaletteReason::Goto("first-page".to_string()),
                "p.12"
            ),
            Some(HistoryMatchBucket::Page)
        );
    }

    #[test]
    fn reason_display_text_matches_visible_labeling() {
        assert_eq!(
            history_reason_display_text(12, HistoryPaletteReason::Search("needle".to_string())),
            "/needle"
        );
        assert_eq!(
            history_reason_display_text(8, HistoryPaletteReason::Outline("Chapter 1".to_string())),
            "#Chapter 1"
        );
        assert_eq!(
            history_reason_display_text(1, HistoryPaletteReason::Goto("first-page".to_string())),
            "first-page"
        );
        assert_eq!(
            history_reason_display_text(5, HistoryPaletteReason::PageOnly),
            "p.5"
        );
    }

    #[test]
    fn initial_selection_uses_current_entry() {
        let provider = HistoryPaletteProvider;
        let extensions = ExtensionUiSnapshot {
            history: HistoryUiSnapshot {
                entries: vec![
                    entry(
                        "future",
                        1,
                        5,
                        HistoryPaletteReason::Search("later".to_string()),
                        false,
                    ),
                    entry("current", 0, 4, HistoryPaletteReason::PageOnly, true),
                    entry(
                        "back",
                        -1,
                        3,
                        HistoryPaletteReason::Search("earlier".to_string()),
                        false,
                    ),
                ]
                .into(),
            },
            ..ExtensionUiSnapshot::default()
        };
        let ctx = context(&extensions, "");
        let candidates = provider.list(&ctx).expect("history list should build");

        assert_eq!(
            provider.initial_selected_candidate(&ctx, &candidates),
            Some(1)
        );
    }
}
