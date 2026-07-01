use crate::app::Mode;
use crate::app::PaletteRequest;
use crate::command::{CommandInvocationSource, CommandOutcome, CommandRequest};
use crate::error::AppResult;
use crate::palette::{PaletteKind, PaletteOpenOptions, PalettePostAction, PaletteSubmitEffect};

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;

pub(in crate::command) fn open_palette(
    _ctx: &mut CommandExecContext<'_>,
    kind: PaletteKind,
    options: PaletteOpenOptions,
) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied().with_palette_request(PaletteRequest::Open { kind, options }))
}

pub(in crate::command) fn close_palette(
    _ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    Ok(CommandExecution::applied().with_palette_request(PaletteRequest::Close))
}

pub(in crate::command) fn palette_submit(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let Some(kind) = ctx.palette_manager.active_kind() else {
        return Ok(CommandExecution::noop());
    };
    let extensions = ctx.extension_host.ui_snapshot(ctx.app);
    let Some(action) = ctx
        .palette_manager
        .submit(ctx.palette_registry, ctx.app, &extensions)?
    else {
        return Ok(CommandExecution::noop());
    };
    if !ctx.palette_manager.close_if_matches(action.session_id) {
        return Ok(CommandExecution::noop());
    }
    ctx.app.mode = Mode::Normal;

    let mut execution = CommandExecution::applied();
    match action.effect {
        PaletteSubmitEffect::Close => {}
        PaletteSubmitEffect::Reopen { kind, options } => {
            execution = execution.with_palette_request(PaletteRequest::Open { kind, options });
        }
        PaletteSubmitEffect::Dispatch {
            command,
            history_record,
            next,
        } => {
            if let Some(record) = history_record {
                execution = execution.with_input_history_record(record);
            }
            let source = match kind {
                PaletteKind::Command => CommandInvocationSource::CommandPaletteInput,
                PaletteKind::Search
                | PaletteKind::SearchResults
                | PaletteKind::History
                | PaletteKind::Outline => CommandInvocationSource::Internal,
            };
            execution = execution.with_follow_up(CommandRequest::new(command, source));
            match next {
                PalettePostAction::Close => {}
                PalettePostAction::Reopen { kind, options } => {
                    execution =
                        execution.with_palette_request(PaletteRequest::Open { kind, options });
                }
            }
        }
    }
    Ok(execution)
}

pub(in crate::command) fn palette_complete(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot(ctx.app);
    let changed = ctx
        .palette_manager
        .complete(ctx.palette_registry, ctx.app, &extensions)?;
    Ok(CommandExecution::from_notice_result((
        if changed {
            CommandOutcome::Applied
        } else {
            CommandOutcome::Noop
        },
        crate::app::NoticeAction::Clear,
    )))
}

pub(in crate::command) fn palette_select_next(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let changed = ctx.palette_manager.select_next_item();
    Ok(CommandExecution::from_notice_result((
        if changed {
            CommandOutcome::Applied
        } else {
            CommandOutcome::Noop
        },
        crate::app::NoticeAction::Clear,
    )))
}

pub(in crate::command) fn palette_select_prev(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let changed = ctx.palette_manager.select_previous();
    Ok(CommandExecution::from_notice_result((
        if changed {
            CommandOutcome::Applied
        } else {
            CommandOutcome::Noop
        },
        crate::app::NoticeAction::Clear,
    )))
}
