use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Mode;
use crate::command::Command;
use crate::palette::PaletteKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapPreset {
    Default,
    Emacs,
}

impl KeymapPreset {
    pub fn parse(value: &str) -> Self {
        match value {
            "default" => Self::Default,
            "emacs" => Self::Emacs,
            _ => Self::Default,
        }
    }
}

pub fn map_key_to_command(key: KeyEvent, mode: Mode) -> Option<Command> {
    map_key_to_command_with_preset(key, mode, KeymapPreset::Default)
}

pub fn map_key_to_command_with_preset(
    key: KeyEvent,
    mode: Mode,
    preset: KeymapPreset,
) -> Option<Command> {
    match mode {
        Mode::Normal => match preset {
            KeymapPreset::Default => map_normal_mode_key_default(key),
            KeymapPreset::Emacs => map_normal_mode_key_emacs(key),
        },
        Mode::Palette => None,
    }
}

fn map_normal_mode_key_default(key: KeyEvent) -> Option<Command> {
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
        KeyCode::Char('j') => Some(Command::NextPage),
        KeyCode::Char('k') => Some(Command::PrevPage),
        KeyCode::Char('g') => Some(Command::FirstPage),
        KeyCode::Char('G') => Some(Command::LastPage),
        KeyCode::Char('+') => Some(Command::ZoomIn),
        KeyCode::Char('-') => Some(Command::ZoomOut),
        KeyCode::Char('h') => Some(Command::Scroll { dx: -1, dy: 0 }),
        KeyCode::Char('l') => Some(Command::Scroll { dx: 1, dy: 0 }),
        KeyCode::Char('n') => Some(Command::NextSearchHit),
        KeyCode::Char('N') => Some(Command::PrevSearchHit),
        KeyCode::Char('q') => Some(Command::Quit),
        KeyCode::Esc => Some(Command::Cancel),
        _ => None,
    }
}

fn map_normal_mode_key_emacs(key: KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return match key.code {
            KeyCode::Char('x') => Some(Command::OpenPalette {
                kind: PaletteKind::Command,
                seed: None,
            }),
            KeyCode::Char('v') => Some(Command::PrevPage),
            _ => None,
        };
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('n') => Some(Command::NextPage),
            KeyCode::Char('p') => Some(Command::PrevPage),
            KeyCode::Char('s') => Some(Command::OpenSearch),
            KeyCode::Char('g') => Some(Command::Cancel),
            KeyCode::Char('o') => Some(Command::HistoryBack),
            KeyCode::Char('i') => Some(Command::HistoryForward),
            KeyCode::Char('q') => Some(Command::Quit),
            _ => None,
        };
    }

    match key.code {
        KeyCode::PageDown => Some(Command::NextPage),
        KeyCode::PageUp => Some(Command::PrevPage),
        _ => map_normal_mode_key_default(key),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::Mode;
    use crate::command::Command;

    use super::{KeymapPreset, map_key_to_command_with_preset};

    #[test]
    fn keymap_preset_parse_defaults_on_unknown_values() {
        assert_eq!(KeymapPreset::parse("default"), KeymapPreset::Default);
        assert_eq!(KeymapPreset::parse("emacs"), KeymapPreset::Emacs);
        assert_eq!(KeymapPreset::parse("unknown"), KeymapPreset::Default);
    }

    #[test]
    fn emacs_preset_maps_ctrl_n_and_alt_x() {
        let next = map_key_to_command_with_preset(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            Mode::Normal,
            KeymapPreset::Emacs,
        );
        assert_eq!(next, Some(Command::NextPage));

        let palette = map_key_to_command_with_preset(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
            Mode::Normal,
            KeymapPreset::Emacs,
        );
        assert!(matches!(palette, Some(Command::OpenPalette { .. })));
    }
}
