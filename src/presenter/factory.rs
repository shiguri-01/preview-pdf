use crate::error::AppResult;

use super::l2_cache::{L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES};
use super::ratatui::RatatuiImagePresenter;
use super::traits::{GraphicsProtocol, ImagePresenter, PresenterKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresenterFactoryOptions {
    pub l2_cache_limits: Option<(usize, usize)>,
    pub graphics_protocol: Option<GraphicsProtocol>,
}

pub fn create_presenter(
    kind: PresenterKind,
    options: PresenterFactoryOptions,
) -> AppResult<Box<dyn ImagePresenter>> {
    match kind {
        PresenterKind::RatatuiImage => {
            let (max_entries, memory_budget_bytes) = options
                .l2_cache_limits
                .unwrap_or((L2_MAX_ENTRIES, L2_MEMORY_BUDGET_BYTES));
            let presenter = RatatuiImagePresenter::with_cache_limits_and_graphics_protocol(
                max_entries,
                memory_budget_bytes,
                options.graphics_protocol,
            );
            Ok(Box::new(presenter))
        }
    }
}
