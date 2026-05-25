use crate::error::AppResult;

use super::super::core::{
    first_page as first_page_core, goto_page as goto_page_core, last_page as last_page_core,
    next_page as next_page_core, prev_page as prev_page_core,
};
use super::super::dispatch::{CommandExecContext, CommandExecution};

pub(in crate::command) fn next_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = next_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn prev_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = prev_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn first_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = first_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn last_page(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = last_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn goto_page(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let result = goto_page_core(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result))
}
