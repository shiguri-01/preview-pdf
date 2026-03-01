use crate::app::AppState;
use crate::error::{AppError, AppResult};

use super::types::{ActionId, CommandOutcome};

const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;

pub(crate) fn next_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::NextPage);
    let page_count = resolve_page_count(app, page_count)?;

    if app.current_page + 1 >= page_count {
        app.status.message = format!(
            "already at last page ({}/{})",
            app.current_page + 1,
            page_count
        );
        return Ok(CommandOutcome::Noop);
    }

    app.current_page += 1;
    app.status.message = format!("page {}/{}", app.current_page + 1, page_count);
    Ok(CommandOutcome::Applied)
}

pub(crate) fn prev_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::PrevPage);
    let page_count = resolve_page_count(app, page_count)?;

    if app.current_page == 0 {
        app.status.message = "already at first page (1)".to_string();
        return Ok(CommandOutcome::Noop);
    }

    app.current_page -= 1;
    app.status.message = format!("page {}/{}", app.current_page + 1, page_count);
    Ok(CommandOutcome::Applied)
}

pub(crate) fn first_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::FirstPage);
    let page_count = resolve_page_count(app, page_count)?;

    if app.current_page == 0 {
        app.status.message = "already at first page (1)".to_string();
        return Ok(CommandOutcome::Noop);
    }

    app.current_page = 0;
    app.status.message = format!("page 1/{page_count}");
    Ok(CommandOutcome::Applied)
}

pub(crate) fn last_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::LastPage);
    let page_count = resolve_page_count(app, page_count)?;

    let target = page_count - 1;
    if app.current_page == target {
        app.status.message = format!("already at last page ({}/{page_count})", target + 1);
        return Ok(CommandOutcome::Noop);
    }

    app.current_page = target;
    app.status.message = format!("page {}/{}", app.current_page + 1, page_count);
    Ok(CommandOutcome::Applied)
}

pub(crate) fn goto_page(
    app: &mut AppState,
    page_count: usize,
    page: usize,
) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::GotoPage);
    let page_count = resolve_page_count(app, page_count)?;

    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }
    if page > page_count {
        return Err(AppError::invalid_argument(
            "page number exceeds document length",
        ));
    }

    let target = page - 1;
    if app.current_page == target {
        app.status.message = format!("already at page {}/{}", target + 1, page_count);
        return Ok(CommandOutcome::Noop);
    }

    app.current_page = target;
    app.status.message = format!("page {}/{}", app.current_page + 1, page_count);
    Ok(CommandOutcome::Applied)
}

pub(crate) fn set_zoom(app: &mut AppState, value: f32) -> AppResult<CommandOutcome> {
    set_zoom_with_id(app, value, ActionId::SetZoom)
}

pub(crate) fn set_zoom_with_id(
    app: &mut AppState,
    value: f32,
    action_id: ActionId,
) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(action_id);

    if !value.is_finite() || value <= 0.0 {
        return Err(AppError::invalid_argument(
            "zoom must be a positive finite value",
        ));
    }

    let clamped = value.clamp(ZOOM_MIN, ZOOM_MAX);
    if zoom_eq(app.zoom, clamped) {
        app.status.message = format!("zoom unchanged ({:.2}x)", app.zoom);
        return Ok(CommandOutcome::Noop);
    }

    app.zoom = clamped;
    app.status.message = format!("zoom {:.2}x", app.zoom);
    Ok(CommandOutcome::Applied)
}

pub(crate) fn set_debug_status_visible(
    app: &mut AppState,
    visible: bool,
    action_id: ActionId,
) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(action_id);
    if app.debug_status_visible == visible {
        let state = if visible { "on" } else { "off" };
        app.status.message = format!("debug status unchanged ({state})");
        return Ok(CommandOutcome::Noop);
    }

    app.debug_status_visible = visible;
    let state = if visible { "on" } else { "off" };
    app.status.message = format!("debug status: {state}");
    Ok(CommandOutcome::Applied)
}

fn resolve_page_count(app: &mut AppState, page_count: usize) -> AppResult<usize> {
    if page_count > 0 {
        return Ok(page_count);
    }

    app.status.message = "command requires an active pdf document".to_string();
    Err(AppError::unsupported("pdf has no pages"))
}

fn zoom_eq(left: f32, right: f32) -> bool {
    (left - right).abs() <= 0.0005
}
