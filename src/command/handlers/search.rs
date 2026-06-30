use std::sync::Arc;

use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;
use super::super::types::SearchMatcherKind;

pub(in crate::command) fn open_search(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let request = ctx.extension_host.command_ports().search.open_palette();
    Ok(CommandExecution::applied().with_palette_request(request))
}

pub(in crate::command) fn open_search_results(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let Some(request) = ctx
        .extension_host
        .command_ports()
        .search
        .open_results_palette()
    else {
        return Ok(CommandExecution::noop());
    };
    Ok(CommandExecution::applied().with_palette_request(request))
}

pub(in crate::command) fn submit_search(
    ctx: &mut CommandExecContext<'_>,
    query: String,
    matcher: SearchMatcherKind,
) -> AppResult<CommandExecution> {
    let pdf = Arc::clone(&ctx.pdf);
    let result = ctx
        .extension_host
        .command_ports()
        .search
        .submit(ctx.app, pdf, query, matcher)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn search_result_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let page_count = ctx.page_count();
    let result = ctx
        .extension_host
        .command_ports()
        .search
        .goto_result(ctx.app, page_count, page)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn next_search_hit(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.command_ports().search.next_hit(ctx.app);
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn prev_search_hit(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.command_ports().search.prev_hit(ctx.app);
    Ok(CommandExecution::from_notice_result(result))
}
