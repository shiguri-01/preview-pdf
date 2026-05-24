use crate::error::AppResult;
use crate::event::NavReason;

use super::super::core::set_page_layout;
use super::super::dispatch::{CommandExecContext, CommandExecution};
use super::super::types::{PageLayoutModeArg, SpreadCoverPolicyArg, SpreadDirectionArg};

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
    Ok(CommandExecution::from_notice_result(result).with_nav(NavReason::LayoutNormalize))
}

pub(in crate::command) fn page_layout_spread(
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
