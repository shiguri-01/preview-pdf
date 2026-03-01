pub mod engine;
pub mod palette;
pub mod state;

use crate::app::AppState;
use crate::extension::{AppEvent, AppInputEvent, Extension, InputHookResult};
use engine::SearchEngine;
pub use palette::SearchPaletteProvider;
pub use state::SearchState;

pub struct SearchExtension;

impl SearchExtension {
    pub fn drain_background(
        state: &mut SearchState,
        app: &mut AppState,
        search_engine: &mut SearchEngine,
    ) -> bool {
        state.on_background(app, search_engine)
    }
}

impl Extension for SearchExtension {
    type State = SearchState;

    fn init_state() -> Self::State {
        SearchState::default()
    }

    fn handle_input(
        state: &mut Self::State,
        event: AppInputEvent,
        app: &mut AppState,
    ) -> InputHookResult {
        state.on_input(event, app)
    }

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState) {
        let _ = (state, event, app);
    }
}
