use std::sync::Arc;

use crate::app::scale::{ZOOM_MAX, ZOOM_MIN, next_zoom_step, prev_zoom_step};
use crate::app::{NoticeAction, PaletteRequest};
use crate::event::{GotoKind, HistoryOp, NavReason};
use crate::palette::PaletteOpenPayload;

use super::core::{
    close_help as close_help_core, first_page as first_page_core, goto_page as goto_page_core,
    last_page as last_page_core, next_page as next_page_core, open_help as open_help_core,
    prev_page as prev_page_core, reset_zoom, set_debug_status_visible, set_page_layout,
    set_zoom as set_zoom_core, set_zoom_with_notice,
};
use super::dispatch::{CommandExecContext, CommandExecution, TransitionHint};
use super::types::{
    CommandOutcome, PageLayoutModeArg, PanAmount, PanDirection, SearchMatcherKind,
    SpreadCoverPolicyArg, SpreadDirectionArg,
};
use crate::error::AppResult;
use crate::palette::PaletteKind;

pub(super) fn next_page(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = next_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::Step))
}

pub(super) fn prev_page(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = prev_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::Step))
}

pub(super) fn first_page(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = first_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::Goto(GotoKind::FirstPage)))
}

pub(super) fn last_page(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = last_page_core(ctx.app, ctx.page_count())?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::Goto(GotoKind::LastPage)))
}

pub(super) fn goto_page(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let result = goto_page_core(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result)
        .with_nav(NavReason::Goto(GotoKind::SpecificPage)))
}

pub(super) fn set_zoom(
    ctx: &mut CommandExecContext<'_>,
    value: f32,
) -> AppResult<CommandExecution> {
    let result = set_zoom_core(ctx.app, value)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn zoom_in(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let next = next_zoom_step(ctx.app.zoom);
    let notice = if next <= ctx.app.zoom {
        NoticeAction::warning(format!("maximum zoom is {ZOOM_MAX:.2}x"))
    } else {
        NoticeAction::Clear
    };
    let result = set_zoom_with_notice(ctx.app, next, notice)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn zoom_out(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let prev = prev_zoom_step(ctx.app.zoom);
    let notice = if prev >= ctx.app.zoom {
        NoticeAction::warning(format!("minimum zoom is {ZOOM_MIN:.2}x"))
    } else {
        NoticeAction::Clear
    };
    let result = set_zoom_with_notice(ctx.app, prev, notice)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn zoom_reset(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = reset_zoom(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn pan(
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

pub(super) fn page_layout_single(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = set_page_layout(
        ctx.app,
        ctx.page_count(),
        PageLayoutModeArg::Single,
        None,
        None,
    )?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::LayoutNormalize))
}

pub(super) fn page_layout_spread(
    ctx: &mut CommandExecContext<'_>,
    direction: Option<SpreadDirectionArg>,
    cover_policy: Option<SpreadCoverPolicyArg>,
) -> AppResult<CommandExecution> {
    let result = set_page_layout(
        ctx.app,
        ctx.page_count(),
        PageLayoutModeArg::Spread,
        direction,
        cover_policy,
    )?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::LayoutNormalize))
}

pub(super) fn debug_status_show(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = set_debug_status_visible(ctx.app, true)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn debug_status_hide(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = set_debug_status_visible(ctx.app, false)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn debug_status_toggle(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let visible = !ctx.app.debug_status_visible;
    let result = set_debug_status_visible(ctx.app, visible)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn open_palette(
    ctx: &mut CommandExecContext<'_>,
    kind: PaletteKind,
    payload: Option<PaletteOpenPayload>,
) -> AppResult<CommandExecution> {
    ctx.palette_requests
        .push_back(PaletteRequest::Open { kind, payload });
    Ok(CommandExecution::applied())
}

pub(super) fn close_palette(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    ctx.palette_requests.push_back(PaletteRequest::Close);
    Ok(CommandExecution::applied())
}

pub(super) fn open_help(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = open_help_core(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn close_help(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = close_help_core(ctx.app)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn help_scroll(
    ctx: &mut CommandExecContext<'_>,
    delta: isize,
) -> AppResult<CommandExecution> {
    ctx.app.scroll_help_by(delta);
    Ok(CommandExecution::applied())
}

pub(super) fn open_search(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .open_search_palette(ctx.app, ctx.palette_requests);
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn open_search_results(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .open_search_results_palette(ctx.app, ctx.palette_requests);
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn submit_search(
    ctx: &mut CommandExecContext<'_>,
    query: String,
    matcher: SearchMatcherKind,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .submit_search(ctx.app, Arc::clone(&ctx.pdf), query, matcher)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn search_result_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let query = ctx.extension_host.search_query().to_string();
    let result = ctx
        .extension_host
        .search_result_goto(ctx.app, ctx.page_count(), page)?;
    Ok(
        CommandExecution::from_notice_result(result).with_transition(TransitionHint {
            nav_reason: NavReason::Search { query },
            emit_when_unchanged: true,
        }),
    )
}

pub(super) fn next_search_hit(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let query = ctx.extension_host.search_query().to_string();
    let result = ctx.extension_host.next_search_hit(ctx.app);
    Ok(
        CommandExecution::from_notice_result(result).with_transition(TransitionHint {
            nav_reason: NavReason::Search { query },
            emit_when_unchanged: true,
        }),
    )
}

pub(super) fn prev_search_hit(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let query = ctx.extension_host.search_query().to_string();
    let result = ctx.extension_host.prev_search_hit(ctx.app);
    Ok(
        CommandExecution::from_notice_result(result).with_transition(TransitionHint {
            nav_reason: NavReason::Search { query },
            emit_when_unchanged: true,
        }),
    )
}

pub(super) fn history_back(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = ctx.extension_host.history_back(ctx.app, ctx.page_count());
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::History(HistoryOp::Back)))
}

pub(super) fn history_forward(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .history_forward(ctx.app, ctx.page_count());
    Ok(CommandExecution::from_notice_result(result)
        .with_nav(NavReason::History(HistoryOp::Forward)))
}

pub(super) fn history_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .history_goto(ctx.app, ctx.page_count(), page)?;
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::History(HistoryOp::Goto)))
}

pub(super) fn open_history(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .open_history_palette(ctx.app, ctx.palette_requests);
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn open_outline(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .open_outline_palette(Arc::clone(&ctx.pdf), ctx.palette_requests)?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(super) fn outline_goto(
    ctx: &mut CommandExecContext<'_>,
    page: usize,
    title: String,
) -> AppResult<CommandExecution> {
    let result = ctx
        .extension_host
        .outline_goto(ctx.app, ctx.page_count(), page)?;
    Ok(
        CommandExecution::from_notice_result(result).with_transition(TransitionHint {
            nav_reason: NavReason::Outline { title },
            emit_when_unchanged: true,
        }),
    )
}

pub(super) fn cancel(ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    if ctx.app.mode == crate::app::Mode::Help {
        let result = close_help_core(ctx.app)?;
        return Ok(CommandExecution::from_notice_result(result));
    }
    if ctx.app.mode == crate::app::Mode::Palette {
        ctx.palette_requests.push_back(PaletteRequest::Close);
        return Ok(CommandExecution::applied());
    }

    let _ = ctx.extension_host.cancel_search(Arc::clone(&ctx.pdf))?;
    Ok(CommandExecution::applied())
}

pub(super) fn quit(_ctx: &mut CommandExecContext<'_>) -> AppResult<CommandExecution> {
    Ok(CommandExecution {
        outcome: CommandOutcome::QuitRequested,
        notice: NoticeAction::Keep,
        transition: None,
    })
}

fn pan_delta(direction: PanDirection, cells: i32) -> (i32, i32) {
    match direction {
        PanDirection::Left => (cells.saturating_neg(), 0),
        PanDirection::Right => (cells, 0),
        PanDirection::Up => (0, cells.saturating_neg()),
        PanDirection::Down => (0, cells),
    }
}
