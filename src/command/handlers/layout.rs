use crate::error::AppResult;

use super::super::core::set_page_layout;
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
    let result = set_page_layout(
        ctx.app,
        ctx.page_count(),
        PageLayoutModeArg::Single,
        None,
        None,
    )?;
    Ok(CommandExecution::from_notice_result(result))
}

pub(in crate::command) fn page_layout_spread(
    ctx: &mut CommandExecContext<'_>,
    direction: Option<SpreadDirectionArg>,
    cover_policy: Option<SpreadCoverPolicyArg>,
) -> AppResult<CommandExecution> {
    let direction = direction.or(Some(direction_arg(ctx.view_policy.spread_direction)));
    let cover_policy = cover_policy.or(Some(cover_policy_arg(ctx.view_policy.spread_cover)));
    let result = set_page_layout(
        ctx.app,
        ctx.page_count(),
        PageLayoutModeArg::Spread,
        direction,
        cover_policy,
    )?;
    Ok(CommandExecution::from_notice_result(result))
}
