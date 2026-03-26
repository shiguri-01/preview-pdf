use crate::command::Command;
use crate::error::AppResult;
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PalettePayload,
    PalettePostAction, PaletteProvider, PaletteSearchText, PaletteSubmitEffect, PaletteTextPart,
    PaletteTextTone,
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
        PaletteInputMode::FilterCandidates
    }

    fn reset_selection_on_input_change(&self) -> bool {
        true
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let seed = ctx.seed.unwrap_or("");
        let parsed = parse_seed(seed, ctx.app.current_page);
        Ok(parsed
            .into_iter()
            .enumerate()
            .map(|(i, entry)| {
                let idx = i + 1;
                let page_1indexed = entry.page + 1;
                let reason_detail = if entry.reason.is_empty() {
                    None
                } else {
                    Some(format!("[{}]", entry.reason))
                };
                let id = if entry.is_current {
                    format!("current-{}", entry.page)
                } else {
                    format!("page-{}", entry.page)
                };
                PaletteCandidate {
                    id,
                    left: history_left_parts(idx, page_1indexed, entry.is_current),
                    right: reason_detail.into_iter().map(secondary).collect(),
                    search_texts: history_search_texts(
                        page_1indexed,
                        entry.reason.as_str(),
                        entry.is_current,
                    ),
                    payload: PalettePayload::Opaque(page_1indexed.to_string()),
                }
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

        if candidate.id.starts_with("current-") {
            return Ok(PaletteSubmitEffect::Close);
        }

        let page = match &candidate.payload {
            PalettePayload::Opaque(val) => val.parse::<usize>().ok(),
            PalettePayload::None => None,
        };
        let Some(page) = page else {
            return Ok(PaletteSubmitEffect::Close);
        };

        Ok(PaletteSubmitEffect::Dispatch {
            command: Command::HistoryGoto { page },
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

fn history_left_parts(idx: usize, page_1indexed: usize, is_current: bool) -> Vec<PaletteTextPart> {
    let mut parts = Vec::new();
    parts.push(primary(format!("{idx:2}")));
    parts.push(primary("  "));
    if is_current {
        parts.push(secondary("> "));
    }
    parts.push(primary(format!("Page {page_1indexed}")));
    parts
}

fn primary(text: impl Into<String>) -> PaletteTextPart {
    PaletteTextPart {
        text: text.into(),
        tone: PaletteTextTone::Primary,
    }
}

fn secondary(text: impl Into<String>) -> PaletteTextPart {
    PaletteTextPart {
        text: text.into(),
        tone: PaletteTextTone::Secondary,
    }
}

fn search(text: impl Into<String>) -> PaletteSearchText {
    PaletteSearchText { text: text.into() }
}

fn history_search_texts(
    page_1indexed: usize,
    reason: &str,
    is_current: bool,
) -> Vec<PaletteSearchText> {
    let mut texts = vec![
        search(format!("page {page_1indexed}")),
        search(page_1indexed.to_string()),
    ];
    if !reason.is_empty() {
        texts.push(search(reason.to_string()));
    }
    if is_current {
        texts.push(search("current"));
    }
    texts
}

struct SeedEntry {
    page: usize,
    reason: String,
    is_current: bool,
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
    forward_entries.reverse();
    entries.append(&mut forward_entries);
    entries.push(SeedEntry {
        page: current_page,
        reason: current_reason,
        is_current: true,
    });
    back_entries.reverse();
    entries.append(&mut back_entries);
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
    })
}
