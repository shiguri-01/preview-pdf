use crate::app::Mode;
use crate::error::{AppError, AppResult};
use crate::extension::ExtensionUiSnapshot;

use super::catalog::{self, Command};
use super::types::{
    CommandAvailability, CommandCondition, CommandExposure, CommandInvocationPolicy,
    CommandInvocationSource, CommandRole, CommandSpec, CommandTargetRequirement,
};

const NO_CONDITIONS: [CommandCondition; 0] = [];

pub fn command_registry() -> &'static [CommandSpec] {
    catalog::command_registry()
}

pub fn all_command_specs() -> Vec<CommandSpec> {
    command_registry().to_vec()
}

pub struct CommandConditionContext<'a> {
    pub extensions: &'a ExtensionUiSnapshot,
    pub mode: Mode,
    pub source: CommandInvocationSource,
    pub active_palette: bool,
    pub focused_text_input: bool,
}

pub fn find_command_spec(id: &str) -> Option<CommandSpec> {
    catalog::find_command_spec(id)
}

pub fn spec_for_command(command: &Command) -> Option<CommandSpec> {
    find_command_spec(command.id())
}

pub fn is_command_visible_in_palette(spec: CommandSpec, ctx: &CommandConditionContext<'_>) -> bool {
    spec.role == CommandRole::UserIntent
        && spec.exposure == CommandExposure::Public
        && is_invocation_source_allowed(spec, ctx.source)
        && is_command_available(spec, ctx)
}

pub fn validate_command_id_for_source(
    id: &str,
    ctx: &CommandConditionContext<'_>,
) -> AppResult<()> {
    let Some(spec) = find_command_spec(id) else {
        return Err(AppError::invalid_argument("unknown command id"));
    };
    validate_command_spec_for_source(spec, ctx)
}

pub fn validate_command_for_source(
    command: &Command,
    ctx: &CommandConditionContext<'_>,
) -> AppResult<()> {
    let Some(spec) = spec_for_command(command) else {
        return Err(AppError::unsupported(
            "command spec should exist for typed command",
        ));
    };
    validate_command_spec_for_source(spec, ctx)
}

pub fn validate_command_invocation_for_source(
    command: &Command,
    source: CommandInvocationSource,
) -> AppResult<()> {
    let Some(spec) = spec_for_command(command) else {
        return Err(AppError::unsupported(
            "command spec should exist for typed command",
        ));
    };
    validate_command_spec_invocation_for_source(spec, source)
}

pub fn validate_command_id_invocation_for_source(
    id: &str,
    source: CommandInvocationSource,
) -> AppResult<()> {
    let Some(spec) = find_command_spec(id) else {
        return Err(AppError::invalid_argument("unknown command id"));
    };
    validate_command_spec_invocation_for_source(spec, source)
}

pub fn rejection_message_for_command(
    command: &Command,
    ctx: &CommandConditionContext<'_>,
) -> Option<String> {
    validate_command_for_source(command, ctx)
        .err()
        .map(app_error_message)
}

fn validate_command_spec_for_source(
    spec: CommandSpec,
    ctx: &CommandConditionContext<'_>,
) -> AppResult<()> {
    validate_command_spec_invocation_for_source(spec, ctx.source)?;

    if !is_target_available(spec.target, ctx) {
        return Err(AppError::invalid_argument(target_unavailable_message(spec)));
    }

    if !is_command_available(spec, ctx) {
        return Err(AppError::invalid_argument(unavailable_message(spec, ctx)));
    }

    Ok(())
}

fn validate_command_spec_invocation_for_source(
    spec: CommandSpec,
    source: CommandInvocationSource,
) -> AppResult<()> {
    if is_invocation_source_allowed(spec, source) {
        return Ok(());
    }

    Err(AppError::invalid_argument(format!(
        "{} is an internal command and cannot be invoked directly",
        spec.id
    )))
}

fn is_command_available(spec: CommandSpec, ctx: &CommandConditionContext<'_>) -> bool {
    match spec.availability {
        CommandAvailability::Always => true,
        CommandAvailability::AllOf(conditions) => conditions
            .iter()
            .copied()
            .all(|condition| is_condition_met(condition, ctx)),
    }
}

fn is_invocation_source_allowed(spec: CommandSpec, source: CommandInvocationSource) -> bool {
    match spec.invocation {
        CommandInvocationPolicy::User => {
            matches!(
                source,
                CommandInvocationSource::Keymap | CommandInvocationSource::CommandPaletteInput
            )
        }
        CommandInvocationPolicy::KeymapOnly => source == CommandInvocationSource::Keymap,
        CommandInvocationPolicy::Interaction => matches!(
            source,
            CommandInvocationSource::Keymap | CommandInvocationSource::Interaction
        ),
        CommandInvocationPolicy::InternalOnly => source == CommandInvocationSource::Interaction,
    }
}

fn is_target_available(
    target: CommandTargetRequirement,
    ctx: &CommandConditionContext<'_>,
) -> bool {
    match target {
        CommandTargetRequirement::App => true,
        CommandTargetRequirement::ActivePalette => ctx.active_palette,
        CommandTargetRequirement::FocusedTextInput => ctx.focused_text_input,
        CommandTargetRequirement::ActiveHelp => ctx.mode == Mode::Help,
    }
}

fn is_condition_met(condition: CommandCondition, ctx: &CommandConditionContext<'_>) -> bool {
    match condition {
        CommandCondition::SearchActive => ctx.extensions.search_active,
        CommandCondition::HelpMode => ctx.mode == Mode::Help,
    }
}

fn unavailable_message(spec: CommandSpec, ctx: &CommandConditionContext<'_>) -> String {
    let unmet_conditions = match spec.availability {
        CommandAvailability::Always => &NO_CONDITIONS[..],
        CommandAvailability::AllOf(conditions) => conditions,
    };

    for condition in unmet_conditions {
        match condition {
            CommandCondition::SearchActive if !ctx.extensions.search_active => {
                return format!("{} is unavailable while search is inactive", spec.id);
            }
            CommandCondition::SearchActive => {}
            CommandCondition::HelpMode if ctx.mode != Mode::Help => {
                return format!("{} is unavailable outside help", spec.id);
            }
            CommandCondition::HelpMode => {}
        }
    }

    format!("{} is unavailable", spec.id)
}

fn target_unavailable_message(spec: CommandSpec) -> String {
    match spec.target {
        CommandTargetRequirement::App => format!("{} has no target", spec.id),
        CommandTargetRequirement::ActivePalette => {
            format!("{} is unavailable without an active palette", spec.id)
        }
        CommandTargetRequirement::FocusedTextInput => {
            format!("{} is unavailable without a focused text input", spec.id)
        }
        CommandTargetRequirement::ActiveHelp => {
            format!("{} is unavailable outside help", spec.id)
        }
    }
}

fn app_error_message(err: AppError) -> String {
    match err {
        AppError::InvalidArgument(message)
        | AppError::Unsupported(message)
        | AppError::Unimplemented(message) => message,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::app::Mode;
    use crate::extension::ExtensionUiSnapshot;

    use super::{
        CommandConditionContext, command_registry, find_command_spec,
        is_command_visible_in_palette, validate_command_for_source, validate_command_id_for_source,
        validate_command_invocation_for_source,
    };
    use crate::command::{Command, CommandInvocationPolicy, CommandInvocationSource};

    #[test]
    fn command_specs_have_unique_ids() {
        let mut seen = HashSet::new();

        for spec in command_registry() {
            assert!(seen.insert(spec.id), "duplicate command id: {}", spec.id);
        }
    }

    #[test]
    fn find_command_spec_returns_metadata_for_internal_command() {
        let spec = find_command_spec("submit-search").expect("spec should exist");
        assert_eq!(spec.invocation, CommandInvocationPolicy::InternalOnly);
    }

    #[test]
    fn palette_visibility_hides_internal_commands() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Normal,
            source: CommandInvocationSource::CommandPaletteInput,
            active_palette: true,
            focused_text_input: true,
        };

        let spec = find_command_spec("open-palette").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn command_validation_rejects_search_navigation_when_search_is_inactive() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Normal,
            source: CommandInvocationSource::Keymap,
            active_palette: false,
            focused_text_input: false,
        };

        let err = validate_command_id_for_source("next-search-hit", &ctx)
            .expect_err("command should be unavailable");
        assert!(err.to_string().contains("search is inactive"));

        let err = validate_command_id_for_source("search-results", &ctx)
            .expect_err("command should be unavailable");
        assert!(err.to_string().contains("search is inactive"));
    }

    #[test]
    fn keymap_only_command_is_allowed_from_keymap_but_not_palette_input() {
        let extensions = ExtensionUiSnapshot::default();
        let keymap_ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Normal,
            source: CommandInvocationSource::Keymap,
            active_palette: false,
            focused_text_input: false,
        };
        validate_command_for_source(
            &Command::OpenPalette {
                kind: crate::palette::PaletteKind::Command,
                payload: None,
            },
            &keymap_ctx,
        )
        .expect("keymap should be allowed");

        let palette_input_ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Normal,
            source: CommandInvocationSource::CommandPaletteInput,
            active_palette: true,
            focused_text_input: true,
        };
        let err = validate_command_id_for_source("open-palette", &palette_input_ctx)
            .expect_err("command palette input should be rejected");
        assert!(err.to_string().contains("internal command"));
    }

    #[test]
    fn keymap_only_close_help_is_hidden_from_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Normal,
            source: CommandInvocationSource::CommandPaletteInput,
            active_palette: true,
            focused_text_input: true,
        };

        let spec = find_command_spec("close-help").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn help_scroll_commands_are_only_available_in_help_mode() {
        let extensions = ExtensionUiSnapshot::default();
        let normal_ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Normal,
            source: CommandInvocationSource::Keymap,
            active_palette: false,
            focused_text_input: false,
        };
        let err = validate_command_id_for_source("help-scroll-down", &normal_ctx)
            .expect_err("help scroll should be unavailable outside help");
        assert!(err.to_string().contains("outside help"));

        let help_ctx = CommandConditionContext {
            extensions: &extensions,
            mode: Mode::Help,
            source: CommandInvocationSource::Keymap,
            active_palette: false,
            focused_text_input: false,
        };
        validate_command_id_for_source("help-scroll-down", &help_ctx)
            .expect("help scroll down should be available in help");
        validate_command_id_for_source("help-scroll-up", &help_ctx)
            .expect("help scroll up should be available in help");
    }

    #[test]
    fn invocation_validation_ignores_runtime_availability() {
        validate_command_invocation_for_source(
            &Command::NextSearchHit,
            CommandInvocationSource::Keymap,
        )
        .expect("keymap config may bind state-dependent commands");
    }
}
