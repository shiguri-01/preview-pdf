use std::sync::Arc;

use crate::error::AppResult;

use super::super::dispatch::{CommandExecContext, CommandExecution};

pub(in crate::command) fn open_outline(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .open_outline_palette(Arc::clone(&ctx.pdf), ctx.palette_requests)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn outline_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
    _title: String,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .outline_goto(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result))
}
