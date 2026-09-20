use crate::app::NoticeAction;
use crate::app::scale::{ZOOM_MAX, ZOOM_MIN, next_zoom_step, prev_zoom_step};
use crate::error::{AppError, AppResult};

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;
use super::super::types::{PanAmount, PanDirection};

pub(in crate::command) fn set_zoom(
    ctx: &mut CommandExecContext<'_>,
    value: f32,
) -> AppResult<CommandExecution> {
    set_zoom_with_notice(ctx, value, NoticeAction::Clear)
}

pub(in crate::command) fn zoom_in(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let next = next_zoom_step(ctx.app.zoom);
    let notice = if next <= ctx.app.zoom {
        NoticeAction::warning(format!("maximum zoom is {ZOOM_MAX:.2}x"))
    } else {
        NoticeAction::Clear
    };
    set_zoom_with_notice(ctx, next, notice)
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
    set_zoom_with_notice(ctx, prev, notice)
}

pub(in crate::command) fn zoom_reset(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    if ctx.app.zoom == 1.0 && ctx.app.pan_x == 0 && ctx.app.pan_y == 0 {
        return Ok(CommandExecution::noop());
    }
    ctx.app.zoom = 1.0;
    ctx.app.pan_x = 0;
    ctx.app.pan_y = 0;
    Ok(CommandExecution::applied())
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

fn set_zoom_with_notice(
    ctx: &mut CommandExecContext<'_>,
    value: f32,
    unclamped_notice: NoticeAction,
) -> AppResult<CommandExecution> {
    if !value.is_finite() || value <= 0.0 {
        return Err(AppError::invalid_argument(
            "zoom must be a positive finite value",
        ));
    }
    let clamped = value.clamp(ZOOM_MIN, ZOOM_MAX);
    let notice = if value == clamped {
        unclamped_notice
    } else if value > clamped {
        NoticeAction::warning(format!("maximum zoom is {ZOOM_MAX:.2}x"))
    } else {
        NoticeAction::warning(format!("minimum zoom is {ZOOM_MIN:.2}x"))
    };
    let outcome = if ctx.app.zoom == clamped {
        crate::command::CommandOutcome::Noop
    } else {
        ctx.app.zoom = clamped;
        crate::command::CommandOutcome::Applied
    };
    Ok(CommandExecution::from_notice_result((outcome, notice)))
}
