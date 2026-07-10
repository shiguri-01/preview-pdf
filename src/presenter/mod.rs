mod encode;
mod factory;
mod image_ops;
mod l2_cache;
mod ratatui;
mod terminal_cell;
mod traits;

#[cfg(test)]
mod tests;

pub use factory::{PresenterFactoryOptions, create_presenter};
pub use ratatui::RatatuiImagePresenter;
pub use traits::{
    GraphicsProtocol, ImagePresenter, PanOffset, PresenterBackgroundEvent, PresenterCaps,
    PresenterFeedback, PresenterHorizontalAlign, PresenterKind, PresenterRenderMode,
    PresenterRenderOptions, PresenterRenderOutcome, PresenterRenderSlot, PresenterRuntimeInfo,
    PresenterSlot, PresenterSlotOutcome, Viewport,
};
