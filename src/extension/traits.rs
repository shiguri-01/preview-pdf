use crate::app::AppState;

use super::events::AppEvent;
use super::input::{AppInputEvent, InputHookResult};

pub trait Extension {
    type State: Send;

    fn init_state() -> Self::State;

    fn handle_input(
        state: &mut Self::State,
        event: AppInputEvent,
        app: &mut AppState,
    ) -> InputHookResult {
        let _ = (state, event, app);
        InputHookResult::Ignored
    }

    fn handle_event(state: &mut Self::State, event: &AppEvent, app: &mut AppState) {
        let _ = (state, event, app);
    }

    fn on_background(state: &mut Self::State, app: &mut AppState) -> bool {
        let _ = (state, app);
        false
    }
}
