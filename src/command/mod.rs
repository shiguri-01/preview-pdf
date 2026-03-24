mod core;
mod dispatch;
mod parse;
mod spec;
mod types;

pub use dispatch::{CommandDispatchResult, dispatch, drain_background_events};
pub(crate) use parse::first_token;
pub use parse::{parse_command_text, parse_invocable_command_text};
pub use spec::{
    CommandConditionContext, all_command_specs, command_registry, find_command_spec,
    is_command_visible_in_palette, rejection_message_for_command, validate_command_for_source,
};
pub use types::{
    ActionId, ArgKind, ArgSpec, Command, CommandAvailability, CommandCondition, CommandExposure,
    CommandInvocationPolicy, CommandInvocationSource, CommandOutcome, CommandRequest, CommandSpec,
    PageLayoutModeArg, SearchMatcherKind, SpreadDirectionArg,
};
