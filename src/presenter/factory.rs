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
            let presenter = match options.l2_cache_limits {
                Some((max_entries, memory_budget_bytes)) => {
                    RatatuiImagePresenter::with_cache_limits_and_graphics_protocol(
                        max_entries,
                        memory_budget_bytes,
                        options.graphics_protocol,
                    )
                }
                None => RatatuiImagePresenter::with_cache_limits_and_graphics_protocol(
                    L2_MAX_ENTRIES,
                    L2_MEMORY_BUDGET_BYTES,
                    options.graphics_protocol,
                ),
            };
            Ok(Box::new(presenter))
        }
    }
}
