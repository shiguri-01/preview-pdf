use crate::app::scale::{ZOOM_MAX, ZOOM_MIN};
use crate::app::{AppState, Mode, NoticeAction, PageLayoutMode, SpreadDirection};
use crate::error::{AppError, AppResult};

use super::types::{CommandOutcome, PageLayoutModeArg, SpreadDirectionArg};

pub(crate) type CommandNoticeResult = (CommandOutcome, NoticeAction);

fn applied() -> CommandNoticeResult {
    (CommandOutcome::Applied, NoticeAction::Clear)
}

fn noop() -> CommandNoticeResult {
    (CommandOutcome::Noop, NoticeAction::Clear)
}

pub(crate) fn next_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(page_count)?;
    let step = app.page_step();

    if app.current_page.saturating_add(step) >= page_count {
        return Ok(noop());
    }

    let target = app.current_page.saturating_add(step);
    app.current_page = app.normalize_page_for_layout(target, page_count);
    Ok(applied())
}

pub(crate) fn prev_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(page_count)?;
    let step = app.page_step();

    if app.current_page == 0 {
        return Ok(noop());
    }

    let target = app.current_page.saturating_sub(step);
    app.current_page = app.normalize_page_for_layout(target, page_count);
    Ok(applied())
}

pub(crate) fn first_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(page_count)?;

    if app.current_page == 0 {
        return Ok(noop());
    }

    app.current_page = app.normalize_page_for_layout(0, page_count);
    Ok(applied())
}

pub(crate) fn last_page(app: &mut AppState, page_count: usize) -> AppResult<CommandNoticeResult> {
    let page_count = resolve_page_count(page_count)?;

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
    let page_count = resolve_page_count(page_count)?;

    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }
    if page > page_count {
        return Err(AppError::page_out_of_range(page, page_count));
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
    let page_count = resolve_page_count(page_count)?;

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
    app.pan_x = 0;
    app.pan_y = 0;
    Ok(applied())
}

pub(crate) fn set_zoom(app: &mut AppState, value: f32) -> AppResult<CommandNoticeResult> {
    set_zoom_with_notice(app, value, NoticeAction::Clear)
}

pub(crate) fn set_zoom_with_notice(
    app: &mut AppState,
    value: f32,
    unclamped_notice: NoticeAction,
) -> AppResult<CommandNoticeResult> {
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

    if app.zoom == clamped {
        return Ok((CommandOutcome::Noop, notice));
    }

    app.zoom = clamped;
    Ok((CommandOutcome::Applied, notice))
}

pub(crate) fn reset_zoom(app: &mut AppState) -> AppResult<CommandNoticeResult> {
    if app.zoom == 1.0 && app.pan_x == 0 && app.pan_y == 0 {
        return Ok(noop());
    }

    app.zoom = 1.0;
    app.pan_x = 0;
    app.pan_y = 0;
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

pub(crate) fn open_help(app: &mut AppState) -> AppResult<CommandNoticeResult> {
    let changed = app.mode != Mode::Help || app.help_scroll != 0;
    if !changed {
        return Ok(noop());
    }

    app.mode = Mode::Help;
    app.reset_help_scroll();
    Ok(applied())
}

pub(crate) fn close_help(app: &mut AppState) -> AppResult<CommandNoticeResult> {
    if app.mode != Mode::Help {
        return Ok(noop());
    }

    app.mode = Mode::Normal;
    app.reset_help_scroll();
    Ok(applied())
}

fn resolve_page_count(page_count: usize) -> AppResult<usize> {
    if page_count > 0 {
        return Ok(page_count);
    }

    Err(AppError::unsupported("pdf has no pages"))
}

#[cfg(test)]
mod tests {
    use crate::app::{AppState, NoticeAction, NoticeLevel};

    use super::{ZOOM_MAX, ZOOM_MIN, reset_zoom, set_zoom};
    use crate::command::types::CommandOutcome;

    #[test]
    fn set_zoom_accepts_exact_bounds_without_warning() {
        let mut app = AppState {
            zoom: 1.0,
            ..AppState::default()
        };

        let (outcome, notice) = set_zoom(&mut app, ZOOM_MIN).expect("set_zoom should succeed");
        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(app.zoom, ZOOM_MIN);

        let (outcome, notice) = set_zoom(&mut app, ZOOM_MAX).expect("set_zoom should succeed");
        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(app.zoom, ZOOM_MAX);
    }

    #[test]
    fn set_zoom_clamps_out_of_range_values_with_warnings() {
        let mut app = AppState::default();

        let (outcome, notice) =
            set_zoom(&mut app, ZOOM_MIN - 0.0003).expect("set_zoom should succeed");
        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(
            notice,
            NoticeAction::Show {
                level: NoticeLevel::Warning,
                message: format!("minimum zoom is {ZOOM_MIN:.2}x"),
            }
        );
        assert_eq!(app.zoom, ZOOM_MIN);

        let (outcome, notice) =
            set_zoom(&mut app, ZOOM_MAX + 0.0004).expect("set_zoom should succeed");
        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(
            notice,
            NoticeAction::Show {
                level: NoticeLevel::Warning,
                message: format!("maximum zoom is {ZOOM_MAX:.2}x"),
            }
        );
        assert_eq!(app.zoom, ZOOM_MAX);
    }

    #[test]
    fn reset_zoom_reports_noop_only_when_zoom_and_pan_are_already_reset() {
        let mut app = AppState::default();

        let (outcome, notice) = reset_zoom(&mut app).expect("reset_zoom should succeed");
        assert_eq!(outcome, CommandOutcome::Noop);
        assert_eq!(notice, NoticeAction::Clear);

        let mut app = AppState {
            zoom: 1.0,
            pan_x: 3,
            pan_y: -2,
            ..AppState::default()
        };
        let (outcome, notice) = reset_zoom(&mut app).expect("reset_zoom should succeed");
        assert_eq!(outcome, CommandOutcome::Applied);
        assert_eq!(notice, NoticeAction::Clear);
        assert_eq!(app.zoom, 1.0);
        assert_eq!(app.pan_x, 0);
        assert_eq!(app.pan_y, 0);
    }
}
