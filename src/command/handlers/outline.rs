use std::sync::Arc;

use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn open_outline(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let pdf = Arc::clone(&ctx.pdf);
    let request = ctx
        .extension_host
        .command_ports()
        .outline
        .open_palette(pdf)?;
    Ok(CommandExecution::applied().with_palette_request(request))
}

pub(in crate::command) fn outline_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
    _title: String,
) -> AppResult<CommandExecution> {
    let page_count = ctx.page_count();
    let result = ctx
        .extension_host
        .command_ports()
        .outline
        .goto(ctx.app, page_count, page)?;
    Ok(CommandExecution::from_notice_result(result))
}
