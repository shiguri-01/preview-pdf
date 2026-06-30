use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn history_back(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let page_count = ctx.page_count();
    let result = ctx
        .extension_host
        .command_ports()
        .history
        .back(ctx.app, page_count);
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn history_forward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let page_count = ctx.page_count();
    let result = ctx
        .extension_host
        .command_ports()
        .history
        .forward(ctx.app, page_count);
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn history_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let page_count = ctx.page_count();
    let result = ctx
        .extension_host
        .command_ports()
        .history
        .goto(ctx.app, page_count, page)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn open_history(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let request = ctx
        .extension_host
        .command_ports()
        .history
        .open_palette(ctx.app);
    Ok(CommandExecution::applied().with_palette_request(request))
}
