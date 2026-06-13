use crate::error::{AppError, AppResult};
use crate::extension::ExtensionUiSnapshot;

use super::catalog::{self, Command};
use super::types::{
    CommandAvailability, CommandCondition, CommandExposure, CommandInvocationPolicy,
    CommandInvocationSource, CommandSpec,
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
    pub source: CommandInvocationSource,
}

pub fn find_command_spec(id: &str) -> Option<CommandSpec> {
    catalog::find_command_spec(id)
}

pub fn spec_for_command(command: &Command) -> Option<CommandSpec> {
    find_command_spec(command.id())
}

pub fn is_command_visible_in_palette(spec: CommandSpec, ctx: &CommandConditionContext<'_>) -> bool {
    spec.exposure == CommandExposure::Public
        && spec.invocation == CommandInvocationPolicy::User
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
        CommandInvocationPolicy::User => true,
        CommandInvocationPolicy::KeymapOnly => source == CommandInvocationSource::Keymap,
        CommandInvocationPolicy::InternalOnly => source == CommandInvocationSource::PaletteProvider,
    }
}

fn is_condition_met(condition: CommandCondition, ctx: &CommandConditionContext<'_>) -> bool {
    match condition {
        CommandCondition::SearchActive => ctx.extensions.search_active,
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
        }
    }

    format!("{} is unavailable", spec.id)
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
            source: CommandInvocationSource::CommandPaletteInput,
        };

        let spec = find_command_spec("open-palette").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn command_validation_rejects_search_navigation_when_search_is_inactive() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            extensions: &extensions,
            source: CommandInvocationSource::Keymap,
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
            source: CommandInvocationSource::Keymap,
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
            source: CommandInvocationSource::CommandPaletteInput,
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
            source: CommandInvocationSource::CommandPaletteInput,
        };

        let spec = find_command_spec("close-help").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
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
