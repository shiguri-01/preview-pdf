use crate::app::{AppState, PageLayoutMode, SpreadDirection};
use crate::error::{AppError, AppResult};

use super::types::{ActionId, CommandOutcome, PageLayoutModeArg, SpreadDirectionArg};

const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;

pub(crate) fn next_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::NextPage);
    let page_count = resolve_page_count(app, page_count)?;
    let step = app.page_step();

    if app.current_page.saturating_add(step) >= page_count {
        app.status.message = format!(
            "already at last page ({})",
            format_page_location(app, page_count)
        );
        return Ok(CommandOutcome::Noop);
    }

    let target = app.current_page.saturating_add(step);
    app.current_page = app.normalize_page_for_layout(target, page_count);
    app.status.message = format!("page {}", format_page_location(app, page_count));
    Ok(CommandOutcome::Applied)
}

pub(crate) fn prev_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::PrevPage);
    let page_count = resolve_page_count(app, page_count)?;
    let step = app.page_step();

    if app.current_page == 0 {
        app.status.message = "already at first page (1)".to_string();
        return Ok(CommandOutcome::Noop);
    }

    let target = app.current_page.saturating_sub(step);
    app.current_page = app.normalize_page_for_layout(target, page_count);
    app.status.message = format!("page {}", format_page_location(app, page_count));
    Ok(CommandOutcome::Applied)
}

pub(crate) fn first_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::FirstPage);
    let page_count = resolve_page_count(app, page_count)?;

    if app.current_page == 0 {
        app.status.message = "already at first page (1)".to_string();
        return Ok(CommandOutcome::Noop);
    }

    app.current_page = app.normalize_page_for_layout(0, page_count);
    app.status.message = format!("page {}", format_page_location(app, page_count));
    Ok(CommandOutcome::Applied)
}

pub(crate) fn last_page(app: &mut AppState, page_count: usize) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::LastPage);
    let page_count = resolve_page_count(app, page_count)?;

    let target = app.normalize_page_for_layout(page_count - 1, page_count);
    if app.current_page == target {
        app.status.message = format!("already at page {}", format_page_location(app, page_count));
        return Ok(CommandOutcome::Noop);
    }

    app.current_page = target;
    app.status.message = format!("page {}", format_page_location(app, page_count));
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

    let target = app.normalize_page_for_layout(page - 1, page_count);
    if app.current_page == target {
        app.status.message = format!("already at page {}", format_page_location(app, page_count));
        return Ok(CommandOutcome::Noop);
    }

    app.current_page = target;
    app.status.message = format!("page {}", format_page_location(app, page_count));
    Ok(CommandOutcome::Applied)
}

pub(crate) fn set_page_layout(
    app: &mut AppState,
    page_count: usize,
    mode: PageLayoutModeArg,
    direction: Option<SpreadDirectionArg>,
) -> AppResult<CommandOutcome> {
    app.status.last_action_id = Some(ActionId::SetPageLayout);
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
        app.status.message = match app.page_layout_mode {
            PageLayoutMode::Single => "layout unchanged (single)".to_string(),
            PageLayoutMode::Spread => {
                format!("layout unchanged (spread {})", app.spread_direction.id())
            }
        };
        return Ok(CommandOutcome::Noop);
    }

    app.page_layout_mode = next_mode;
    if next_mode == PageLayoutMode::Spread {
        app.spread_direction = next_direction;
    }
    app.normalize_current_page(page_count);
    app.scroll_x = 0;
    app.scroll_y = 0;
    app.status.message = match app.page_layout_mode {
        PageLayoutMode::Single => "layout single".to_string(),
        PageLayoutMode::Spread => format!("layout spread ({})", app.spread_direction.id()),
    };
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

fn format_page_location(app: &AppState, page_count: usize) -> String {
    let slots = app.visible_page_slots(page_count);
    match app.page_layout_mode {
        PageLayoutMode::Single => format!("{}/{}", slots.anchor_page + 1, page_count),
        PageLayoutMode::Spread => match slots.trailing_page {
            Some(trailing) => format!("{}-{}/{}", slots.anchor_page + 1, trailing + 1, page_count),
            None => format!("{}/{}", slots.anchor_page + 1, page_count),
        },
    }
}
