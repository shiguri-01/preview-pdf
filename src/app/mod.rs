mod actors;
mod constants;
mod core;
mod event_bus;
mod event_loop;
mod frame_ops;
mod input_ops;
mod loop_driver;
mod loop_effects;
mod loop_router;
mod loop_runtime;
mod nav;
mod render_ops;
mod runtime;
pub(crate) mod scale;
mod state;
pub(crate) mod terminal_session;
mod view_ops;

#[cfg(test)]
mod tests;

pub use core::{App, AppBuilder, RunOptions};
pub use runtime::RenderRuntime;
pub use state::{
    AppState, CacheHandle, CacheRefs, Mode, Notice, NoticeAction, NoticeLevel, PageLayoutMode,
    PaletteRequest, SpreadCoverPolicy, SpreadDirection, VisiblePageSlots, notice_action_for_error,
};

pub(crate) use loop_driver::{
    LoopDriver, LoopDriverDecision, LoopDriverHandle, LoopEventMode, LoopMetricsSnapshot,
    LoopObservation, binding_request,
};
pub(crate) use terminal_session::{TerminalSession, TerminalSurface};
