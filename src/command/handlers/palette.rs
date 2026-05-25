use crate::app::PaletteRequest;
use crate::error::AppResult;
use crate::palette::{PaletteKind, PaletteOpenPayload};

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
