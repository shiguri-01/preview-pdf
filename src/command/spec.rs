use crate::app::AppState;
use crate::error::{AppError, AppResult};
use crate::extension::ExtensionUiSnapshot;

use super::types::{
    ArgHint, ArgKind, ArgSpec, Command, CommandAvailability, CommandCondition, CommandExposure,
    CommandInvocationPolicy, CommandInvocationSource, CommandSpec, PanDirection,
    SpreadDirectionArg,
};

const NO_ARGS: [ArgSpec; 0] = [];
const NO_CONDITIONS: [CommandCondition; 0] = [];
const REQUIRES_SEARCH_ACTIVE: [CommandCondition; 1] = [CommandCondition::SearchActive];
const ARGS_GOTO_PAGE: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_ZOOM: [ArgSpec; 1] = [ArgSpec {
    name: "value",
    kind: ArgKind::F32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_PAN: [ArgSpec; 2] = [
    ArgSpec {
        name: "direction",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::Enum(PanDirection::values),
    },
    ArgSpec {
        name: "amount",
        kind: ArgKind::I32,
        required: false,
        hint: ArgHint::None,
    },
];
const ARGS_PAGE_LAYOUT_SPREAD: [ArgSpec; 1] = [ArgSpec {
    name: "direction",
    kind: ArgKind::String,
    required: false,
    hint: ArgHint::Enum(SpreadDirectionArg::values),
}];
const ARGS_OPEN_PALETTE: [ArgSpec; 2] = [
    ArgSpec {
        name: "kind",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::None,
    },
    ArgSpec {
        name: "seed",
        kind: ArgKind::String,
        required: false,
        hint: ArgHint::None,
    },
];
const ARGS_SUBMIT_SEARCH: [ArgSpec; 2] = [
    ArgSpec {
        name: "query",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::None,
    },
    ArgSpec {
        name: "matcher",
        kind: ArgKind::String,
        required: false,
        hint: ArgHint::None,
    },
];
const ARGS_HISTORY_GOTO: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_SEARCH_GOTO: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_OUTLINE_GOTO: [ArgSpec; 2] = [
    ArgSpec {
        name: "page",
        kind: ArgKind::I32,
        required: true,
        hint: ArgHint::None,
    },
    ArgSpec {
        name: "title",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::None,
    },
];

const COMMAND_SPECS: [CommandSpec; 33] = [
    CommandSpec {
        id: "next-page",
        title: "Next Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "prev-page",
        title: "Previous Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "first-page",
        title: "First Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "last-page",
        title: "Last Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "goto-page",
        title: "Go to Page",
        args: &ARGS_GOTO_PAGE,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "zoom",
        title: "Zoom",
        args: &ARGS_ZOOM,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "zoom-in",
        title: "Zoom In",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "zoom-out",
        title: "Zoom Out",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "zoom-reset",
        title: "Reset Zoom",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "pan",
        title: "Pan",
        args: &ARGS_PAN,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "page-layout-single",
        title: "Single Page Layout",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "page-layout-spread",
        title: "Spread Page Layout",
        args: &ARGS_PAGE_LAYOUT_SPREAD,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "debug-status-show",
        title: "Show Debug Status",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "debug-status-hide",
        title: "Hide Debug Status",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "debug-status-toggle",
        title: "Toggle Debug Status",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "open-palette",
        title: "Open Palette",
        args: &ARGS_OPEN_PALETTE,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::KeymapOnly,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "close-palette",
        title: "Close Palette",
        args: &NO_ARGS,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "help",
        title: "Open Help",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "close-help",
        title: "Close Help",
        args: &NO_ARGS,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::KeymapOnly,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "search",
        title: "Search",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "search-results",
        title: "Open Search Results",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
    },
    CommandSpec {
        id: "submit-search",
        title: "Submit Search",
        args: &ARGS_SUBMIT_SEARCH,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "search-goto",
        title: "Search Go to Result",
        args: &ARGS_SEARCH_GOTO,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
    },
    CommandSpec {
        id: "next-search-hit",
        title: "Next Search Hit",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
    },
    CommandSpec {
        id: "prev-search-hit",
        title: "Previous Search Hit",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
    },
    CommandSpec {
        id: "history-back",
        title: "History Back",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "history-forward",
        title: "History Forward",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "history-goto",
        title: "History Go to Page",
        args: &ARGS_HISTORY_GOTO,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "history",
        title: "Open History",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "outline",
        title: "Open Outline",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "outline-goto",
        title: "Outline Go to Page",
        args: &ARGS_OUTLINE_GOTO,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
    },
    CommandSpec {
        id: "cancel-search",
        title: "Cancel Search",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
    },
    CommandSpec {
        id: "quit",
        title: "Quit",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
    },
];

pub fn command_registry() -> &'static [CommandSpec] {
    &COMMAND_SPECS
}

pub fn all_command_specs() -> Vec<CommandSpec> {
    COMMAND_SPECS.to_vec()
}

pub struct CommandConditionContext<'a> {
    pub app: &'a AppState,
    pub extensions: &'a ExtensionUiSnapshot,
    pub source: CommandInvocationSource,
}

pub fn find_command_spec(id: &str) -> Option<CommandSpec> {
    COMMAND_SPECS.iter().find(|spec| spec.id == id).copied()
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
    if !is_invocation_source_allowed(spec, ctx.source) {
        return Err(AppError::invalid_argument(format!(
            "{} is an internal command and cannot be invoked directly",
            spec.id
        )));
    }

    if !is_command_available(spec, ctx) {
        return Err(AppError::invalid_argument(unavailable_message(spec, ctx)));
    }

    Ok(())
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
    use crate::app::AppState;
    use crate::extension::ExtensionUiSnapshot;

    use super::{
        CommandConditionContext, find_command_spec, is_command_visible_in_palette,
        validate_command_for_source, validate_command_id_for_source,
    };
    use crate::command::{Command, CommandInvocationPolicy, CommandInvocationSource};

    #[test]
    fn find_command_spec_returns_metadata_for_internal_command() {
        let spec = find_command_spec("submit-search").expect("spec should exist");
        assert_eq!(spec.invocation, CommandInvocationPolicy::InternalOnly);
    }

    #[test]
    fn palette_visibility_hides_internal_commands() {
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            app: &app,
            extensions: &extensions,
            source: CommandInvocationSource::CommandPaletteInput,
        };

        let spec = find_command_spec("open-palette").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }

    #[test]
    fn command_validation_rejects_search_navigation_when_search_is_inactive() {
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            app: &app,
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
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let keymap_ctx = CommandConditionContext {
            app: &app,
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
            app: &app,
            extensions: &extensions,
            source: CommandInvocationSource::CommandPaletteInput,
        };
        let err = validate_command_id_for_source("open-palette", &palette_input_ctx)
            .expect_err("command palette input should be rejected");
        assert!(err.to_string().contains("internal command"));
    }

    #[test]
    fn keymap_only_close_help_is_hidden_from_palette() {
        let app = AppState::default();
        let extensions = ExtensionUiSnapshot::default();
        let ctx = CommandConditionContext {
            app: &app,
            extensions: &extensions,
            source: CommandInvocationSource::CommandPaletteInput,
        };

        let spec = find_command_spec("close-help").expect("spec should exist");
        assert!(!is_command_visible_in_palette(spec, &ctx));
    }
}
