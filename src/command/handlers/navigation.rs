use crate::error::{AppError, AppResult};

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn next_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let page_count = page_count(ctx)?;
    let target = ctx
        .app
        .next_page_for_layout(ctx.app.current_page, page_count);
    if ctx.app.current_page == target {
        return Ok(CommandExecution::noop());
    }
    ctx.app.current_page = target;
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn prev_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let page_count = page_count(ctx)?;
    let target = ctx
        .app
        .prev_page_for_layout(ctx.app.current_page, page_count);
    if ctx.app.current_page == target {
        return Ok(CommandExecution::noop());
    }
    ctx.app.current_page = target;
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn first_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let page_count = page_count(ctx)?;
    if ctx.app.current_page == 0 {
        return Ok(CommandExecution::noop());
    }
    ctx.app.current_page = ctx.app.normalize_page_for_layout(0, page_count);
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn last_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let page_count = page_count(ctx)?;
    let target = ctx
        .app
        .normalize_page_for_layout(page_count - 1, page_count);
    if ctx.app.current_page == target {
        return Ok(CommandExecution::noop());
    }
    ctx.app.current_page = target;
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn goto_page(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let page_count = page_count(ctx)?;
    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }
    if page > page_count {
        return Err(AppError::page_out_of_range(page, page_count));
    }
    let target = ctx.app.normalize_page_for_layout(page - 1, page_count);
    if ctx.app.current_page == target {
        return Ok(CommandExecution::noop());
    }
    ctx.app.current_page = target;
    Ok(CommandExecution::applied())
}

fn page_count(ctx: &CommandExecContext<'_>) -> AppResult<usize> {
    let page_count = ctx.page_count();
    if page_count == 0 {
        return Err(AppError::unsupported("pdf has no pages"));
    }
    Ok(page_count)
}
