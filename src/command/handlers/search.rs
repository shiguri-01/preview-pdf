use std::sync::Arc;

use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;
use super::super::types::SearchMatcherKind;

pub(in crate::command) fn open_search(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied().with_palette_request(ctx.extension_host.open_search_palette()))
}

pub(in crate::command) fn open_search_results(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let Some(request) = ctx.extension_host.open_search_results_palette() else {
        return Ok(CommandExecution::noop());
    };
    Ok(CommandExecution::applied().with_palette_request(request))
}

pub(in crate::command) fn submit_search(
    ctx: &mut CommandExecContext<'_>,
    query: String,
    matcher: SearchMatcherKind,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .submit_search(ctx.app, Arc::clone(&ctx.pdf), query, matcher)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn search_result_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .search_result_goto(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn next_search_hit(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.next_search_hit(ctx.app);
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn prev_search_hit(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.prev_search_hit(ctx.app);
    Ok(CommandExecution::from_notice_result(result))
}
