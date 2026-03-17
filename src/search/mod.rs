pub mod engine;
pub mod palette;
pub mod state;

use crate::app::AppState;
use crate::event::AppEvent;
use crate::extension::Extension;
pub use palette::SearchPaletteProvider;
pub use state::{SearchRuntime, SearchState};

pub struct SearchExtension;

impl SearchExtension {
    pub fn drain_background(state: &mut SearchRuntime, app: &mut AppState) -> bool {
        state.on_background(app)
    }
}

impl Extension for SearchExtension {
    type State = SearchRuntime;

    fn init_state() -> Self::State {
        SearchRuntime::default()
    }

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState) {
        let _ = (state, event, app);
    }

    fn status_bar_segment(state: &Self::State, _app: &AppState) -> Option<String> {
        state.status_bar_segment()
    }
}
