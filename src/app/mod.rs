mod actors;
mod constants;
mod core;
mod event_bus;
mod event_loop;
mod frame_ops;
mod input_ops;
mod nav;
mod render_ops;
mod render_runtime;
mod routing;
mod routing_effects;
mod runtime;
mod runtime_driver;
pub(crate) mod scale;
mod state;
pub(crate) mod terminal_session;
mod view_ops;

#[cfg(test)]
mod tests;

pub use core::{App, AppBuilder, RunOptions};
pub use render_runtime::RenderRuntime;
pub use state::{
    AppState, CacheHandle, CacheRefs, Mode, Notice, NoticeAction, NoticeLevel, PageLayoutMode,
    PaletteRequest, SpreadCoverPolicy, SpreadDirection, VisiblePageSlots, notice_action_for_error,
};

pub(crate) use runtime_driver::{
    RuntimeDriver, RuntimeDriverDecision, RuntimeDriverHandle, RuntimeMetricsSnapshot, RuntimeMode,
    RuntimeObservation, binding_request,
};
pub(crate) use terminal_session::{TerminalSession, TerminalSurface};
