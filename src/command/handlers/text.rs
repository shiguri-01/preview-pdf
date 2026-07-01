use crate::app::NoticeAction;
use crate::command::CommandOutcome;
use crate::error::AppResult;
use tui_input::InputRequest;

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn text_insert(
    ctx: &mut CommandExecContext<'_>,
    text: String,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot(ctx.app);
    text_execution(ctx.palette_session.insert_text(
        ctx.palette_registry,
        ctx.app,
        &extensions,
        text.as_str(),
    )?)
}

pub(in crate::command) fn text_delete_backward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::DeletePrevChar)
}

pub(in crate::command) fn text_delete_forward(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::DeleteNextChar)
}

pub(in crate::command) fn text_move_left(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::GoToPrevChar)
}

pub(in crate::command) fn text_move_right(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::GoToNextChar)
}

pub(in crate::command) fn text_move_start(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::GoToStart)
}

pub(in crate::command) fn text_move_end(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::GoToEnd)
}

pub(in crate::command) fn text_move_prev_word(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::GoToPrevWord)
}

pub(in crate::command) fn text_move_next_word(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::GoToNextWord)
}

pub(in crate::command) fn text_delete_prev_word(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::DeletePrevWord)
}

pub(in crate::command) fn text_delete_next_word(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::DeleteNextWord)
}

pub(in crate::command) fn text_delete_line(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::DeleteLine)
}

pub(in crate::command) fn text_delete_to_end(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::DeleteTillEnd)
}

pub(in crate::command) fn text_yank(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    text_edit(ctx, InputRequest::Yank)
}

fn text_edit(
    ctx: &mut CommandExecContext<'_>,
    request: InputRequest,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot(ctx.app);
    text_execution(ctx.palette_session.edit_input(
        ctx.palette_registry,
        ctx.app,
        &extensions,
        request,
    )?)
}

pub(in crate::command) fn palette_input_history_older(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot(ctx.app);
    text_execution(ctx.palette_session.recall_history(
        ctx.palette_registry,
        ctx.app,
        &extensions,
        true,
    )?)
}

pub(in crate::command) fn palette_input_history_newer(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot(ctx.app);
    text_execution(ctx.palette_session.recall_history(
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
