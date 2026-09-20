use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;
use crate::app::Mode;

pub(in crate::command) fn open_help(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    if ctx.app.mode == Mode::Help && ctx.app.help_scroll == 0 {
        return Ok(CommandExecution::noop());
    }
    ctx.app.mode = Mode::Help;
    ctx.app.reset_help_scroll();
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn close_help(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    if ctx.app.mode != Mode::Help {
        return Ok(CommandExecution::noop());
    }
    ctx.app.mode = Mode::Normal;
    ctx.app.reset_help_scroll();
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn help_scroll_down(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let previous = ctx.app.help_scroll;
    ctx.app.scroll_help_by(1);
    Ok(if ctx.app.help_scroll == previous {
        CommandExecution::noop()
    } else {
        CommandExecution::applied()
    })
}

pub(in crate::command) fn help_scroll_up(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let previous = ctx.app.help_scroll;
    ctx.app.scroll_help_by(-1);
    Ok(if ctx.app.help_scroll == previous {
        CommandExecution::noop()
    } else {
        CommandExecution::applied()
    })
}
