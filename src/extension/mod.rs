mod host;
mod traits;

pub use crate::history::HistoryPaletteProvider;
pub use crate::outline::OutlinePaletteProvider;
pub use crate::search::{SearchPaletteProvider, SearchResultsPaletteProvider};
pub use host::{ExtensionHost, ExtensionUiSnapshot};
pub use traits::Extension;
