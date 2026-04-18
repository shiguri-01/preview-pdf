use crate::command::all_command_specs;
use crate::command::find_command_spec;
use crate::command::first_token;
use crate::command::is_command_visible_in_palette;
use crate::command::parse_command_text;
use crate::command::parse_invocable_command_text;
use crate::command::{ArgHint, ArgKind, ArgSpec, CommandConditionContext, CommandInvocationSource};
use crate::error::AppResult;
use crate::input::InputHistoryRecord;
use crate::input::shortcut::{
    ShortcutKey, format_shortcut_alternatives_tight, format_shortcut_key,
};
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PaletteOpenPayload,
    PalettePayload, PalettePostAction, PaletteProvider, PaletteSearchText, PaletteSubmitEffect,
    PaletteTabEffect, PaletteTextPart,
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

    fn reset_selection_on_input_change(&self) -> bool {
        true
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        let analysis = analyze_command_input(ctx.input);
        match analysis.active_argument {
            Some(argument) if argument.is_enum() => {
                let mut candidates = argument
                    .values()
                    .iter()
                    .map(|value| enum_value_candidate(value))
                    .collect::<Vec<_>>();
                filter_enum_candidates(argument.token, &mut candidates);
                Ok(candidates)
            }
            Some(_) => Ok(Vec::new()),
            None => {
                let mut candidates = all_command_specs()
                    .into_iter()
                    .filter(|spec| {
                        let command_ctx = CommandConditionContext {
                            app: ctx.app,
                            extensions: ctx.extensions,
                            source: CommandInvocationSource::CommandPaletteInput,
                        };
                        is_command_visible_in_palette(*spec, &command_ctx)
                    })
                    .map(|spec| PaletteCandidate {
                        id: spec.id.to_string(),
                        left: format_left(spec.id, spec.args),
                        right: vec![PaletteTextPart::secondary(spec.title)],
                        search_texts: format_search_texts(spec.id, spec.title, spec.args),
                        payload: PalettePayload::Opaque(spec.id.to_string()),
                    })
                    .collect::<Vec<_>>();
                rank_command_candidates(ctx.input, &mut candidates);
                Ok(candidates)
            }
        }
    }

    fn on_submit(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteSubmitEffect> {
        let input = ctx.input.trim();

        if let Some(effect) = submit_selected_enum_candidate(ctx, selected)? {
            return Ok(effect);
        }

        let mut deferred_error = None;
        if !input.is_empty() {
            match parse_invocable_command_text(
                input,
                CommandInvocationSource::CommandPaletteInput,
                ctx.app,
                ctx.extensions,
            ) {
                Ok(command) => {
                    return Ok(PaletteSubmitEffect::Dispatch {
                        command,
                        history_record: Some(InputHistoryRecord::Command(input.to_string())),
                        next: PalettePostAction::Close,
                    });
                }
                Err(err) if find_command_spec(first_token(input)).is_some() => {
                    deferred_error = Some(err);
                }
                Err(_) => {}
            }
        }

        // 2. A candidate is selected → use it.
        if let Some(candidate) = selected
            && let Some(spec) = find_command_spec(&candidate.id)
        {
            if !command_requires_argument_input(spec) {
                // No args needed: dispatch immediately.
                if let Ok(command) = parse_command_text(spec.id) {
                    return Ok(PaletteSubmitEffect::Dispatch {
                        command,
                        history_record: Some(InputHistoryRecord::Command(spec.id.to_string())),
                        next: PalettePostAction::Close,
                    });
                }
            } else {
                // Args required: reopen with command name pre-filled.
                return Ok(PaletteSubmitEffect::Reopen {
                    kind: self.kind(),
                    payload: Some(PaletteOpenPayload::CommandInput(format!("{} ", spec.id))),
                });
            }
        }

        if let Some(err) = deferred_error {
            return Err(err);
        }

        // 3. Fallback: reopen preserving current input.
        Ok(PaletteSubmitEffect::Reopen {
            kind: self.kind(),
            payload: Some(PaletteOpenPayload::CommandInput(ctx.input.to_string())),
        })
    }

    fn on_tab(
        &self,
        ctx: &PaletteContext<'_>,
        selected: Option<&PaletteCandidate>,
    ) -> AppResult<PaletteTabEffect> {
        let Some(candidate) = selected else {
            return Ok(PaletteTabEffect::Noop);
        };

        let analysis = analyze_command_input(ctx.input);
        if let Some(value) = selected_enum_value(&analysis, candidate) {
            return Ok(PaletteTabEffect::SetInput {
                value: apply_enum_completion(&analysis, value),
                move_cursor_to_end: true,
            });
        }

        let value = match &candidate.payload {
            PalettePayload::Opaque(value) => value.clone(),
            PalettePayload::None => candidate.plain_left_text(),
        };

        Ok(PaletteTabEffect::SetInput {
            // Keep completion uniform so the next keystroke can always start an argument.
            value: format!("{value} "),
            move_cursor_to_end: true,
        })
    }

    fn assistive_text(
        &self,
        ctx: &PaletteContext<'_>,
        _selected: Option<&PaletteCandidate>,
    ) -> Option<String> {
        let enter = format_shortcut_key(ShortcutKey::key(crossterm::event::KeyCode::Enter));
        let selection =
            format_shortcut_alternatives_tight(&[ShortcutKey::ctrl('p'), ShortcutKey::ctrl('n')]);
        let history = format_shortcut_alternatives_tight(&[
            ShortcutKey::key(crossterm::event::KeyCode::Up),
            ShortcutKey::key(crossterm::event::KeyCode::Down),
        ]);
        let default_hint = format!("{enter} run   {selection} select   {history} history");
        let trimmed = ctx.input.trim();
        if trimmed.is_empty() {
            return Some(default_hint);
        }

        let analysis = analyze_command_input(ctx.input);
        match analysis.active_argument {
            Some(argument) if argument.is_enum() => {
                return Some(format!(
                    "{} {} | {}: {}",
                    argument.spec.id,
                    usage_text(argument.spec.args),
                    argument.arg.name,
                    argument.values().join(" / ")
                ));
            }
            Some(argument) => {
                return Some(format!(
                    "{} {} | {}: {}",
                    argument.spec.id,
                    usage_text(argument.spec.args),
                    argument.arg.name,
                    ui_type_label(argument.arg.kind)
                ));
            }
            None => {}
        }

        if let Some(spec) = analysis.command_spec {
            let usage = usage_text(spec.args);
            if usage.is_empty() {
                return Some(format!("{} | {}", spec.id, spec.title));
            }
            return Some(format!("{} {} | {}", spec.id, usage, spec.title));
        }

        Some(default_hint)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveArgument<'a> {
    spec: crate::command::CommandSpec,
    arg: ArgSpec,
    index: usize,
    token: &'a str,
}

impl ActiveArgument<'_> {
    fn is_enum(self) -> bool {
        matches!(self.arg.hint, ArgHint::Enum(_))
    }

    fn values(self) -> &'static [&'static str] {
        match self.arg.hint {
            ArgHint::Enum(values) => values(),
            ArgHint::None => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommandInputAnalysis<'a> {
    trimmed_input: &'a str,
    command_spec: Option<crate::command::CommandSpec>,
    active_argument: Option<ActiveArgument<'a>>,
}

fn format_left(command_id: &str, args: &[crate::command::ArgSpec]) -> Vec<PaletteTextPart> {
    let usage = usage_text(args);
    let mut parts = vec![PaletteTextPart::primary(command_id)];
    if !usage.is_empty() {
        parts.push(PaletteTextPart::primary(" "));
        parts.push(PaletteTextPart::secondary(usage));
    }
    parts
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

fn command_requires_argument_input(spec: crate::command::CommandSpec) -> bool {
    spec.args.iter().any(|arg| arg.required)
}

fn ui_type_label(kind: ArgKind) -> &'static str {
    match kind {
        ArgKind::I32 => "integer",
        ArgKind::F32 => "number",
        ArgKind::String => "text",
    }
}

fn enum_value_candidate(value: &str) -> PaletteCandidate {
    PaletteCandidate {
        id: value.to_string(),
        left: vec![PaletteTextPart::primary(value)],
        right: Vec::new(),
        search_texts: vec![PaletteSearchText::new(value)],
        payload: PalettePayload::Opaque(value.to_string()),
    }
}

fn analyze_command_input(input: &str) -> CommandInputAnalysis<'_> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return CommandInputAnalysis {
            trimmed_input: trimmed,
            command_spec: None,
            active_argument: None,
        };
    }

    let Some(spec) = find_command_spec(first_token(trimmed)) else {
        return CommandInputAnalysis {
            trimmed_input: trimmed,
            command_spec: None,
            active_argument: None,
        };
    };

    CommandInputAnalysis {
        trimmed_input: trimmed,
        command_spec: Some(spec),
        active_argument: active_argument(trimmed, spec),
    }
}

fn active_argument<'a>(
    input: &'a str,
    spec: crate::command::CommandSpec,
) -> Option<ActiveArgument<'a>> {
    let trimmed = input.trim_start();
    let split_idx = trimmed.find(char::is_whitespace)?;
    let args_text = trimmed[split_idx..].trim_start();
    let has_trailing_whitespace = trimmed.chars().last().is_some_and(char::is_whitespace);

    let tokens = args_text.split_whitespace().collect::<Vec<_>>();
    let active_index = if has_trailing_whitespace {
        tokens.len()
    } else {
        tokens.len().checked_sub(1)?
    };
    let arg = *spec.args.get(active_index)?;
    let token = if has_trailing_whitespace {
        ""
    } else {
        tokens.get(active_index).copied().unwrap_or("")
    };

    Some(ActiveArgument {
        spec,
        arg,
        index: active_index,
        token,
    })
}

fn selected_enum_value<'a>(
    analysis: &CommandInputAnalysis<'_>,
    candidate: &'a PaletteCandidate,
) -> Option<&'a str> {
    analysis
        .active_argument
        .is_some_and(ActiveArgument::is_enum)
        .then_some(match &candidate.payload {
            PalettePayload::Opaque(value) => value.as_str(),
            PalettePayload::None => "",
        })
        .filter(|value| !value.is_empty())
}

fn apply_enum_completion(analysis: &CommandInputAnalysis<'_>, value: &str) -> String {
    let Some(spec) = analysis.command_spec else {
        return format!("{value} ");
    };
    let Some(active_argument) = analysis.active_argument else {
        return format!("{value} ");
    };

    let mut parts = vec![spec.id];
    let existing_args = analysis
        .trimmed_input
        .split_whitespace()
        .skip(1)
        .take(active_argument.index)
        .collect::<Vec<_>>();
    parts.extend(existing_args);
    parts.push(value);

    format!("{} ", parts.join(" "))
}

fn submit_selected_enum_candidate(
    ctx: &PaletteContext<'_>,
    selected: Option<&PaletteCandidate>,
) -> AppResult<Option<PaletteSubmitEffect>> {
    let Some(candidate) = selected else {
        return Ok(None);
    };
    let analysis = analyze_command_input(ctx.input);
    let Some(value) = selected_enum_value(&analysis, candidate) else {
        return Ok(None);
    };

    let synthesized = apply_enum_completion(&analysis, value);
    let synthesized_trimmed = synthesized.trim();
    match parse_invocable_command_text(
        synthesized_trimmed,
        CommandInvocationSource::CommandPaletteInput,
        ctx.app,
        ctx.extensions,
    ) {
        Ok(command) => Ok(Some(PaletteSubmitEffect::Dispatch {
            command,
            history_record: Some(InputHistoryRecord::Command(synthesized_trimmed.to_string())),
            next: PalettePostAction::Close,
        })),
        Err(_) => Ok(Some(PaletteSubmitEffect::Reopen {
            kind: PaletteKind::Command,
            payload: Some(PaletteOpenPayload::CommandInput(synthesized)),
        })),
    }
}

const SCORE_ID_EXACT: i32 = 10_000;
const SCORE_ID_PREFIX: i32 = 9_000;
const SCORE_ID_TOKEN_PREFIX: i32 = 8_000;
const SCORE_ID_ACRONYM: i32 = 7_000;
const SCORE_ID_CONTAINS: i32 = 6_000;
const SCORE_ID_SUBSEQUENCE: i32 = 5_000;
const SCORE_SEARCH_TEXT_PREFIX: i32 = 800;
const SCORE_SEARCH_TEXT_CONTAINS: i32 = 700;

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

fn filter_enum_candidates(input: &str, candidates: &mut Vec<PaletteCandidate>) {
    let query = input.trim().to_ascii_lowercase();
    if query.is_empty() {
        return;
    }

    let filtered: Vec<_> = candidates
        .iter()
        .filter(|candidate| score_command_candidate(&query, candidate).is_some())
        .cloned()
        .collect();

    if !filtered.is_empty() {
        *candidates = filtered;
    }
}

fn score_command_candidate(query: &str, candidate: &PaletteCandidate) -> Option<CandidateScore> {
    let id = candidate.id.to_ascii_lowercase();
    let search_text = candidate.search_text().to_ascii_lowercase();

    let id_score = score_id(query, &id);
    let search_text_score = score_search_text(query, &search_text);
    let score = id_score.max(search_text_score);
    if score <= 0 {
        return None;
    }

    Some(CandidateScore {
        score,
        tie_len: id.len(),
    })
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

fn score_search_text(query: &str, search_text: &str) -> i32 {
    if search_text.is_empty() {
        return 0;
    }
    if search_text.starts_with(query) {
        return SCORE_SEARCH_TEXT_PREFIX;
    }
    if search_text.contains(query) {
        return SCORE_SEARCH_TEXT_CONTAINS;
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

fn format_search_texts(
    command_id: &str,
    title: &str,
    args: &[crate::command::ArgSpec],
) -> Vec<PaletteSearchText> {
    let usage = usage_text(args);
    let mut parts = vec![PaletteSearchText::new(title)];
    if !usage.is_empty() {
        parts.push(PaletteSearchText::new(usage));
    }
    parts.push(PaletteSearchText::new(command_id));
    parts
}

#[cfg(test)]
mod tests {
    use crate::app::AppState;
    use crate::command::Command;
    use crate::extension::ExtensionUiSnapshot;
    use crate::input::InputHistoryRecord;
    use crate::palette::{
        PaletteContext, PaletteKind, PaletteOpenPayload, PalettePostAction, PaletteProvider,
        PaletteSubmitEffect, PaletteTabEffect,
    };

    use super::CommandPaletteProvider;

    fn ids(list: &[crate::palette::PaletteCandidate]) -> Vec<String> {
        list.iter().map(|candidate| candidate.id.clone()).collect()
    }

    fn command_list_for_input(
        input: &str,
        search_active: bool,
    ) -> Vec<crate::palette::PaletteCandidate> {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::with_search_active(search_active);
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
            open_payload: None,
        };
        provider.list(&ctx).expect("list should be built")
    }

    fn command_submit_effect(
        input: &str,
        selected_id: &str,
        search_active: bool,
    ) -> PaletteSubmitEffect {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::with_search_active(search_active);
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
            open_payload: None,
        };
        let candidates = provider.list(&ctx).expect("list should be built");
        let selected = candidates
            .iter()
            .find(|candidate| candidate.id == selected_id)
            .expect("selected candidate should exist");
        provider
            .on_submit(&ctx, Some(selected))
            .expect("submit should succeed")
    }

    fn command_tab_effect(input: &str, selected_id: &str, search_active: bool) -> PaletteTabEffect {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::with_search_active(search_active);
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
            open_payload: None,
        };
        let candidates = provider.list(&ctx).expect("list should be built");
        let selected = candidates
            .iter()
            .find(|candidate| candidate.id == selected_id)
            .expect("selected candidate should exist");
        provider
            .on_tab(&ctx, Some(selected))
            .expect("tab should succeed")
    }

    fn assistive_text_for_input(input: &str, search_active: bool) -> Option<String> {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::with_search_active(search_active);
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
            open_payload: None,
        };
        provider.assistive_text(&ctx, None)
    }

    #[test]
    fn list_hides_search_hit_navigation_when_search_is_inactive() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "",
            open_payload: None,
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
        assert!(!list.iter().any(|candidate| candidate.id == "open-palette"));
        assert!(!list.iter().any(|candidate| candidate.id == "submit-search"));
        assert!(!list.iter().any(|candidate| candidate.id == "history-goto"));
    }

    #[test]
    fn list_shows_search_hit_navigation_when_search_is_active() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::with_search_active(true);
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "",
            open_payload: None,
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
    fn non_enum_argument_phase_hides_candidates() {
        let list = command_list_for_input("goto-page ", false);
        assert!(list.is_empty());
    }

    #[test]
    fn enum_argument_phase_lists_values() {
        let list = command_list_for_input("page-layout-spread ", false);
        assert_eq!(ids(&list), vec!["ltr".to_string(), "rtl".to_string()]);
    }

    #[test]
    fn enum_argument_phase_filters_values() {
        let list = command_list_for_input("page-layout-spread r", false);
        assert_eq!(ids(&list), vec!["ltr".to_string(), "rtl".to_string()]);
    }

    #[test]
    fn enum_argument_candidates_keep_definition_order() {
        let list = command_list_for_input("pan ", false);
        assert_eq!(
            ids(&list),
            vec![
                "left".to_string(),
                "right".to_string(),
                "up".to_string(),
                "down".to_string()
            ]
        );
    }

    #[test]
    fn enum_argument_candidates_filter_without_reordering() {
        let list = command_list_for_input("pan t", false);
        assert_eq!(ids(&list), vec!["left".to_string(), "right".to_string()]);

        let list = command_list_for_input("page-layout-spread rt", false);
        assert_eq!(ids(&list), vec!["rtl".to_string()]);
    }

    #[test]
    fn enum_argument_candidates_fall_back_to_full_list_when_no_match() {
        let list = command_list_for_input("pan z", false);
        assert_eq!(
            ids(&list),
            vec![
                "left".to_string(),
                "right".to_string(),
                "up".to_string(),
                "down".to_string()
            ]
        );
    }

    #[test]
    fn trailing_non_enum_argument_phase_hides_enum_candidates() {
        let list = command_list_for_input("pan left ", false);
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

    #[test]
    fn submit_dispatches_optional_only_page_layout_without_reopen() {
        let effect = command_submit_effect("", "page-layout-spread", false);
        assert_eq!(
            effect,
            PaletteSubmitEffect::Dispatch {
                command: Command::SetPageLayout {
                    mode: crate::command::PageLayoutModeArg::Spread,
                    direction: None,
                },
                history_record: Some(InputHistoryRecord::Command(
                    "page-layout-spread".to_string(),
                )),
                next: PalettePostAction::Close,
            }
        );
    }

    #[test]
    fn submit_reopens_for_required_argument_commands() {
        let effect = command_submit_effect("", "zoom", false);
        assert_eq!(
            effect,
            PaletteSubmitEffect::Reopen {
                kind: PaletteKind::Command,
                payload: Some(PaletteOpenPayload::CommandInput("zoom ".to_string())),
            }
        );
    }

    #[test]
    fn submit_dispatches_search_to_open_search_palette() {
        let effect = command_submit_effect("", "search", false);
        assert_eq!(
            effect,
            PaletteSubmitEffect::Dispatch {
                command: Command::OpenSearch,
                history_record: Some(InputHistoryRecord::Command("search".to_string())),
                next: PalettePostAction::Close,
            }
        );
    }

    #[test]
    fn submit_dispatches_typed_command_with_history_record() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "quit",
            open_payload: None,
        };

        let effect = provider
            .on_submit(&ctx, None)
            .expect("typed command submit should succeed");

        assert_eq!(
            effect,
            PaletteSubmitEffect::Dispatch {
                command: Command::Quit,
                history_record: Some(InputHistoryRecord::Command("quit".to_string())),
                next: PalettePostAction::Close,
            }
        );
    }

    #[test]
    fn submit_dispatches_typed_optional_enum_command_without_argument() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "page-layout-spread",
            open_payload: None,
        };

        let effect = provider
            .on_submit(&ctx, None)
            .expect("typed command submit should succeed");

        assert_eq!(
            effect,
            PaletteSubmitEffect::Dispatch {
                command: Command::SetPageLayout {
                    mode: crate::command::PageLayoutModeArg::Spread,
                    direction: None,
                },
                history_record: Some(InputHistoryRecord::Command(
                    "page-layout-spread".to_string(),
                )),
                next: PalettePostAction::Close,
            }
        );
    }

    #[test]
    fn tab_completion_appends_trailing_space_for_required_argument_commands() {
        let effect = command_tab_effect("z", "zoom", false);
        assert_eq!(
            effect,
            PaletteTabEffect::SetInput {
                value: "zoom ".to_string(),
                move_cursor_to_end: true,
            }
        );
    }

    #[test]
    fn tab_completion_appends_trailing_space_for_no_argument_commands() {
        let effect = command_tab_effect("q", "quit", false);
        assert_eq!(
            effect,
            PaletteTabEffect::SetInput {
                value: "quit ".to_string(),
                move_cursor_to_end: true,
            }
        );
    }

    #[test]
    fn tab_completion_replaces_enum_argument_and_appends_space() {
        let effect = command_tab_effect("page-layout-spread r", "rtl", false);
        assert_eq!(
            effect,
            PaletteTabEffect::SetInput {
                value: "page-layout-spread rtl ".to_string(),
                move_cursor_to_end: true,
            }
        );
    }

    #[test]
    fn submit_dispatches_selected_enum_argument_when_result_is_complete() {
        let effect = command_submit_effect("page-layout-spread ", "rtl", false);
        assert_eq!(
            effect,
            PaletteSubmitEffect::Dispatch {
                command: Command::SetPageLayout {
                    mode: crate::command::PageLayoutModeArg::Spread,
                    direction: Some(crate::command::SpreadDirectionArg::Rtl),
                },
                history_record: Some(InputHistoryRecord::Command(
                    "page-layout-spread rtl".to_string(),
                )),
                next: PalettePostAction::Close,
            }
        );
    }

    #[test]
    fn assistive_text_uses_enum_values_for_enum_arguments() {
        assert_eq!(
            assistive_text_for_input("page-layout-spread ", false),
            Some("page-layout-spread [direction] | direction: ltr / rtl".to_string())
        );
    }

    #[test]
    fn assistive_text_uses_integer_label_for_integer_arguments() {
        assert_eq!(
            assistive_text_for_input("goto-page ", false),
            Some("goto-page <page> | page: integer".to_string())
        );
    }

    #[test]
    fn assistive_text_uses_number_label_for_float_arguments() {
        assert_eq!(
            assistive_text_for_input("zoom ", false),
            Some("zoom <value> | value: number".to_string())
        );
    }

    #[test]
    fn assistive_text_shows_title_when_all_arguments_are_complete() {
        assert_eq!(
            assistive_text_for_input("pan left 1 ", false),
            Some("pan <direction> [amount] | Pan".to_string())
        );
    }

    #[test]
    fn assistive_text_shows_title_for_complete_no_argument_command() {
        assert_eq!(
            assistive_text_for_input("quit ", false),
            Some("quit | Quit".to_string())
        );
    }

    #[test]
    fn submit_reopens_when_input_targets_internal_command() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::with_search_active(true);
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "submit-search hello",
            open_payload: None,
        };

        let err = provider
            .on_submit(&ctx, None)
            .expect_err("internal command input should error");
        assert_eq!(
            err.to_string(),
            "invalid argument: submit-search is an internal command and cannot be invoked directly"
        );
    }

    #[test]
    fn submit_errors_when_explicit_input_has_invalid_arguments() {
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app: &app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "first-page hoge",
            open_payload: None,
        };

        let err = provider
            .on_submit(&ctx, None)
            .expect_err("invalid command arguments should error");
        assert_eq!(
            err.to_string(),
            "invalid argument: first-page does not accept arguments"
        );
    }
}
