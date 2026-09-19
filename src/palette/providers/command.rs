use crate::app::Mode;
use crate::command::all_command_specs;
use crate::command::find_command_spec;
use crate::command::first_token;
use crate::command::is_command_visible_in_palette;
use crate::command::parse_command_text;
use crate::command::parse_invocable_command_text;
use crate::command::{ArgHint, ArgKind, ArgSpec, CommandInvocationSource, CommandPolicyContext};
use crate::condition::RuntimeConditionContext;
use crate::error::AppResult;
use crate::input::InputHistoryRecord;
use crate::input::shortcut::{
    ShortcutKey, format_shortcut_alternatives_tight, format_shortcut_key,
};
use crate::palette::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteKind, PaletteOpenOptions,
    PalettePostAction, PaletteProvider, PaletteRow, PaletteSubmitEffect, PaletteTabEffect,
    PaletteTextPart,
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
                        let command_ctx = post_submit_command_policy_context(ctx);
                        is_command_visible_in_palette(*spec, &command_ctx)
                    })
                    .map(|spec| command_candidate(spec.id, spec.args, spec.title))
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

        let command_ctx = post_submit_command_policy_context(ctx);
        let mut deferred_error = None;
        if !input.is_empty() {
            match parse_invocable_command_text(input, &command_ctx) {
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
            && let Some(spec) = find_command_spec(candidate.id().as_str())
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
                    options: PaletteOpenOptions::input(format!("{} ", spec.id)),
                });
            }
        }

        if let Some(err) = deferred_error {
            return Err(err);
        }

        // 3. Fallback: reopen preserving current input.
        Ok(PaletteSubmitEffect::Reopen {
            kind: self.kind(),
            options: PaletteOpenOptions::input(ctx.input.to_string()),
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

        let value = candidate.id().as_str().to_string();

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

fn post_submit_command_policy_context<'a>(ctx: &'a PaletteContext<'a>) -> CommandPolicyContext<'a> {
    CommandPolicyContext {
        source: CommandInvocationSource::CommandPaletteInput,
        runtime: RuntimeConditionContext::new(Mode::Normal, None, ctx.extensions),
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

fn format_label(command_id: &str, args: &[crate::command::ArgSpec]) -> Vec<PaletteTextPart> {
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

fn command_candidate(
    command_id: &str,
    args: &[crate::command::ArgSpec],
    title: &str,
) -> PaletteCandidate {
    PaletteRow::new(command_id)
        .label_matchable_parts(format_label(command_id, args))
        .detail_matchable_text(title)
        .into_candidate()
}

fn enum_value_candidate(value: &str) -> PaletteCandidate {
    PaletteRow::new(value)
        .label_matchable_text(value)
        .into_candidate()
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
        .then_some(candidate.id().as_str())
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
    let command_ctx = post_submit_command_policy_context(ctx);
    match parse_invocable_command_text(synthesized_trimmed, &command_ctx) {
        Ok(command) => Ok(Some(PaletteSubmitEffect::Dispatch {
            command,
            history_record: Some(InputHistoryRecord::Command(synthesized_trimmed.to_string())),
            next: PalettePostAction::Close,
        })),
        Err(_) => Ok(Some(PaletteSubmitEffect::Reopen {
            kind: PaletteKind::Command,
            options: PaletteOpenOptions::input(synthesized),
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
                .then_with(|| {
                    left_candidate
                        .id()
                        .as_str()
                        .cmp(right_candidate.id().as_str())
                })
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
    let id = candidate.id().as_str().to_ascii_lowercase();
    let search_text = candidate.match_text().to_ascii_lowercase();

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

#[cfg(test)]
mod tests {
    use crate::app::AppState;
    use crate::app::Mode;
    use crate::command::Command;
    use crate::extension::ExtensionUiSnapshot;
    use crate::input::{InputHistoryRecord, InputHistorySnapshot};
    use crate::palette::{
        PaletteAppSnapshot, PaletteContext, PaletteKind, PaletteOpenOptions, PalettePostAction,
        PaletteProvider, PaletteRegistry, PaletteSessionController, PaletteSubmitEffect,
        PaletteTabEffect,
    };

    use super::{CommandPaletteProvider, post_submit_command_policy_context};

    fn ids(list: &[crate::palette::PaletteCandidate]) -> Vec<String> {
        list.iter()
            .map(|candidate| candidate.id().as_str().to_string())
            .collect()
    }

    fn history_snapshot(entries: &[&str]) -> InputHistorySnapshot {
        InputHistorySnapshot::from_entries(entries)
    }

    fn label_text(item: &crate::palette::PaletteItemView) -> String {
        item.label
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>()
    }

    fn extension_snapshot(search_active: bool) -> ExtensionUiSnapshot {
        ExtensionUiSnapshot {
            search: crate::search::SearchUiSnapshot {
                active: search_active,
                ..Default::default()
            },
            ..ExtensionUiSnapshot::default()
        }
    }

    fn command_list_for_input(
        input: &str,
        search_active: bool,
    ) -> Vec<crate::palette::PaletteCandidate> {
        let provider = CommandPaletteProvider;
        let app = PaletteAppSnapshot::default();
        let extensions = extension_snapshot(search_active);
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
        };
        provider.list(&ctx).expect("list should be built")
    }

    #[test]
    fn command_policy_uses_post_submit_normal_context() {
        let app = PaletteAppSnapshot {
            mode: Mode::Palette,
            ..PaletteAppSnapshot::default()
        };
        let extensions = ExtensionUiSnapshot::default();
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input: "",
        };

        let command_ctx = post_submit_command_policy_context(&ctx);
        assert_eq!(command_ctx.runtime.mode, Mode::Normal);
        assert_eq!(command_ctx.runtime.active_palette, None);
    }

    fn command_submit_effect(
        input: &str,
        selected_id: &str,
        search_active: bool,
    ) -> PaletteSubmitEffect {
        let provider = CommandPaletteProvider;
        let app = PaletteAppSnapshot::default();
        let extensions = extension_snapshot(search_active);
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
        };
        let candidates = provider.list(&ctx).expect("list should be built");
        let selected = candidates
            .iter()
            .find(|candidate| candidate.id().as_str() == selected_id)
            .expect("selected candidate should exist");
        provider
            .on_submit(&ctx, Some(selected))
            .expect("submit should succeed")
    }

    fn command_tab_effect(input: &str, selected_id: &str, search_active: bool) -> PaletteTabEffect {
        let provider = CommandPaletteProvider;
        let app = PaletteAppSnapshot::default();
        let extensions = extension_snapshot(search_active);
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
        };
        let candidates = provider.list(&ctx).expect("list should be built");
        let selected = candidates
            .iter()
            .find(|candidate| candidate.id().as_str() == selected_id)
            .expect("selected candidate should exist");
        provider
            .on_tab(&ctx, Some(selected))
            .expect("tab should succeed")
    }

    fn assistive_text_for_input(input: &str, search_active: bool) -> Option<String> {
        let provider = CommandPaletteProvider;
        let app = PaletteAppSnapshot::default();
        let extensions = extension_snapshot(search_active);
        let ctx = PaletteContext {
            app,
            extensions: &extensions,
            kind: PaletteKind::Command,
            input,
        };
        provider.assistive_text(&ctx, None)
    }

    #[test]
    fn session_selection_resets_when_command_input_filters_candidates() {
        let registry = PaletteRegistry::default();
        let mut session = PaletteSessionController::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        session
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Command,
                PaletteOpenOptions::default(),
                None,
            )
            .expect("command palette should open");

        let initial_view = session.view().expect("palette should be visible");
        assert!(initial_view.items.len() > 1);

        assert!(session.select_next_item());
        let selected_view = session.view().expect("palette should be visible");
        assert_eq!(selected_view.selected_idx, Some(1));

        session
            .insert_text(&registry, &app, &extensions, "p")
            .expect("typing should succeed");
        let filtered_view = session.view().expect("palette should be visible");
        assert_eq!(filtered_view.selected_idx, Some(0));
        assert_eq!(filtered_view.input, "p");
    }

    #[test]
    fn session_recalls_command_input_history_and_restores_draft() {
        let registry = PaletteRegistry::default();
        let mut session = PaletteSessionController::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        session
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Command,
                PaletteOpenOptions::default(),
                Some(history_snapshot(&["next-page", "prev-page"])),
            )
            .expect("command palette should open");

        assert!(session.select_next_item());

        session
            .insert_text(&registry, &app, &extensions, "z")
            .expect("typing should succeed");

        session
            .recall_history(&registry, &app, &extensions, true)
            .expect("history recall should succeed");
        let older_view = session.view().expect("palette should be visible");
        assert_eq!(older_view.input, "prev-page");
        assert_eq!(older_view.selected_idx, Some(0));
        assert_eq!(
            older_view.items.first().map(label_text),
            Some("prev-page".to_string())
        );

        session
            .recall_history(&registry, &app, &extensions, true)
            .expect("history recall should succeed");
        let oldest_view = session.view().expect("palette should be visible");
        assert_eq!(oldest_view.input, "next-page");
        assert_eq!(oldest_view.selected_idx, Some(0));
        assert_eq!(
            oldest_view.items.first().map(label_text),
            Some("next-page".to_string())
        );

        session
            .recall_history(&registry, &app, &extensions, false)
            .expect("history recall should succeed");
        let newer_view = session.view().expect("palette should be visible");
        assert_eq!(newer_view.input, "prev-page");

        session
            .recall_history(&registry, &app, &extensions, false)
            .expect("draft restore should succeed");
        let restored_view = session.view().expect("palette should be visible");
        assert_eq!(restored_view.input, "z");
    }

    #[test]
    fn tab_completion_resets_command_history_navigation_state() {
        let registry = PaletteRegistry::default();
        let mut session = PaletteSessionController::default();
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();

        session
            .open(
                &registry,
                &app,
                &extensions,
                PaletteKind::Command,
                PaletteOpenOptions::default(),
                Some(history_snapshot(&["next-page", "prev-page"])),
            )
            .expect("command palette should open");

        session
            .recall_history(&registry, &app, &extensions, true)
            .expect("history recall should succeed");
        session
            .complete(&registry, &app, &extensions)
            .expect("tab completion should succeed");

        let completed_view = session.view().expect("palette should be visible");
        assert_eq!(completed_view.input, "prev-page ");

        session
            .recall_history(&registry, &app, &extensions, false)
            .expect("down after tab should be handled");

        let after_down_view = session.view().expect("palette should be visible");
        assert_eq!(after_down_view.input, "prev-page ");
    }

    #[test]
    fn list_exposes_search_navigation_only_while_search_is_active() {
        for (case, search_active) in [("inactive", false), ("active", true)] {
            let list = command_list_for_input("", search_active);
            for id in ["next-search-hit", "search-results", "prev-search-hit"] {
                assert_eq!(
                    list.iter().any(|candidate| candidate.id().as_str() == id),
                    search_active,
                    "{case}: {id}"
                );
            }
            for id in [
                "open-palette",
                "submit-search",
                "search-goto",
                "history-goto",
            ] {
                assert!(
                    !list.iter().any(|candidate| candidate.id().as_str() == id),
                    "{case}: internal command {id}"
                );
            }
        }
    }

    #[test]
    fn enum_candidates_follow_argument_phase_filtering_and_definition_order() {
        for (case, input, expected) in [
            ("non-enum", "goto-page ", &[][..]),
            ("first enum", "layout-spread ", &["ltr", "rtl"]),
            ("second enum", "layout-spread ltr ", &["paired", "cover"]),
            ("fuzzy enum", "layout-spread r", &["ltr", "rtl"]),
            ("definition order", "pan ", &["left", "right", "up", "down"]),
            ("filtered order", "pan t", &["left", "right"]),
            ("filtered singleton", "layout-spread rt", &["rtl"]),
            ("second enum filtered", "layout-spread rtl p", &["paired"]),
            (
                "no-match fallback",
                "pan z",
                &["left", "right", "up", "down"],
            ),
            ("trailing non-enum", "pan left ", &[][..]),
        ] {
            assert_eq!(
                ids(&command_list_for_input(input, false)),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn scoring_orders_exact_prefix_acronym_and_tied_matches() {
        for (case, query, search_active, expected_order) in [
            ("exact", "quit", false, &["quit"][..]),
            (
                "prefix before contains",
                "search",
                true,
                &["search", "next-search-hit"],
            ),
            ("hyphen acronym", "nsh", true, &["next-search-hit"]),
            (
                "shorter then lexicographic",
                "page",
                false,
                &["goto-page", "last-page", "next-page", "prev-page"],
            ),
        ] {
            let actual = ids(&command_list_for_input(query, search_active));
            let positions = expected_order
                .iter()
                .map(|expected| {
                    actual
                        .iter()
                        .position(|id| id == expected)
                        .unwrap_or_else(|| panic!("{case}: missing {expected}"))
                })
                .collect::<Vec<_>>();
            assert!(
                positions.windows(2).all(|pair| pair[0] < pair[1]),
                "{case}: {actual:?}"
            );
            assert_eq!(positions.first(), Some(&0), "{case}: {actual:?}");
        }
    }

    #[test]
    fn submit_selected_candidates_dispatches_or_reopens_for_missing_required_arguments() {
        for (case, input, selected, expected) in [
            (
                "optional arguments omitted",
                "",
                "layout-spread",
                PaletteSubmitEffect::Dispatch {
                    command: Command::PageLayoutSpread {
                        direction: None,
                        cover_policy: None,
                    },
                    history_record: Some(InputHistoryRecord::Command("layout-spread".to_string())),
                    next: PalettePostAction::Close,
                },
            ),
            (
                "required argument missing",
                "",
                "zoom",
                PaletteSubmitEffect::Reopen {
                    kind: PaletteKind::Command,
                    options: PaletteOpenOptions::input("zoom "),
                },
            ),
            (
                "search command",
                "",
                "search",
                PaletteSubmitEffect::Dispatch {
                    command: Command::OpenSearch,
                    history_record: Some(InputHistoryRecord::Command("search".to_string())),
                    next: PalettePostAction::Close,
                },
            ),
            (
                "first enum selected",
                "layout-spread ",
                "rtl",
                PaletteSubmitEffect::Dispatch {
                    command: Command::PageLayoutSpread {
                        direction: Some(crate::command::SpreadDirectionArg::Rtl),
                        cover_policy: None,
                    },
                    history_record: Some(InputHistoryRecord::Command(
                        "layout-spread rtl".to_string(),
                    )),
                    next: PalettePostAction::Close,
                },
            ),
            (
                "second enum selected",
                "layout-spread rtl ",
                "cover",
                PaletteSubmitEffect::Dispatch {
                    command: Command::PageLayoutSpread {
                        direction: Some(crate::command::SpreadDirectionArg::Rtl),
                        cover_policy: Some(crate::command::SpreadCoverPolicyArg::Cover),
                    },
                    history_record: Some(InputHistoryRecord::Command(
                        "layout-spread rtl cover".to_string(),
                    )),
                    next: PalettePostAction::Close,
                },
            ),
        ] {
            assert_eq!(
                command_submit_effect(input, selected, false),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn submit_dispatches_complete_typed_commands_with_history() {
        for (case, input, expected_command) in [
            ("no arguments", "quit", Command::Quit),
            (
                "optional enum omitted",
                "layout-spread",
                Command::PageLayoutSpread {
                    direction: None,
                    cover_policy: None,
                },
            ),
        ] {
            let provider = CommandPaletteProvider;
            let app = PaletteAppSnapshot::default();
            let extensions = ExtensionUiSnapshot::default();
            let ctx = PaletteContext {
                app,
                extensions: &extensions,
                kind: PaletteKind::Command,
                input,
            };
            assert_eq!(
                provider
                    .on_submit(&ctx, None)
                    .expect("typed command submit should succeed"),
                PaletteSubmitEffect::Dispatch {
                    command: expected_command,
                    history_record: Some(InputHistoryRecord::Command(input.to_string())),
                    next: PalettePostAction::Close,
                },
                "{case}"
            );
        }
    }

    #[test]
    fn tab_completion_replaces_the_selected_command_or_enum_value() {
        for (case, input, selected, expected) in [
            ("required argument", "z", "zoom", "zoom "),
            ("no argument", "q", "quit", "quit "),
            (
                "enum argument",
                "layout-spread r",
                "rtl",
                "layout-spread rtl ",
            ),
        ] {
            assert_eq!(
                command_tab_effect(input, selected, false),
                PaletteTabEffect::SetInput {
                    value: expected.to_string(),
                    move_cursor_to_end: true,
                },
                "{case}"
            );
        }
    }

    #[test]
    fn assistive_text_describes_the_current_argument_or_completed_command() {
        for (case, input, expected) in [
            (
                "first enum",
                "layout-spread ",
                "layout-spread [direction] [cover-policy] | direction: ltr / rtl",
            ),
            (
                "second enum",
                "layout-spread rtl ",
                "layout-spread [direction] [cover-policy] | cover-policy: paired / cover",
            ),
            ("integer", "goto-page ", "goto-page <page> | page: integer"),
            ("number", "zoom ", "zoom <ratio> | ratio: number"),
            (
                "arguments complete",
                "pan left 1 ",
                "pan <direction> [amount] | Pan",
            ),
            ("no arguments", "quit ", "quit | Quit"),
        ] {
            assert_eq!(
                assistive_text_for_input(input, false),
                Some(expected.to_string()),
                "{case}"
            );
        }
    }

    #[test]
    fn submit_rejects_internal_commands_and_invalid_arguments() {
        for (case, input, search_active, expected) in [
            (
                "internal command",
                "submit-search hello",
                true,
                "invalid argument: submit-search is an internal command and cannot be invoked directly",
            ),
            (
                "invalid arguments",
                "first-page hoge",
                false,
                "invalid argument: first-page does not accept arguments",
            ),
        ] {
            let provider = CommandPaletteProvider;
            let app = PaletteAppSnapshot::default();
            let extensions = extension_snapshot(search_active);
            let ctx = PaletteContext {
                app,
                extensions: &extensions,
                kind: PaletteKind::Command,
                input,
            };
            let err = provider
                .on_submit(&ctx, None)
                .expect_err("invalid command input should error");
            assert_eq!(err.to_string(), expected, "{case}");
        }
    }
}
