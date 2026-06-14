use crate::app::NoticeAction;
use crate::command::CommandOutcome;
use crate::error::AppResult;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn text_insert(
    ctx: &mut CommandExecContext<'_>,
    text: String,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.insert_text(
        ctx.palette_registry,
        ctx.app,
        &extensions,
        text.as_str(),
    )?)
}

pub(in crate::command) fn text_delete_backward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.delete_backward(
        ctx.palette_registry,
        ctx.app,
        &extensions,
    )?)
}

pub(in crate::command) fn text_delete_forward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.delete_forward(
        ctx.palette_registry,
        ctx.app,
        &extensions,
    )?)
}

pub(in crate::command) fn text_move_left(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.move_cursor_left(
        ctx.palette_registry,
        ctx.app,
        &extensions,
    )?)
}

pub(in crate::command) fn text_move_right(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.move_cursor_right(
        ctx.palette_registry,
        ctx.app,
        &extensions,
    )?)
}

pub(in crate::command) fn palette_input_history_older(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.recall_history(
        ctx.palette_registry,
        ctx.app,
        &extensions,
        true,
    )?)
}

pub(in crate::command) fn palette_input_history_newer(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    text_execution(ctx.palette_manager.recall_history(
        ctx.palette_registry,
        ctx.app,
        &extensions,
        false,
    )?)
}

fn text_execution(changed: bool) -> AppResult<CommandExecution> {
    Ok(CommandExecution::from_notice_result((
        if changed {
            CommandOutcome::Applied
        } else {
            CommandOutcome::Noop
        },
        NoticeAction::Clear,
    )))
}
