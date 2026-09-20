mod catalog;
mod dispatch;
mod effects;
mod handlers;
mod parse;
mod spec;
#[cfg(test)]
mod tests;
mod types;

pub use catalog::{Command, CommandId, CommandRequest};
pub use dispatch::{CommandDispatchContext, CommandDispatchResult, dispatch_with_view_policy};
pub use effects::CommandLifecycleEffect;
pub(crate) use parse::first_token;
pub use parse::{parse_command_text, parse_invocable_command_text};
pub(crate) use spec::command_registry;
pub use spec::{CommandPolicyContext, find_command_spec, is_command_visible_in_palette};
pub use types::{
    ArgHint, ArgKind, ArgSpec, CommandInvocationSource, CommandOutcome, CommandSpec, PanAmount,
    PanDirection, SearchMatcherKind,
};
pub(crate) use types::{CommandInvocationPolicy, CommandTargetRequirement};
#[cfg(test)]
pub use types::{SpreadCoverPolicyArg, SpreadDirectionArg};
