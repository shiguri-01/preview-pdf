use std::sync::Arc;

use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::{CommandExecution, CommandLifecycleEffect};

pub(in crate::command) fn cancel_search(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let pdf = Arc::clone(&ctx.pdf);
    let _ = ctx.extension_host.command_ports().search.cancel(pdf)?;
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn reload_document(
    _ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn quit(_ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied().with_lifecycle(CommandLifecycleEffect::Quit))
}
