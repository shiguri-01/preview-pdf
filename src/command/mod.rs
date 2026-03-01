mod core;
mod dispatch;
mod parse;
mod spec;
mod types;

pub use dispatch::{CommandDispatchResult, dispatch, drain_background_events};
pub use parse::parse_command_text;
pub use spec::{all_command_specs, command_registry};
pub use types::{
    ActionId, ArgKind, ArgSpec, Command, CommandOutcome, CommandSpec, SearchMatcherKind,
};
