use std::sync::Arc;

use crate::app::NoticeAction;
use crate::error::AppResult;

use super::super::dispatch::{CommandExecContext, CommandExecution};
use super::super::types::CommandOutcome;

pub(in crate::command) fn cancel_search(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let _ = ctx.extension_host.cancel_search(Arc::clone(&ctx.pdf))?;
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn reload_document(
    _ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn quit(_ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    Ok(CommandExecution {
        outcome: CommandOutcome::QuitRequested,
        notice: NoticeAction::Keep,
        follow_up_commands: Vec::new(),
    })
}
