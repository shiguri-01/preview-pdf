use crossterm::event::KeyEvent;

use crate::command::Command;

#[derive(Debug, Clone, Copy)]
pub enum AppInputEvent {
    Key(KeyEvent),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputHookResult {
    Ignored,
    Consumed,
    EmitCommand(Command),
}
