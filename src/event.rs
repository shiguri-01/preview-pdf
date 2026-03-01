use crossterm::event::Event;

use crate::command::Command;
use crate::extension::AppEvent;
use crate::render::worker::RenderWorkerResult;

#[derive(Debug)]
pub(crate) enum DomainEvent {
    Input(Event),
    InputError(String),
    Command(Command),
    App(AppEvent),
    RenderComplete(RenderWorkerResult),
    PrefetchTick,
    RedrawTick,
    Wake,
}
