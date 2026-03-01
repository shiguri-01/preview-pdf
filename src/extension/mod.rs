mod events;
mod host;
mod input;
mod traits;

pub use crate::history::HistoryPaletteProvider;
pub use crate::search::SearchPaletteProvider;
pub use events::{AppEvent, NavReason};
pub use host::ExtensionHost;
pub use input::{AppInputEvent, InputHookResult};
pub use traits::Extension;
