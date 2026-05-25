use crate::error::AppResult;

use super::super::core::set_debug_status_visible;
use super::super::dispatch::{CommandExecContext, CommandExecution};

pub(in crate::command) fn debug_status_show(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = set_debug_status_visible(ctx.app, true)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn debug_status_hide(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = set_debug_status_visible(ctx.app, false)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn debug_status_toggle(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let visible = !ctx.app.debug_status_visible;
    let result = set_debug_status_visible(ctx.app, visible)?;
    Ok(CommandExecution::from_notice_result(result))
}
