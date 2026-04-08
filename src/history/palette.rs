use crate::command::Command;
use crate::error::AppResult;
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PalettePayload,
    PalettePostAction, PaletteProvider, PaletteSearchText, PaletteSubmitEffect, PaletteTextPart,
};

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
        candidates
            .iter()
            .position(|candidate| candidate.id.starts_with("current-"))
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let seed = ctx.seed.unwrap_or("");
        let parsed = parse_seed(seed, ctx.app.current_page);
        let query = ctx.input.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Ok(build_history_candidates(parsed));
        }

        let mut index_matches = Vec::new();
        let mut reason_matches = Vec::new();
        let mut page_matches = Vec::new();

        for entry in parsed.into_iter() {
            let idx = entry.display_index;
            let page_1indexed = entry.page + 1;
            let view = HistoryEntryView::new(idx, page_1indexed, entry.reason.as_str());
            let bucket = view.match_bucket(idx, page_1indexed, &query);
            let candidate = view.into_candidate(&entry);
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
        _ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        let Some(candidate) = selected else {
            return Ok(PaletteSubmitEffect::Close);
        };

        let page = match &candidate.payload {
            PalettePayload::Opaque(val) => val.parse::<usize>().ok(),
            PalettePayload::None => None,
        };
        let Some(page) = page else {
            return Ok(PaletteSubmitEffect::Close);
        };

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::HistoryGoto { page },
            history_record: None,
            next: PalettePostAction::Close,
        })
    }

    fn assistive_text(
        &self,
        _ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        Some("Enter: jump to page".to_string())
    }

    fn initial_input(&self, _seed: Option<&str>) -> String {
        String::new()
    }
}

fn build_history_candidates(entries: Vec<SeedEntry>) -> Vec<PaletteCandidate> {
    entries
        .into_iter()
        .map(|entry| {
            HistoryEntryView::new(entry.display_index, entry.page + 1, entry.reason.as_str())
                .into_candidate(&entry)
        })
        .collect()
}

#[derive(Debug, Clone)]
struct HistoryEntryView {
    reason: HistoryReasonLabel,
    left: Vec<PaletteTextPart>,
    right: Vec<PaletteTextPart>,
    search_texts: Vec<PaletteSearchText>,
}

impl HistoryEntryView {
    fn new(idx: isize, page_1indexed: usize, reason: &str) -> Self {
        let reason = HistoryReasonLabel::parse(reason);
        let left = reason.left_parts(idx, page_1indexed);
        let right = vec![PaletteTextPart::secondary(page_text(page_1indexed))];
        let search_texts = reason.search_texts(idx, page_1indexed);
        Self {
            reason,
            left,
            right,
            search_texts,
        }
    }

    fn into_candidate(self, entry: &SeedEntry) -> PaletteCandidate {
        let id = if entry.is_current {
            format!("current-{}-{}", entry.page, entry.display_index)
        } else {
            format!("page-{}-{}", entry.page, entry.display_index)
        };

        PaletteCandidate {
            id,
            left: self.left,
            right: self.right,
            search_texts: self.search_texts,
            payload: PalettePayload::Opaque((entry.page + 1).to_string()),
        }
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

fn decode_seed_component(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let mut i = 0;
    let raw = value.as_bytes();
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && let (Some(hi), Some(lo)) = (hex_value(raw[i + 1]), hex_value(raw[i + 2]))
        {
            bytes.push((hi << 4) | lo);
            i += 3;
            continue;
        }

        bytes.push(raw[i]);
        i += 1;
    }

    String::from_utf8(bytes).unwrap_or_else(|_| value.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl HistoryReasonLabel {
    fn parse(reason: &str) -> Self {
        if let Some(query) = reason.strip_prefix("Search:~") {
            let query = decode_seed_component(query);
            if query.is_empty() {
                HistoryReasonLabel::PageOnly
            } else {
                HistoryReasonLabel::Search(query)
            }
        } else if let Some(query) = reason.strip_prefix("Search: ") {
            if query.is_empty() {
                HistoryReasonLabel::PageOnly
            } else {
                HistoryReasonLabel::Search(query.to_string())
            }
        } else if let Some(title) = reason.strip_prefix("Outline:~") {
            let title = decode_seed_component(title);
            if title.is_empty() {
                HistoryReasonLabel::PageOnly
            } else {
                HistoryReasonLabel::Outline(title)
            }
        } else if let Some(title) = reason.strip_prefix("Outline: ") {
            if title.is_empty() {
                HistoryReasonLabel::PageOnly
            } else {
                HistoryReasonLabel::Outline(title.to_string())
            }
        } else if let Some(label) = reason.strip_prefix("Goto:") {
            let label = label.trim_start();
            match label {
                "first-page" | "last-page" => HistoryReasonLabel::Goto(label.to_string()),
                _ => HistoryReasonLabel::PageOnly,
            }
        } else {
            HistoryReasonLabel::PageOnly
        }
    }

    fn left_parts(&self, idx: isize, page_1indexed: usize) -> Vec<PaletteTextPart> {
        let mut parts = Vec::with_capacity(4);
        parts.push(PaletteTextPart::primary(idx.to_string()));
        parts.push(PaletteTextPart::primary("  "));

        match self {
            HistoryReasonLabel::Search(query) => {
                parts.push(PaletteTextPart::secondary("/"));
                parts.push(PaletteTextPart::primary(query.clone()));
            }
            HistoryReasonLabel::Outline(title) => {
                parts.push(PaletteTextPart::secondary("#"));
                parts.push(PaletteTextPart::primary(title.clone()));
            }
            HistoryReasonLabel::Goto(label) => {
                parts.push(PaletteTextPart::primary(label.clone()));
            }
            HistoryReasonLabel::PageOnly => {
                parts.push(PaletteTextPart::primary(page_text(page_1indexed)));
            }
        }

        parts
    }

    fn search_texts(&self, idx: isize, page_1indexed: usize) -> Vec<PaletteSearchText> {
        let mut texts = Vec::with_capacity(3);
        texts.push(PaletteSearchText::new(idx.to_string()));

        match self {
            HistoryReasonLabel::Search(query) => {
                texts.push(PaletteSearchText::new(format!("/{query}")));
                texts.push(PaletteSearchText::new(page_text(page_1indexed)));
            }
            HistoryReasonLabel::Outline(title) => {
                texts.push(PaletteSearchText::new(format!("#{title}")));
                texts.push(PaletteSearchText::new(page_text(page_1indexed)));
            }
            HistoryReasonLabel::Goto(label) => {
                texts.push(PaletteSearchText::new(label.clone()));
                texts.push(PaletteSearchText::new(page_text(page_1indexed)));
            }
            HistoryReasonLabel::PageOnly => {
                texts.push(PaletteSearchText::new(page_text(page_1indexed)));
            }
        }

        texts
    }

    fn display_text(&self, page_1indexed: usize) -> String {
        match self {
            HistoryReasonLabel::Search(query) => format!("/{query}"),
            HistoryReasonLabel::Outline(title) => format!("#{title}"),
            HistoryReasonLabel::Goto(label) => label.clone(),
            HistoryReasonLabel::PageOnly => page_text(page_1indexed),
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

#[cfg(test)]
fn history_parts(
    page_1indexed: usize,
    idx: isize,
    reason: &str,
) -> (
    Vec<PaletteTextPart>,
    Vec<PaletteTextPart>,
    Vec<PaletteSearchText>,
) {
    let view = HistoryEntryView::new(idx, page_1indexed, reason);
    (view.left, view.right, view.search_texts)
}

#[derive(Debug, Clone)]
enum HistoryReasonLabel {
    Search(String),
    Outline(String),
    Goto(String),
    PageOnly,
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
    reason: &str,
    query: &str,
) -> Option<HistoryMatchBucket> {
    HistoryReasonLabel::parse(reason).match_bucket(idx, page_1indexed, query)
}

#[cfg(test)]
fn history_reason_display_text(page_1indexed: usize, reason: &str) -> String {
    HistoryReasonLabel::parse(reason).display_text(page_1indexed)
}

struct SeedEntry {
    page: usize,
    reason: String,
    is_current: bool,
    display_index: isize,
}

fn parse_seed(seed: &str, fallback_current: usize) -> Vec<SeedEntry> {
    let mut back_entries = Vec::new();
    let mut forward_entries = Vec::new();
    let mut current_page = fallback_current;
    let mut current_reason = String::new();

    let parts: Vec<&str> = seed.split('|').collect();
    for part in &parts {
        if let Some(data) = part.strip_prefix("b:") {
            for item in data.split(';') {
                if let Some(entry) = parse_entry(item) {
                    back_entries.push(entry);
                }
            }
        } else if let Some(data) = part.strip_prefix("c:") {
            if let Some(entry) = parse_entry(data) {
                current_page = entry.page;
                current_reason = entry.reason;
            } else if let Ok(page) = data.parse::<usize>() {
                current_page = page;
            }
        } else if let Some(data) = part.strip_prefix("f:") {
            for item in data.split(';') {
                if let Some(entry) = parse_entry(item) {
                    forward_entries.push(entry);
                }
            }
        }
    }

    let mut entries = Vec::new();
    for (i, entry) in forward_entries.into_iter().enumerate().rev() {
        entries.push(SeedEntry {
            page: entry.page,
            reason: entry.reason,
            is_current: false,
            display_index: (i as isize) + 1,
        });
    }
    entries.push(SeedEntry {
        page: current_page,
        reason: current_reason,
        is_current: true,
        display_index: 0,
    });
    for (i, entry) in back_entries.into_iter().rev().enumerate() {
        entries.push(SeedEntry {
            page: entry.page,
            reason: entry.reason,
            is_current: false,
            display_index: -((i as isize) + 1),
        });
    }
    entries
}

fn parse_entry(item: &str) -> Option<SeedEntry> {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (page_str, reason) = match trimmed.find(',') {
        Some(idx) => (&trimmed[..idx], trimmed[idx + 1..].to_string()),
        None => (trimmed, String::new()),
    };
    let page = page_str.parse::<usize>().ok()?;
    Some(SeedEntry {
        page,
        reason,
        is_current: false,
        display_index: 0,
    })
}

#[cfg(test)]
mod tests {
    use crate::palette::PaletteTextTone;
    use crate::{
        app::AppState,
        extension::ExtensionUiSnapshot,
        palette::{PaletteCandidate, PaletteContext, PalettePayload, PaletteProvider},
    };

    use super::{
        HistoryMatchBucket, HistoryPaletteProvider, history_match_bucket, history_parts,
        history_reason_display_text,
    };

    #[test]
    fn search_reason_renders_secondary_prefix_and_right_page() {
        let (left, right, search_texts) = history_parts(12, 1, "Search: needle");

        assert_eq!(left.len(), 4);
        assert_eq!(left[0].text.trim(), "1");
        assert!(matches!(left[0].tone, PaletteTextTone::Primary));
        assert_eq!(left[2].text, "/");
        assert!(matches!(left[2].tone, PaletteTextTone::Secondary));
        assert_eq!(left[3].text, "needle");
        assert!(matches!(left[3].tone, PaletteTextTone::Primary));
        assert_eq!(right.len(), 1);
        assert_eq!(right[0].text, "p.12");
        assert!(matches!(right[0].tone, PaletteTextTone::Secondary));
        assert_eq!(search_texts[0].text, "1");
        assert_eq!(search_texts[1].text, "/needle");
        assert_eq!(search_texts[2].text, "p.12");
        assert_eq!(search_texts.len(), 3);
    }

    #[test]
    fn outline_reason_renders_secondary_prefix_and_right_page() {
        let (left, right, search_texts) = history_parts(8, 2, "Outline: Chapter 1");

        assert_eq!(left.len(), 4);
        assert_eq!(left[0].text.trim(), "2");
        assert_eq!(left[2].text, "#");
        assert!(matches!(left[2].tone, PaletteTextTone::Secondary));
        assert_eq!(left[3].text, "Chapter 1");
        assert!(matches!(left[3].tone, PaletteTextTone::Primary));
        assert_eq!(right[0].text, "p.8");
        assert!(matches!(right[0].tone, PaletteTextTone::Secondary));
        assert_eq!(search_texts[0].text, "2");
        assert_eq!(search_texts[1].text, "#Chapter 1");
        assert_eq!(search_texts[2].text, "p.8");
        assert_eq!(search_texts.len(), 3);
    }

    #[test]
    fn goto_first_page_renders_label_and_right_page() {
        let (left, right, search_texts) = history_parts(1, 3, "Goto:first-page");

        assert_eq!(left.len(), 3);
        assert_eq!(left[2].text, "first-page");
        assert!(matches!(left[2].tone, PaletteTextTone::Primary));
        assert_eq!(right[0].text, "p.1");
        assert!(matches!(right[0].tone, PaletteTextTone::Secondary));
        assert_eq!(search_texts[0].text, "3");
        assert_eq!(search_texts[1].text, "first-page");
        assert_eq!(search_texts[2].text, "p.1");
        assert_eq!(search_texts.len(), 3);
    }

    #[test]
    fn goto_last_page_renders_label_and_right_page() {
        let (left, right, search_texts) = history_parts(9, 4, "Goto:last-page");

        assert_eq!(left.len(), 3);
        assert_eq!(left[2].text, "last-page");
        assert!(matches!(left[2].tone, PaletteTextTone::Primary));
        assert_eq!(right[0].text, "p.9");
        assert!(matches!(right[0].tone, PaletteTextTone::Secondary));
        assert_eq!(search_texts[0].text, "4");
        assert_eq!(search_texts[1].text, "last-page");
        assert_eq!(search_texts[2].text, "p.9");
        assert_eq!(search_texts.len(), 3);
    }

    #[test]
    fn assistive_text_reflects_selected_candidate() {
        let provider = HistoryPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: crate::palette::PaletteKind::History,
            input: "",
            seed: None,
        };

        let current = PaletteCandidate {
            id: "current-0-0".to_string(),
            left: Vec::new(),
            right: Vec::new(),
            search_texts: Vec::new(),
            payload: PalettePayload::None,
        };
        let other = PaletteCandidate {
            id: "page-1-0".to_string(),
            left: Vec::new(),
            right: Vec::new(),
            search_texts: Vec::new(),
            payload: PalettePayload::None,
        };

        assert_eq!(
            provider.assistive_text(&ctx, Some(&current)),
            Some("Enter: jump to page".to_string())
        );
        assert_eq!(
            provider.assistive_text(&ctx, Some(&other)),
            Some("Enter: jump to page".to_string())
        );
        assert_eq!(
            provider.assistive_text(&ctx, None),
            Some("Enter: jump to page".to_string())
        );
    }

    #[test]
    fn page_only_reason_renders_page_on_both_sides() {
        let (left, right, search_texts) = history_parts(5, 3, "");

        assert_eq!(left.len(), 3);
        assert_eq!(left[2].text, "p.5");
        assert!(matches!(left[2].tone, PaletteTextTone::Primary));
        assert_eq!(right[0].text, "p.5");
        assert!(matches!(right[0].tone, PaletteTextTone::Secondary));
        assert_eq!(search_texts[0].text, "3");
        assert_eq!(search_texts[1].text, "p.5");
        assert_eq!(search_texts.len(), 2);
    }

    #[test]
    fn list_orders_matches_by_index_then_reason_then_page() {
        let provider = HistoryPaletteProvider;
        let seed = "b:11|c:0,Search: 1|f:9";
        let app = AppState {
            current_page: 4,
            ..AppState::default()
        };
        let extensions = ExtensionUiSnapshot::default();

        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: crate::palette::PaletteKind::History,
            input: "",
            seed: Some(seed),
        };

        let items = provider.list(&ctx).expect("history list should build");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].left[0].text, "1");
        assert_eq!(items[1].left[0].text, "0");
        assert_eq!(items[2].left[0].text, "-1");
    }

    #[test]
    fn list_orders_forward_entries_before_current_in_reverse_stack_order() {
        let provider = HistoryPaletteProvider;
        let seed = "b:11|c:12|f:13,Search: one;14,Search: two;15,Search: three;16,Search: four";
        let app = AppState {
            current_page: 12,
            ..AppState::default()
        };
        let extensions = ExtensionUiSnapshot::default();

        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: crate::palette::PaletteKind::History,
            input: "",
            seed: Some(seed),
        };

        let items = provider.list(&ctx).expect("history list should build");
        let labels: Vec<_> = items
            .iter()
            .map(|item| item.left[0].text.as_str())
            .collect();
        assert_eq!(labels, vec!["4", "3", "2", "1", "0", "-1"]);
    }

    #[test]
    fn match_bucket_prefers_index_before_reason_before_page() {
        assert_eq!(
            history_match_bucket(1, 12, "Search: needle", "1"),
            Some(HistoryMatchBucket::Index)
        );
        assert_eq!(
            history_match_bucket(-1, 12, "Search: needle", "1"),
            Some(HistoryMatchBucket::Index)
        );
        assert_eq!(
            history_match_bucket(0, 12, "Outline: Chapter 1", "#chap"),
            Some(HistoryMatchBucket::Reason)
        );
        assert_eq!(
            history_match_bucket(0, 12, "Goto:first-page", "p.12"),
            Some(HistoryMatchBucket::Page)
        );
    }

    #[test]
    fn reason_display_text_matches_visible_labeling() {
        assert_eq!(history_reason_display_text(12, "Search: needle"), "/needle");
        assert_eq!(
            history_reason_display_text(8, "Outline: Chapter 1"),
            "#Chapter 1"
        );
        assert_eq!(
            history_reason_display_text(1, "Goto:first-page"),
            "first-page"
        );
        assert_eq!(history_reason_display_text(5, ""), "p.5");
    }

    #[test]
    fn reason_display_text_decodes_escaped_components() {
        assert_eq!(
            history_reason_display_text(12, "Search:~needle%7Ctwo"),
            "/needle|two"
        );
        assert_eq!(
            history_reason_display_text(8, "Outline:~Chapter%3B%201"),
            "#Chapter; 1"
        );
    }
}
