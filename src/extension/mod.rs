mod host;

pub use crate::history::HistoryPaletteProvider;
pub use crate::outline::OutlinePaletteProvider;
pub use crate::search::{SearchPaletteProvider, SearchResultsPaletteProvider};
pub(crate) use host::ExtensionWorkerEvent;
pub use host::{ExtensionHost, ExtensionUiSnapshot};
