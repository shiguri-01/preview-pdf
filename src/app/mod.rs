mod actors;
mod constants;
mod core;
mod event_bus;
mod event_loop;
mod frame_ops;
mod input_ops;
mod nav;
mod perf_runner;
mod render_ops;
mod runtime;
mod scale;
mod state;
pub(crate) mod terminal_session;
mod view_ops;

#[cfg(test)]
mod tests;

pub use core::App;
pub use runtime::RenderRuntime;
pub use state::{
    AppState, CacheHandle, CacheRefs, Mode, Notice, NoticeAction, NoticeLevel, PageLayoutMode,
    PaletteRequest, SpreadDirection, VisiblePageSlots,
};
