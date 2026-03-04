pub mod engine;
pub mod palette;
pub mod state;

use crate::app::AppState;
use crate::event::AppEvent;
use crate::extension::Extension;
use crate::input::{AppInputEvent, InputHookResult};
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

    fn status_bar_segment(state: &Self::State, _app: &AppState) -> Option<String> {
        state.status_bar_segment()
    }
}
