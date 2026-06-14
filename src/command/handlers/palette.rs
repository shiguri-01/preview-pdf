use crate::app::Mode;
use crate::app::PaletteRequest;
use crate::command::{CommandInvocationSource, CommandRequest};
use crate::error::AppResult;
use crate::palette::{PaletteKind, PaletteOpenPayload, PalettePostAction, PaletteSubmitEffect};

use super::super::dispatch::{CommandExecContext, CommandExecution};

pub(in crate::command) fn open_palette(
    ctx: &mut CommandExecContext<'_>,
    kind: PaletteKind,
    payload: Option<PaletteOpenPayload>,
) -> AppResult<CommandExecution> {
    ctx.palette_requests
        .push_back(PaletteRequest::Open { kind, payload });
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn close_palette(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    ctx.palette_requests.push_back(PaletteRequest::Close);
    Ok(CommandExecution::applied())
}

pub(in crate::command) fn palette_submit(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let Some(kind) = ctx.palette_manager.active_kind() else {
        return Ok(CommandExecution::from_notice_result((
            crate::command::CommandOutcome::Noop,
            crate::app::NoticeAction::Clear,
        )));
    };
    let extensions = ctx.extension_host.ui_snapshot();
    let Some(action) = ctx
        .palette_manager
        .submit(ctx.palette_registry, ctx.app, &extensions)?
    else {
        return Ok(CommandExecution::from_notice_result((
            crate::command::CommandOutcome::Noop,
            crate::app::NoticeAction::Clear,
        )));
    };
    if !ctx.palette_manager.close_if_matches(action.session_id) {
        return Ok(CommandExecution::from_notice_result((
            crate::command::CommandOutcome::Noop,
            crate::app::NoticeAction::Clear,
        )));
    }
    ctx.app.mode = Mode::Normal;

    let mut execution = CommandExecution::applied();
    match action.effect {
        PaletteSubmitEffect::Close => {}
        PaletteSubmitEffect::Reopen { kind, payload } => {
            ctx.palette_requests
                .push_back(PaletteRequest::Open { kind, payload });
        }
        PaletteSubmitEffect::Dispatch {
            command,
            history_record,
            next,
        } => {
            if let Some(record) = history_record {
                ctx.input_history.record(record);
            }
            let source = match kind {
                PaletteKind::Command => CommandInvocationSource::CommandPaletteInput,
                PaletteKind::Search
                | PaletteKind::SearchResults
                | PaletteKind::History
                | PaletteKind::Outline => CommandInvocationSource::Interaction,
            };
            execution = execution.with_follow_up(CommandRequest::new(command, source));
            match next {
                PalettePostAction::Close => {}
                PalettePostAction::Reopen { kind, payload } => {
                    ctx.palette_requests
                        .push_back(PaletteRequest::Open { kind, payload });
                }
            }
        }
    }
    Ok(execution)
}

pub(in crate::command) fn palette_complete(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let extensions = ctx.extension_host.ui_snapshot();
    let changed = ctx
        .palette_manager
        .complete(ctx.palette_registry, ctx.app, &extensions)?;
    Ok(CommandExecution::from_notice_result((
        if changed {
            crate::command::CommandOutcome::Applied
        } else {
            crate::command::CommandOutcome::Noop
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
            crate::command::CommandOutcome::Applied
        } else {
            crate::command::CommandOutcome::Noop
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
            crate::command::CommandOutcome::Applied
        } else {
            crate::command::CommandOutcome::Noop
        },
        crate::app::NoticeAction::Clear,
    )))
}
