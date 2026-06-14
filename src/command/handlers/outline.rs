use std::sync::Arc;

use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn open_outline(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let request = ctx
        .extension_host
        .open_outline_palette(Arc::clone(&ctx.pdf))?;
    Ok(CommandExecution::applied().with_palette_request(request))
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
