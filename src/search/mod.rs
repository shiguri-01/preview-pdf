pub mod engine;
pub mod matcher;
pub mod palette;
pub mod state;
pub(crate) mod worker;

use crate::app::AppState;
use crate::event::AppEvent;
use crate::extension::Extension;
pub use engine::SearchEvent;
pub use palette::SearchPaletteProvider;
pub use palette::SearchResultsPaletteProvider;
pub use state::{SearchCommandPort, SearchRuntime, SearchUiSnapshot};

pub struct SearchExtension;

impl Extension for SearchExtension {
    type State = SearchRuntime;

    fn init_state() -> Self::State {
        SearchRuntime::default()
    }

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState) {
        let _ = (state, event, app);
    }

    fn on_document_reloaded(
        state: &mut Self::State,
        app: &mut AppState,
        pdf: crate::backend::SharedPdfBackend,
    ) {
        state.on_document_reloaded(app, pdf);
    }

    fn status_bar_segment(state: &Self::State, _app: &AppState) -> Option<String> {
        state.status_bar_segment()
    }
}
