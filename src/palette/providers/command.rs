use crate::command::all_command_specs;
use crate::command::parse_command_text;
use crate::error::AppResult;
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PalettePayload,
    PalettePostAction, PaletteProvider, PaletteSubmitEffect, PaletteTabEffect,
};

pub struct CommandPaletteProvider;

impl PaletteProvider for CommandPaletteProvider {
    fn kind(&self) -> PaletteKind {
        PaletteKind::Command
    }

    fn title(&self, _ctx: &PaletteContext<'_>) -> String {
        "Command".to_string()
    }

    fn input_mode(&self) -> PaletteInputMode {
        PaletteInputMode::Custom
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        if has_argument_phase(ctx.input) {
            return Ok(Vec::new());
        }

        let mut candidates = all_command_specs()
            .into_iter()
            .filter(|spec| can_show_command_spec(spec.id, ctx))
            .map(|spec| PaletteCandidate {
                id: spec.id.to_string(),
                label: spec.id.to_string(),
                detail: Some(format_detail(spec.title, spec.args)),
                payload: PalettePayload::Opaque(spec.id.to_string()),
            })
            .collect::<Vec<_>>();
        rank_command_candidates(ctx.input, &mut candidates);
        Ok(candidates)
    }

    fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        let input = ctx.input.trim();

        // 1. Input text parses as a valid command (with args) → dispatch directly.
        if !input.is_empty()
            && let Ok(command) = parse_command_text(input)
        {
            return Ok(PaletteSubmitEffect::Dispatch {
                command,
                next: PalettePostAction::Close,
            });
        }

        // 2. A candidate is selected → use it.
        if let Some(candidate) = selected
            && let Some(spec) = find_spec(&candidate.id)
        {
            if spec.args.is_empty() {
                // No args needed: dispatch immediately.
                if let Ok(command) = parse_command_text(spec.id) {
                    return Ok(PaletteSubmitEffect::Dispatch {
                        command,
                        next: PalettePostAction::Close,
                    });
                }
            } else {
                // Args required: reopen with command name pre-filled.
                return Ok(PaletteSubmitEffect::Reopen {
                    kind: self.kind(),
                    seed: Some(format!("{} ", spec.id)),
                });
            }
        }

        // 3. Fallback: reopen preserving current input.
        Ok(PaletteSubmitEffect::Reopen {
            kind: self.kind(),
            seed: Some(ctx.input.to_string()),
        })
    }

    fn on_tab(
        &self,
        _ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteTabEffect> {
        let Some(candidate) = selected else {
            return Ok(PaletteTabEffect::Noop);
        };

        let value = match &candidate.payload {
            PalettePayload::Opaque(value) => value.clone(),
            PalettePayload::None => candidate.label.clone(),
        };

        Ok(PaletteTabEffect::SetInput {
            value,
            move_cursor_to_end: true,
        })
    }

    fn assistive_text(
        &self,
        ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        let trimmed = ctx.input.trim();
        if trimmed.is_empty() {
            return Some("Enter: run  Tab: complete".to_string());
        }

        if has_argument_phase(ctx.input) {
            let command_id = first_token(trimmed);
            return match find_spec(command_id) {
                Some(spec) => {
                    let usage = usage_text(spec.args);
                    if usage.is_empty() {
                        Some(format!("{} | {}", spec.id, spec.title))
                    } else {
                        Some(format!("{} {} | {}", spec.id, usage, spec.title))
                    }
                }
                None => Some("Enter: run  Tab: complete".to_string()),
            };
        }

        if let Some(spec) = find_spec(trimmed) {
            let usage = usage_text(spec.args);
            if usage.is_empty() {
                return Some(format!("{} | {}", spec.id, spec.title));
            } else {
                return Some(format!("{} {} | {}", spec.id, usage, spec.title));
            }
        }

        Some("Enter: run  Tab: complete".to_string())
    }
}

fn format_detail(title: &str, args: &[crate::command::ArgSpec]) -> String {
    let usage = usage_text(args);
    if usage.is_empty() {
        format!("| {title}")
    } else {
        format!("{usage} | {title}")
    }
}

fn usage_text(args: &[crate::command::ArgSpec]) -> String {
    if args.is_empty() {
        return String::new();
    }
    let mut usage = String::new();
    for arg in args {
        if !usage.is_empty() {
            usage.push(' ');
        }
        if arg.required {
            usage.push('<');
            usage.push_str(arg.name);
            usage.push('>');
        } else {
            usage.push('[');
            usage.push_str(arg.name);
            usage.push(']');
        }
    }
    usage
}

fn has_argument_phase(input: &str) -> bool {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.contains(char::is_whitespace)
}

fn first_token(input: &str) -> &str {
    match input.find(char::is_whitespace) {
        Some(index) => &input[..index],
        None => input,
    }
}

fn find_spec(id: &str) -> Option<crate::command::CommandSpec> {
    all_command_specs().into_iter().find(|spec| spec.id == id)
}

fn can_show_command_spec(id: &str, ctx: &PaletteContext<'_>) -> bool {
    if is_search_navigation_command(id) {
        return ctx.app.search_ui.active;
    }
    true
}

fn is_search_navigation_command(id: &str) -> bool {
    matches!(id, "next-search-hit" | "prev-search-hit")
}

const SCORE_ID_EXACT: i32 = 10_000;
const SCORE_ID_PREFIX: i32 = 9_000;
const SCORE_ID_TOKEN_PREFIX: i32 = 8_000;
const SCORE_ID_ACRONYM: i32 = 7_000;
const SCORE_ID_CONTAINS: i32 = 6_000;
const SCORE_ID_SUBSEQUENCE: i32 = 5_000;
const SCORE_TITLE_PREFIX: i32 = 800;
const SCORE_TITLE_CONTAINS: i32 = 700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateScore {
    score: i32,
    tie_len: usize,
}

fn rank_command_candidates(input: &str, candidates: &mut Vec<PaletteCandidate>) {
    let query = input.trim().to_ascii_lowercase();
    if query.is_empty() {
        return;
    }

    let mut scored = candidates
        .drain(..)
        .filter_map(|candidate| {
            score_command_candidate(&query, &candidate).map(|meta| (candidate, meta))
        })
        .collect::<Vec<_>>();

    scored.sort_by(
        |(left_candidate, left_meta), (right_candidate, right_meta)| {
            right_meta
                .score
                .cmp(&left_meta.score)
                .then_with(|| left_meta.tie_len.cmp(&right_meta.tie_len))
                .then_with(|| left_candidate.id.cmp(&right_candidate.id))
        },
    );

    *candidates = scored
        .into_iter()
        .map(|(candidate, _meta)| candidate)
        .collect();
}

fn score_command_candidate(query: &str, candidate: &PaletteCandidate) -> Option<CandidateScore> {
    let id = candidate.id.to_ascii_lowercase();
    let title = extract_title(candidate).to_ascii_lowercase();

    let id_score = score_id(query, &id);
    let title_score = score_title(query, &title);
    let score = id_score.max(title_score);
    if score <= 0 {
        return None;
    }

    Some(CandidateScore {
        score,
        tie_len: id.len(),
    })
}

fn extract_title(candidate: &PaletteCandidate) -> &str {
    let Some(detail) = candidate.detail.as_deref() else {
        return "";
    };
    let Some((_, title)) = detail.split_once('|') else {
        return "";
    };
    title.trim()
}

fn score_id(query: &str, id: &str) -> i32 {
    if id == query {
        return SCORE_ID_EXACT;
    }
    if id.starts_with(query) {
        return SCORE_ID_PREFIX;
    }
    if token_prefix_match(query, id) {
        return SCORE_ID_TOKEN_PREFIX;
    }
    if acronym_match(query, id) {
        return SCORE_ID_ACRONYM;
    }
    if id.contains(query) {
        return SCORE_ID_CONTAINS;
    }
    if is_subsequence(query, id) {
        return SCORE_ID_SUBSEQUENCE;
    }
    0
}

fn score_title(query: &str, title: &str) -> i32 {
    if title.is_empty() {
        return 0;
    }
    if title.starts_with(query) {
        return SCORE_TITLE_PREFIX;
    }
    if title.contains(query) {
        return SCORE_TITLE_CONTAINS;
    }
    0
}

fn token_prefix_match(query: &str, id: &str) -> bool {
    id.split('-').any(|token| token.starts_with(query))
}

fn acronym_match(query: &str, id: &str) -> bool {
    let acronym = id
        .split('-')
        .filter(|token| !token.is_empty())
        .filter_map(|token| token.chars().next())
        .collect::<String>();
    !acronym.is_empty() && acronym.starts_with(query)
}

fn is_subsequence(query: &str, text: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    let mut query_chars = query.chars();
    let mut current = match query_chars.next() {
        Some(ch) => ch,
        None => return true,
    };

    for text_char in text.chars() {
        if text_char == current {
            if let Some(next) = query_chars.next() {
                current = next;
            } else {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use crate::app::AppState;
    use crate::palette::{PaletteContext, PaletteKind, PaletteProvider};

    use super::CommandPaletteProvider;

    fn ids(list: &[crate::palette::PaletteCandidate]) -> Vec<String> {
        list.iter().map(|candidate| candidate.id.clone()).collect()
    }

    fn command_list_for_input(
        input: &str,
        search_active: bool,
    ) -> Vec<crate::palette::PaletteCandidate> {
        let provider = CommandPaletteProvider;
        let mut app = AppState::default();
        app.search_ui.active = search_active;
        let ctx = PaletteContext {
            app: &app,
            kind: PaletteKind::Command,
            input,
            seed: None,
        };
        provider.list(&ctx).expect("list should be built")
    }

    #[test]
    fn list_hides_search_hit_navigation_when_search_is_inactive() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let ctx = PaletteContext {
            app: &app,
            kind: PaletteKind::Command,
            input: "",
            seed: None,
        };

        let list = provider.list(&ctx).expect("list should be built");
        assert!(
            !list
                .iter()
                .any(|candidate| candidate.id == "next-search-hit")
        );
        assert!(
            !list
                .iter()
                .any(|candidate| candidate.id == "prev-search-hit")
        );
    }

    #[test]
    fn list_shows_search_hit_navigation_when_search_is_active() {
        let provider = CommandPaletteProvider;
        let mut app = AppState::default();
        app.search_ui.active = true;
        let ctx = PaletteContext {
            app: &app,
            kind: PaletteKind::Command,
            input: "",
            seed: None,
        };

        let list = provider.list(&ctx).expect("list should be built");
        assert!(
            list.iter()
                .any(|candidate| candidate.id == "next-search-hit")
        );
        assert!(
            list.iter()
                .any(|candidate| candidate.id == "prev-search-hit")
        );
    }

    #[test]
    fn argument_phase_still_hides_candidates() {
        let list = command_list_for_input("goto-page ", false);
        assert!(list.is_empty());
    }

    #[test]
    fn scoring_prioritizes_exact_id_match() {
        let list = command_list_for_input("quit", false);
        assert_eq!(
            list.first().map(|candidate| candidate.id.as_str()),
            Some("quit")
        );
    }

    #[test]
    fn scoring_prioritizes_id_prefix_over_contains() {
        let list = command_list_for_input("search", true);
        let ids = ids(&list);
        let idx_search = ids
            .iter()
            .position(|id| id == "search")
            .expect("search should exist");
        let idx_next_search_hit = ids
            .iter()
            .position(|id| id == "next-search-hit")
            .expect("next-search-hit should exist");
        assert!(idx_search < idx_next_search_hit);
    }

    #[test]
    fn scoring_supports_hyphen_acronym_query() {
        let list = command_list_for_input("nsh", true);
        assert_eq!(
            list.first().map(|candidate| candidate.id.as_str()),
            Some("next-search-hit")
        );
    }

    #[test]
    fn scoring_prefers_id_matches_over_title_only_matches() {
        let list = command_list_for_input("open", false);
        let ids = ids(&list);
        let idx_open_palette = ids
            .iter()
            .position(|id| id == "open-palette")
            .expect("open-palette should exist");
        let idx_history = ids
            .iter()
            .position(|id| id == "history")
            .expect("history should exist");
        assert!(
            idx_open_palette < idx_history,
            "id matches should outrank title-only matches"
        );
    }

    #[test]
    fn scoring_tie_breaks_by_shorter_id_then_lexicographic() {
        let list = command_list_for_input("page", false);
        let ids = ids(&list);
        let idx_goto_page = ids
            .iter()
            .position(|id| id == "goto-page")
            .expect("goto-page should exist");
        let idx_last_page = ids
            .iter()
            .position(|id| id == "last-page")
            .expect("last-page should exist");
        let idx_next_page = ids
            .iter()
            .position(|id| id == "next-page")
            .expect("next-page should exist");
        let idx_prev_page = ids
            .iter()
            .position(|id| id == "prev-page")
            .expect("prev-page should exist");

        assert!(idx_goto_page < idx_last_page);
        assert!(idx_last_page < idx_next_page);
        assert!(idx_next_page < idx_prev_page);
    }
}
