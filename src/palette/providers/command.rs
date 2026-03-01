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
        PaletteInputMode::FilterCandidates
    }

    fn list(&self, ctx: &PaletteContext<'_>) -> AppResult<Vec<PaletteCandidate>> {
        if has_argument_phase(ctx.input) {
            return Ok(Vec::new());
        }

        let candidates = all_command_specs()
            .into_iter()
            .filter(|spec| can_show_command_spec(spec.id, ctx))
            .map(|spec| PaletteCandidate {
                id: spec.id.to_string(),
                label: spec.id.to_string(),
                detail: Some(format_detail(spec.title, spec.args)),
                payload: PalettePayload::Opaque(spec.id.to_string()),
            })
            .collect();
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

#[cfg(test)]
mod tests {
    use crate::app::AppState;
    use crate::palette::{PaletteContext, PaletteKind, PaletteProvider};

    use super::CommandPaletteProvider;

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
        let provider = CommandPaletteProvider;
        let app = AppState::default();
        let ctx = PaletteContext {
            app: &app,
            kind: PaletteKind::Command,
            input: "goto-page ",
            seed: None,
        };

        let list = provider.list(&ctx).expect("list should be built");
        assert!(list.is_empty());
    }
}
