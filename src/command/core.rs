use crate::app::{AppState, NoticeAction, PageLayoutMode, SpreadDirection};
use crate::error::{AppError, AppResult};

use super::types::{CommandOutcome, PageLayoutModeArg, SpreadDirectionArg};

const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;

pub(crate) type CommandNoticeResult = (CommandOutcome, NoticeAction);

fn applied() -> CommandNoticeResult {
    (CommandOutcome::Applied, NoticeAction::Clear)
}

fn noop() -> CommandNoticeResult {
    (CommandOutcome::Noop, NoticeAction::Clear)
}

pub(crate) fn next_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(app, page_count)?;
    let step = app.page_step();

    if app.current_page.saturating_add(step) >= page_count {
        return Ok(noop());
    }

    let target = app.current_page.saturating_add(step);
    app.current_page = app.normalize_page_for_layout(target, page_count);
    Ok(applied())
}

pub(crate) fn prev_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(app, page_count)?;
    let step = app.page_step();

    if app.current_page == 0 {
        return Ok(noop());
    }

    let target = app.current_page.saturating_sub(step);
    app.current_page = app.normalize_page_for_layout(target, page_count);
    Ok(applied())
}

pub(crate) fn first_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(app, page_count)?;

    if app.current_page == 0 {
        return Ok(noop());
    }

    app.current_page = app.normalize_page_for_layout(0, page_count);
    Ok(applied())
}

pub(crate) fn last_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(app, page_count)?;

    let target = app.normalize_page_for_layout(page_count - 1, page_count);
    if app.current_page == target {
        return Ok(noop());
    }

    app.current_page = target;
    Ok(applied())
}

pub(crate) fn goto_page(
    app: &mut AppState,
    page_count: usize,
    page: usize,
) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(app, page_count)?;

    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }
    if page > page_count {
        return Err(AppError::invalid_argument(
            "page number exceeds document length",
        ));
    }

    let target = app.normalize_page_for_layout(page - 1, page_count);
    if app.current_page == target {
        return Ok(noop());
    }

    app.current_page = target;
    Ok(applied())
}

pub(crate) fn set_page_layout(
    app: &mut AppState,
    page_count: usize,
    mode: PageLayoutModeArg,
    direction: Option<SpreadDirectionArg>,
) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(app, page_count)?;

    let next_mode = match mode {
        PageLayoutModeArg::Single => PageLayoutMode::Single,
        PageLayoutModeArg::Spread => PageLayoutMode::Spread,
    };
    let next_direction = match direction {
        Some(SpreadDirectionArg::Ltr) => SpreadDirection::Ltr,
        Some(SpreadDirectionArg::Rtl) => SpreadDirection::Rtl,
        None => app.spread_direction,
    };
    if next_mode == PageLayoutMode::Single && direction.is_some() {
        return Err(AppError::invalid_argument(
            "single layout does not accept a spread direction",
        ));
    }

    let changed = app.page_layout_mode != next_mode
        || (next_mode == PageLayoutMode::Spread && app.spread_direction != next_direction);
    if !changed {
        return Ok(noop());
    }

    app.page_layout_mode = next_mode;
    if next_mode == PageLayoutMode::Spread {
        app.spread_direction = next_direction;
    }
    app.normalize_current_page(page_count);
    app.scroll_x = 0;
    app.scroll_y = 0;
    Ok(applied())
}

pub(crate) fn set_zoom(app: &mut AppState, value: f32) -> AppResult<CommandNoticeResult> {
    set_zoom_internal(app, value)
}

pub(crate) fn set_zoom_internal(app: &mut AppState, value: f32) -> AppResult<CommandNoticeResult> {
    if !value.is_finite() || value <= 0.0 {
        return Err(AppError::invalid_argument(
            "zoom must be a positive finite value",
        ));
    }

    let clamped = value.clamp(ZOOM_MIN, ZOOM_MAX);
    if zoom_eq(app.zoom, clamped) {
        return Ok(noop());
    }

    app.zoom = clamped;
    Ok(applied())
}

pub(crate) fn set_debug_status_visible(
    app: &mut AppState,
    visible: bool,
) -> AppResult<CommandNoticeResult> {
    if app.debug_status_visible == visible {
        return Ok(noop());
    }

    app.debug_status_visible = visible;
    Ok(applied())
}

fn resolve_page_count(_app: &mut AppState, page_count: usize) -> AppResult<usize> {
    if page_count > 0 {
        return Ok(page_count);
    }

    Err(AppError::unsupported("pdf has no pages"))
}

fn zoom_eq(left: f32, right: f32) -> bool {
    (left - right).abs() <= 0.0005
}
