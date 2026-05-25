use crate::app::NoticeAction;
use crate::app::scale::{ZOOM_MAX, ZOOM_MIN, next_zoom_step, prev_zoom_step};
use crate::error::AppResult;

use super::super::core::{reset_zoom, set_zoom as set_zoom_core, set_zoom_with_notice};
use super::super::dispatch::{CommandExecContext, CommandExecution};
use super::super::types::{PanAmount, PanDirection};

pub(in crate::command) fn set_zoom(
    ctx: &mut CommandExecContext<'_>,
    value: f32,
) -> AppResult<CommandExecution> {
    let result = set_zoom_core(ctx.app, value)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn zoom_in(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let next = next_zoom_step(ctx.app.zoom);
    let notice = if next <= ctx.app.zoom {
        NoticeAction::warning(format!("maximum zoom is {ZOOM_MAX:.2}x"))
    } else {
        NoticeAction::Clear
    };
    let result = set_zoom_with_notice(ctx.app, next, notice)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn zoom_out(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let prev = prev_zoom_step(ctx.app.zoom);
    let notice = if prev >= ctx.app.zoom {
        NoticeAction::warning(format!("minimum zoom is {ZOOM_MIN:.2}x"))
    } else {
        NoticeAction::Clear
    };
    let result = set_zoom_with_notice(ctx.app, prev, notice)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn zoom_reset(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    let result = reset_zoom(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn pan(
    ctx: &mut CommandExecContext<'_>,
    direction: PanDirection,
    amount: PanAmount,
) -> AppResult<CommandExecution> {
    let cells = match amount {
        PanAmount::DefaultStep => 1,
        PanAmount::Cells(cells) => cells,
    };
    let (dx, dy) = pan_delta(direction, cells);
    ctx.app.pan_x = ctx.app.pan_x.saturating_add(dx);
    ctx.app.pan_y = ctx.app.pan_y.saturating_add(dy);
    Ok(CommandExecution::applied())
}

fn pan_delta(direction: PanDirection, cells: i32) -> (i32, i32) {
    match direction {
        PanDirection::Left => (cells.saturating_neg(), 0),
        PanDirection::Right => (cells, 0),
        PanDirection::Up => (0, cells.saturating_neg()),
        PanDirection::Down => (0, cells),
    }
}
