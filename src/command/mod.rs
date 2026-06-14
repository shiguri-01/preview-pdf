mod catalog;
mod core;
mod dispatch;
mod handlers;
mod parse;
mod spec;
#[cfg(test)]
mod tests;
mod types;

pub use catalog::{Command, CommandId, CommandRequest};
pub use dispatch::{
    CommandDispatchContext, CommandDispatchResult, dispatch_with_view_policy,
    drain_background_events,
};
pub(crate) use parse::first_token;
pub use parse::{parse_command_text, parse_invocable_command_text};
#[cfg(test)]
pub use spec::command_registry;
pub use spec::{
    CommandPolicyContext, all_command_specs, find_command_spec, is_command_visible_in_palette,
    validate_command_id_invocation_for_source, validate_command_invocation_for_source,
};
pub use types::{
    ArgHint, ArgKind, ArgSpec, CommandInvocationSource, CommandOutcome, CommandSpec, PanAmount,
    PanDirection, SearchMatcherKind,
};
#[cfg(test)]
pub use types::{
    CommandExposure, CommandInvocationPolicy, SpreadCoverPolicyArg, SpreadDirectionArg,
};
