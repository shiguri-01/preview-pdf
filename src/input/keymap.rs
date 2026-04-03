use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Mode;
use crate::command::{Command, PanAmount, PanDirection};
use crate::palette::PaletteKind;

pub fn map_key_to_command(key: KeyEvent, mode: Mode) -> Option<Command> {
    match mode {
        Mode::Normal => map_normal_mode_key(key),
        Mode::Palette => None,
        Mode::Help => map_help_mode_key(key),
    }
}

fn map_normal_mode_key(key: KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('o') => Some(Command::HistoryBack),
            KeyCode::Char('i') => Some(Command::HistoryForward),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char(':') => Some(Command::OpenPalette {
            kind: PaletteKind::Command,
            seed: None,
        }),
        KeyCode::Char('/') => Some(Command::OpenSearch),
        KeyCode::Char('?') => Some(Command::OpenHelp),
        KeyCode::Char('H') => Some(Command::Pan {
            direction: PanDirection::Left,
            amount: PanAmount::DefaultStep,
        }),
        KeyCode::Char('J') => Some(Command::Pan {
            direction: PanDirection::Down,
            amount: PanAmount::DefaultStep,
        }),
        KeyCode::Char('K') => Some(Command::Pan {
            direction: PanDirection::Up,
            amount: PanAmount::DefaultStep,
        }),
        KeyCode::Char('L') => Some(Command::Pan {
            direction: PanDirection::Right,
            amount: PanAmount::DefaultStep,
        }),
        KeyCode::Char('j') => Some(Command::NextPage),
        KeyCode::Char('k') => Some(Command::PrevPage),
        KeyCode::Char('g') => Some(Command::FirstPage),
        KeyCode::Char('G') => Some(Command::LastPage),
        KeyCode::Char('+') => Some(Command::ZoomIn),
        KeyCode::Char('-') => Some(Command::ZoomOut),
        KeyCode::Char('0') => Some(Command::ZoomReset),
        KeyCode::Char('n') => Some(Command::NextSearchHit),
        KeyCode::Char('N') => Some(Command::PrevSearchHit),
        KeyCode::Char('q') => Some(Command::Quit),
        KeyCode::Esc => Some(Command::Cancel),
        _ => None,
    }
}

fn map_help_mode_key(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Esc => Some(Command::CloseHelp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::Mode;
    use crate::command::{Command, PanAmount, PanDirection};

    use super::map_key_to_command;

    #[test]
    fn normal_mode_maps_slash_to_open_search() {
        let search = map_key_to_command(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            Mode::Normal,
        );

        assert_eq!(search, Some(Command::OpenSearch));
    }

    #[test]
    fn help_mode_maps_escape_to_close_help() {
        let close_help =
            map_key_to_command(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), Mode::Help);
        assert_eq!(close_help, Some(Command::CloseHelp));

        let question_mark_in_help = map_key_to_command(
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            Mode::Help,
        );
        assert_eq!(question_mark_in_help, None);
    }

    #[test]
    fn normal_mode_maps_zero_to_zoom_reset() {
        let reset = map_key_to_command(
            KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE),
            Mode::Normal,
        );
        assert_eq!(reset, Some(Command::ZoomReset));
    }

    #[test]
    fn normal_mode_maps_history_shortcuts() {
        let back = map_key_to_command(
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
            Mode::Normal,
        );
        let forward = map_key_to_command(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL),
            Mode::Normal,
        );

        assert_eq!(back, Some(Command::HistoryBack));
        assert_eq!(forward, Some(Command::HistoryForward));
    }

    #[test]
    fn normal_mode_maps_pan_keys_to_default_step_commands() {
        let left = map_key_to_command(
            KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE),
            Mode::Normal,
        );
        assert_eq!(
            left,
            Some(Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::DefaultStep,
            })
        );

        let down = map_key_to_command(
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
            Mode::Normal,
        );
        assert_eq!(
            down,
            Some(Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            })
        );
    }
}
