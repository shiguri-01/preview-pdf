pub mod palette;
pub mod state;

use crate::app::AppState;
use crate::event::AppEvent;
use crate::extension::Extension;
pub use palette::OutlinePaletteProvider;
pub use state::{OutlineCommandPort, OutlineState, OutlineUiSnapshot};

pub struct OutlineExtension;

impl Extension for OutlineExtension {
    type State = OutlineState;

    fn init_state() -> Self::State {
        OutlineState::default()
    }

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState) {
        let _ = (state, event, app);
    }

    fn on_document_reloaded(
        state: &mut Self::State,
        app: &mut AppState,
        pdf: crate::backend::SharedPdfBackend,
    ) {
        let _ = (app, pdf);
        state.on_document_reloaded();
    }
}
