mod encode;
mod factory;
mod image_ops;
mod l2_cache;
mod ratatui;
mod terminal_cell;
mod traits;

#[cfg(test)]
mod tests;

pub use factory::{create_presenter, create_presenter_with_cache_limits};
pub use ratatui::RatatuiImagePresenter;
pub use traits::{
    ImagePresenter, PanOffset, PresenterCaps, PresenterKind, PresenterRuntimeInfo, Viewport,
};
