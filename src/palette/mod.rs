mod candidate;
mod effect;
mod kind;
mod manager;
mod matcher;
mod provider;
pub mod providers;
mod registry;
mod request;
mod row;
mod text;
mod view;

pub use candidate::{PaletteCandidate, PaletteCandidateId};
pub use effect::{PalettePostAction, PaletteSubmitEffect, PaletteTabEffect};
pub use kind::PaletteKind;
pub use manager::PaletteManager;
#[cfg(test)]
pub use provider::PaletteAppSnapshot;
pub use provider::{PaletteContext, PaletteInputMode, PaletteProvider};
pub use registry::PaletteRegistry;
pub use request::PaletteOpenOptions;
pub use row::{PageIndex, PaletteRow};
pub use text::{PaletteTextPart, PaletteTextTone};
pub use view::{PaletteItemView, PaletteView};
