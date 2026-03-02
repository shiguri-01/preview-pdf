pub mod palette;
pub mod state;

use crate::app::AppState;
use crate::event::AppEvent;
use crate::extension::Extension;
use crate::input::{AppInputEvent, InputHookResult};
pub use palette::HistoryPaletteProvider;
pub use state::HistoryState;

pub struct HistoryExtension;

impl Extension for HistoryExtension {
    type State = HistoryState;

    fn init_state() -> Self::State {
        HistoryState::default()
    }

    fn handle_input(
        state: &mut Self::State,
        event: AppInputEvent,
        app: &mut AppState,
    ) -> InputHookResult {
        let _ = (state, event, app);
        InputHookResult::Ignored
    }

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState) {
        let _ = app;
        state.on_event(event);
    }
}
