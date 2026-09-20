use crate::app::{PageLayoutMode, SpreadCoverPolicy, SpreadDirection};
use crate::error::{AppError, AppResult};

use super::super::dispatch::CommandExecContext;
use super::super::effects::CommandExecution;
use super::super::types::{PageLayoutModeArg, SpreadCoverPolicyArg, SpreadDirectionArg};

fn direction_arg(policy: crate::app::SpreadDirection) -> SpreadDirectionArg {
    match policy {
        crate::app::SpreadDirection::Ltr => SpreadDirectionArg::Ltr,
        crate::app::SpreadDirection::Rtl => SpreadDirectionArg::Rtl,
    }
}

fn cover_policy_arg(policy: crate::app::SpreadCoverPolicy) -> SpreadCoverPolicyArg {
    match policy {
        crate::app::SpreadCoverPolicy::Paired => SpreadCoverPolicyArg::Paired,
        crate::app::SpreadCoverPolicy::Cover => SpreadCoverPolicyArg::Cover,
    }
}

pub(in crate::command) fn page_layout_single(
    ctx: &mut CommandExecContext<'_>,
) -> AppResult<CommandExecution> {
    set_page_layout(ctx, PageLayoutModeArg::Single, None, None)
}

pub(in crate::command) fn page_layout_spread(
    ctx: &mut CommandExecContext<'_>,
    direction: Option<SpreadDirectionArg>,
    cover_policy: Option<SpreadCoverPolicyArg>,
) -> AppResult<CommandExecution> {
    let direction = direction.or(Some(direction_arg(ctx.view_policy.spread_direction)));
    let cover_policy = cover_policy.or(Some(cover_policy_arg(ctx.view_policy.spread_cover)));
    set_page_layout(ctx, PageLayoutModeArg::Spread, direction, cover_policy)
}

fn set_page_layout(
    ctx: &mut CommandExecContext<'_>,
    mode: PageLayoutModeArg,
    direction: Option<SpreadDirectionArg>,
    cover_policy: Option<SpreadCoverPolicyArg>,
) -> AppResult<CommandExecution> {
    let page_count = ctx.page_count();
    if page_count == 0 {
        return Err(AppError::unsupported("pdf has no pages"));
    }
    let next_mode = match mode {
        PageLayoutModeArg::Single => PageLayoutMode::Single,
        PageLayoutModeArg::Spread => PageLayoutMode::Spread,
    };
    let next_direction = match direction {
        Some(SpreadDirectionArg::Ltr) => SpreadDirection::Ltr,
        Some(SpreadDirectionArg::Rtl) => SpreadDirection::Rtl,
        None => ctx.app.spread_direction,
    };
    let next_cover_policy = match cover_policy {
        Some(SpreadCoverPolicyArg::Cover) => SpreadCoverPolicy::Cover,
        Some(SpreadCoverPolicyArg::Paired) | None => SpreadCoverPolicy::Paired,
    };
    if next_mode == PageLayoutMode::Single && (direction.is_some() || cover_policy.is_some()) {
        return Err(AppError::invalid_argument(
            "single layout does not accept spread arguments",
        ));
    }
    let changed = ctx.app.page_layout_mode != next_mode
        || (next_mode == PageLayoutMode::Spread
            && (ctx.app.spread_direction != next_direction
                || ctx.app.spread_cover_policy != next_cover_policy));
    if !changed {
        return Ok(CommandExecution::noop());
    }
    ctx.app.page_layout_mode = next_mode;
    if next_mode == PageLayoutMode::Spread {
        ctx.app.spread_direction = next_direction;
        ctx.app.spread_cover_policy = next_cover_policy;
    }
    ctx.app.normalize_current_page(page_count);
    ctx.app.pan_x = 0;
    ctx.app.pan_y = 0;
    Ok(CommandExecution::applied())
}
