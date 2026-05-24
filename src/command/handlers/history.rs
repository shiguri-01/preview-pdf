use crate::error::AppResult;
use crate::event::{HistoryOp, NavReason};

use super::super::dispatch::{CommandExecContext, CommandExecution};

pub(in crate::command) fn history_back(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.history_back(ctx.app, ctx.page_count());
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::History(HistoryOp::Back)))
}

pub(in crate::command) fn history_forward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .history_forward(ctx.app, ctx.page_count());
    Ok(CommandExecution::from_notice_result(result)
        .with_nav(NavReason::History(HistoryOp::Forward)))
}

pub(in crate::command) fn history_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .history_goto(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::History(HistoryOp::Goto)))
}

pub(in crate::command) fn open_history(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .open_history_palette(ctx.app, ctx.palette_requests);
    Ok(CommandExecution::from_notice_result(result))
}
