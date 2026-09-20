use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn debug_status_show(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    set_visible(ctx, true)
}

pub(in crate::command) fn debug_status_hide(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    set_visible(ctx, false)
}

pub(in crate::command) fn debug_status_toggle(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    set_visible(ctx, !ctx.app.debug_status_visible)
}

fn set_visible(ctx: &mut CommandExecContext<'_>, visible: bool) -> AppResult<CommandExecution> {
    if ctx.app.debug_status_visible == visible {
        return Ok(CommandExecution::noop());
    }
    ctx.app.debug_status_visible = visible;
    Ok(CommandExecution::applied())
}
