use crate::error::AppResult;

use super::ratatui::RatatuiImagePresenter;
use super::traits::{ImagePresenter, PresenterKind};

pub fn create_presenter(kind: PresenterKind) -> AppResult<Box<dyn ImagePresenter>> {
    create_presenter_with_cache_limits(kind, None)
}

pub fn create_presenter_with_cache_limits(
    kind: PresenterKind,
    l2_cache_limits: Option<(usize, usize)>,
) -> AppResult<Box<dyn ImagePresenter>> {
    match kind {
        PresenterKind::RatatuiImage => {
            let presenter = match l2_cache_limits {
                Some((max_entries, memory_budget_bytes)) => {
                    RatatuiImagePresenter::with_cache_limits(max_entries, memory_budget_bytes)
                }
                None => RatatuiImagePresenter::new(),
            };
            Ok(Box::new(presenter))
        }
    }
}
