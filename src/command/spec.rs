use crate::app::Mode;
use crate::condition::{
    RuntimeCondition, RuntimeConditionContext, evaluate_condition, first_unmet_condition,
};
use crate::error::{AppError, AppResult};

use super::catalog::{self, Command};
use super::types::{
    CommandExposure, CommandInvocationPolicy, CommandInvocationSource, CommandRole, CommandSpec,
    CommandTargetRequirement,
};

pub fn command_registry() -> &'static [CommandSpec] {
    catalog::command_registry()
}

pub fn all_command_specs() -> Vec<CommandSpec> {
    command_registry().to_vec()
}

#[derive(Debug, Clone, Copy)]
pub struct CommandPolicyContext<'a> {
    pub source: CommandInvocationSource,
    pub runtime: RuntimeConditionContext<'a>,
}

pub fn find_command_spec(id: &str) -> Option<CommandSpec> {
    catalog::find_command_spec(id)
}

pub fn spec_for_command(command: &Command) -> Option<CommandSpec> {
    find_command_spec(command.id())
}

pub fn is_command_visible_in_palette(spec: CommandSpec, ctx: &CommandPolicyContext<'_>) -> bool {
    spec.role == CommandRole::UserIntent
        && spec.exposure == CommandExposure::Public
        && is_invocation_source_allowed(spec, ctx.source)
        && is_target_available(spec.target, ctx)
        && is_command_enabled(spec, ctx)
}

pub fn validate_command_id_for_policy(id: &str, ctx: &CommandPolicyContext<'_>) -> AppResult<()> {
    let Some(spec) = find_command_spec(id) else {
        return Err(AppError::invalid_argument("unknown command id"));
    };
    validate_command_spec_for_policy(spec, ctx)
}

pub fn validate_command_for_policy(
    command: &Command,
    ctx: &CommandPolicyContext<'_>,
) -> AppResult<()> {
    let Some(spec) = spec_for_command(command) else {
        return Err(AppError::unsupported(
            "command spec should exist for typed command",
        ));
    };
    validate_command_spec_for_policy(spec, ctx)
}

pub fn rejection_message_for_command(
    command: &Command,
    ctx: &CommandPolicyContext<'_>,
) -> Option<String> {
    validate_command_for_policy(command, ctx)
        .err()
        .map(app_error_message)
}

fn validate_command_spec_for_policy(
    spec: CommandSpec,
    ctx: &CommandPolicyContext<'_>,
) -> AppResult<()> {
    validate_command_spec_invocation_for_source(spec, ctx.source)?;

    if !is_target_available(spec.target, ctx) {
        return Err(AppError::invalid_argument(target_unavailable_message(spec)));
    }

    if !is_command_enabled(spec, ctx) {
        return Err(AppError::invalid_argument(disabled_message(spec, ctx)));
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

fn is_command_enabled(spec: CommandSpec, ctx: &CommandPolicyContext<'_>) -> bool {
    evaluate_condition(spec.enabled_when, &ctx.runtime)
}

fn is_invocation_source_allowed(spec: CommandSpec, source: CommandInvocationSource) -> bool {
    match spec.invocation {
        CommandInvocationPolicy::User => {
            matches!(
                source,
                CommandInvocationSource::Binding | CommandInvocationSource::CommandPaletteInput
            )
        }
        CommandInvocationPolicy::BindingOnly => source == CommandInvocationSource::Binding,
        CommandInvocationPolicy::InternalOnly => source == CommandInvocationSource::Internal,
    }
}

fn is_target_available(target: CommandTargetRequirement, ctx: &CommandPolicyContext<'_>) -> bool {
    match target {
        CommandTargetRequirement::App => true,
        CommandTargetRequirement::ActivePalette => ctx.runtime.active_palette.is_some(),
        CommandTargetRequirement::ActiveHelp => ctx.runtime.mode == Mode::Help,
    }
}

fn disabled_message(spec: CommandSpec, ctx: &CommandPolicyContext<'_>) -> String {
    if let Some(condition) = first_unmet_condition(spec.enabled_when, &ctx.runtime) {
        return condition_unavailable_message(spec.id, condition);
    }

    format!("{} is unavailable", spec.id)
}

fn condition_unavailable_message(id: &str, condition: RuntimeCondition) -> String {
    match condition {
        RuntimeCondition::ModeIs(Mode::Normal) => {
            format!("{id} is unavailable outside normal mode")
        }
        RuntimeCondition::ModeIs(Mode::Palette) => {
            format!("{id} is unavailable outside palette mode")
        }
        RuntimeCondition::ModeIs(Mode::Help) | RuntimeCondition::HelpIsOpen => {
            format!("{id} is unavailable outside help")
        }
        RuntimeCondition::ModeIsNot(Mode::Normal) => {
            format!("{id} is unavailable in normal mode")
        }
        RuntimeCondition::ModeIsNot(Mode::Palette) => {
            format!("{id} is unavailable in palette mode")
        }
        RuntimeCondition::ModeIsNot(Mode::Help) | RuntimeCondition::HelpIsClosed => {
            format!("{id} is unavailable while help is closed")
        }
        RuntimeCondition::SearchIsActive => {
            format!("{id} is unavailable while search is inactive")
        }
        RuntimeCondition::SearchIsInactive => {
            format!("{id} is unavailable while search is active")
        }
        RuntimeCondition::PaletteIsOpen => {
            format!("{id} is unavailable without an active palette")
        }
        RuntimeCondition::PaletteIsClosed => {
            format!("{id} is unavailable while a palette is active")
        }
        RuntimeCondition::PaletteKindIs(kind) => {
            format!("{id} is unavailable outside the {} palette", kind.id())
        }
        RuntimeCondition::PaletteInputHistoryIsAvailable => {
            format!("{id} is unavailable without palette input history")
        }
        RuntimeCondition::PaletteInputHistoryIsUnavailable => {
            format!("{id} is unavailable while palette input history is available")
        }
    }
}

fn target_unavailable_message(spec: CommandSpec) -> String {
    match spec.target {
        CommandTargetRequirement::App => format!("{} has no target", spec.id),
        CommandTargetRequirement::ActivePalette => {
            format!("{} is unavailable without an active palette", spec.id)
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
    use crate::condition::RuntimeConditionContext;
    use crate::extension::ExtensionUiSnapshot;
    use crate::palette::PaletteKind;

    use super::{
        CommandPolicyContext, command_registry, find_command_spec, is_command_visible_in_palette,
        validate_command_for_policy, validate_command_id_for_policy,
    };
    use crate::command::types::{CommandRole, CommandTargetRequirement};
    use crate::command::{
        Command, CommandExposure, CommandInvocationPolicy, CommandInvocationSource, CommandSpec,
    };
    use crate::condition::ConditionExpr;

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
        let ctx = policy_context(
            CommandInvocationSource::CommandPaletteInput,
            Mode::Normal,
            Some(PaletteKind::Command),
            &extensions,
        );

        let spec = find_command_spec("open-palette").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn command_validation_rejects_search_navigation_when_search_is_inactive() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Normal,
            None,
            &extensions,
        );

        let err = validate_command_id_for_policy("next-search-hit", &ctx)
            .expect_err("command should be unavailable");
        assert!(err.to_string().contains("search is inactive"));

        let err = validate_command_id_for_policy("search-results", &ctx)
            .expect_err("command should be unavailable");
        assert!(err.to_string().contains("search is inactive"));
    }

    #[test]
    fn binding_only_command_is_allowed_from_binding_but_not_palette_input() {
        let extensions = ExtensionUiSnapshot::default();
        let keymap_ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Normal,
            None,
            &extensions,
        );
        validate_command_for_policy(
            &Command::OpenPalette {
                kind: crate::palette::PaletteKind::Command,
                payload: None,
            },
            &keymap_ctx,
        )
        .expect("binding should be allowed");

        let palette_input_ctx = policy_context(
            CommandInvocationSource::CommandPaletteInput,
            Mode::Normal,
            Some(PaletteKind::Command),
            &extensions,
        );
        let err = validate_command_id_for_policy("open-palette", &palette_input_ctx)
            .expect_err("command palette input should be rejected");
        assert!(err.to_string().contains("internal command"));
    }

    #[test]
    fn binding_only_close_help_is_hidden_from_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = policy_context(
            CommandInvocationSource::CommandPaletteInput,
            Mode::Normal,
            Some(PaletteKind::Command),
            &extensions,
        );

        let spec = find_command_spec("close-help").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn palette_visibility_requires_command_target() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = policy_context(
            CommandInvocationSource::CommandPaletteInput,
            Mode::Normal,
            None,
            &extensions,
        );

        let spec = CommandSpec {
            id: "test-palette-target",
            title: "Test Palette Target",
            args: &[],
            role: CommandRole::UserIntent,
            exposure: CommandExposure::Public,
            invocation: CommandInvocationPolicy::User,
            target: CommandTargetRequirement::ActivePalette,
            enabled_when: ConditionExpr::Always,
        };
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn help_scroll_commands_are_only_available_in_help_mode() {
        let extensions = ExtensionUiSnapshot::default();
        let normal_ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Normal,
            None,
            &extensions,
        );
        let err = validate_command_id_for_policy("help-scroll-down", &normal_ctx)
            .expect_err("help scroll should be unavailable outside help");
        assert!(err.to_string().contains("outside help"));

        let help_ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Help,
            None,
            &extensions,
        );
        validate_command_id_for_policy("help-scroll-down", &help_ctx)
            .expect("help scroll down should be available in help");
        validate_command_id_for_policy("help-scroll-up", &help_ctx)
            .expect("help scroll up should be available in help");
    }

    #[test]
    fn binding_only_commands_reject_direct_command_input() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = policy_context(
            CommandInvocationSource::CommandPaletteInput,
            Mode::Help,
            None,
            &extensions,
        );

        let err = validate_command_id_for_policy("help-scroll-down", &ctx)
            .expect_err("binding-only command should reject direct command input");
        assert!(err.to_string().contains("internal command"));
    }

    #[test]
    fn internal_commands_reject_binding_invocation() {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Normal,
            None,
            &extensions,
        );

        let err = validate_command_id_for_policy("submit-search", &ctx)
            .expect_err("internal command should reject binding invocation");
        assert!(err.to_string().contains("internal command"));

        let internal_ctx = policy_context(
            CommandInvocationSource::Internal,
            Mode::Normal,
            None,
            &extensions,
        );
        validate_command_id_for_policy("submit-search", &internal_ctx)
            .expect("internal follow-up should be allowed");
    }

    #[test]
    fn palette_input_history_commands_require_history_capable_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let command_palette_ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Palette,
            Some(PaletteKind::Command),
            &extensions,
        );
        validate_command_for_policy(&Command::PaletteInputHistoryOlder, &command_palette_ctx)
            .expect("command palette should support input history");

        let outline_palette_ctx = policy_context(
            CommandInvocationSource::Binding,
            Mode::Palette,
            Some(PaletteKind::Outline),
            &extensions,
        );
        let err =
            validate_command_for_policy(&Command::PaletteInputHistoryOlder, &outline_palette_ctx)
                .expect_err("outline palette should not support input history");
        assert!(err.to_string().contains("palette input history"));
    }

    fn policy_context<'a>(
        source: CommandInvocationSource,
        mode: Mode,
        active_palette: Option<PaletteKind>,
        extensions: &'a ExtensionUiSnapshot,
    ) -> CommandPolicyContext<'a> {
        CommandPolicyContext {
            source,
            runtime: RuntimeConditionContext::new(mode, active_palette, extensions),
        }
    }
}
