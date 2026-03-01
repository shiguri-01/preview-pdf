mod kind;
mod manager;
mod matcher;
pub mod providers;
mod registry;
mod types;

pub use kind::PaletteKind;
pub use manager::PaletteManager;
pub use matcher::{CandidateMatcher, ContainsMatcher};
pub use registry::PaletteRegistry;
pub use types::{
    PaletteCandidate, PaletteContext, PaletteInputMode, PaletteItemView, PaletteKeyResult,
    PalettePayload, PalettePostAction, PaletteProvider, PaletteSubmitAction, PaletteSubmitEffect,
    PaletteTabEffect, PaletteView,
};
