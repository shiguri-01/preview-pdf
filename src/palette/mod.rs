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
    PageIndex, PaletteCandidate, PaletteCandidateId, PaletteContext, PaletteInputMode,
    PaletteItemView, PaletteOpenOptions, PalettePostAction, PaletteProvider, PaletteRow,
    PaletteSubmitEffect, PaletteTabEffect, PaletteTextPart, PaletteTextTone, PaletteView,
};
