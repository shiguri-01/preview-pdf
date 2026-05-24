use std::sync::Arc;

use crate::app::{Mode, NoticeAction, PaletteRequest};
use crate::error::AppResult;

use super::super::core::close_help as close_help_core;
use super::super::dispatch::{CommandExecContext, CommandExecution};
use super::super::types::CommandOutcome;

pub(in crate::command) fn cancel(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    if ctx.app.mode == Mode::Help {
        let result = close_help_core(ctx.app)?;
        return Ok(CommandExecution::from_notice_result(result));
    }
    if ctx.app.mode == Mode::Palette {
        ctx.palette_requests.push_back(PaletteRequest::Close);
        return Ok(CommandExecution::applied());
    }

    let _ = ctx.extension_host.cancel_search(Arc::clone(&ctx.pdf))?;
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn quit(_ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    Ok(CommandExecution {
        outcome: CommandOutcome::QuitRequested,
        notice: NoticeAction::Keep,
        transition: None,
    })
}
