pub mod engine;
pub mod matcher;
pub mod palette;
pub mod state;
pub(crate) mod worker;

pub use engine::SearchEvent;
pub use palette::SearchPaletteProvider;
pub use palette::SearchResultsPaletteProvider;
pub use state::{SearchRuntime, SearchUiSnapshot};
