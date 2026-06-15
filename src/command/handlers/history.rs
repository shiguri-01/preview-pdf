use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn history_back(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.history_back(ctx.app, ctx.page_count());
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn history_forward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .history_forward(ctx.app, ctx.page_count());
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn history_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .history_goto(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn open_history(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied()
        .with_palette_request(ctx.extension_host.open_history_palette(ctx.app)))
}
