mod kind;
mod manager;
mod matcher;
pub mod providers;
mod registry;
mod types;

pub use kind::PaletteKind;
pub use manager::PaletteManager;
pub use registry::PaletteRegistry;
#[cfg(test)]
pub use types::PaletteAppSnapshot;
pub use types::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteItemView, PaletteKeyResult,
    PaletteOpenPayload, PalettePayload, PalettePostAction, PaletteProvider, PaletteSearchText,
    PaletteSubmitEffect, PaletteTabEffect, PaletteTextPart, PaletteTextTone, PaletteView,
};
