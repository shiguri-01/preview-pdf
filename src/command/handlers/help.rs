use crate::error::AppResult;

use super::super::core::{
    close_help as close_help_core, open_help as open_help_core,
    scroll_help_down as scroll_help_down_core, scroll_help_up as scroll_help_up_core,
};
use super::super::dispatch::{CommandExecContext, CommandExecution};

pub(in crate::command) fn open_help(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = open_help_core(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn close_help(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = close_help_core(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn help_scroll_down(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = scroll_help_down_core(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn help_scroll_up(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = scroll_help_up_core(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}
