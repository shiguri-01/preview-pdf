use crossterm::event::{KeyCode, KeyEvent};

use crate::command::{Command, PanAmount, PanDirection};
use crate::palette::PaletteKind;

use super::sequence::SequenceRegistry;
use super::shortcut::ShortcutKey;

pub fn build_builtin_sequence_registry() -> SequenceRegistry {
    let mut registry = SequenceRegistry::new();

    register_static(
        &mut registry,
        &[ShortcutKey::char(':')],
        Command::OpenPalette {
            kind: PaletteKind::Command,
            seed: None,
        },
    );
    register_static(
        &mut registry,
        &[ShortcutKey::char('/')],
        Command::OpenSearch,
    );
    register_static(&mut registry, &[ShortcutKey::char('?')], Command::OpenHelp);
    register_static(
        &mut registry,
        &[ShortcutKey::char('H')],
        Command::Pan {
            direction: PanDirection::Left,
            amount: PanAmount::DefaultStep,
        },
    );
    register_static(
        &mut registry,
        &[ShortcutKey::char('J')],
        Command::Pan {
            direction: PanDirection::Down,
            amount: PanAmount::DefaultStep,
        },
    );
    register_static(
        &mut registry,
        &[ShortcutKey::char('K')],
        Command::Pan {
            direction: PanDirection::Up,
            amount: PanAmount::DefaultStep,
        },
    );
    register_static(
        &mut registry,
        &[ShortcutKey::char('L')],
        Command::Pan {
            direction: PanDirection::Right,
            amount: PanAmount::DefaultStep,
        },
    );
    register_static(&mut registry, &[ShortcutKey::char('j')], Command::NextPage);
    register_static(&mut registry, &[ShortcutKey::char('k')], Command::PrevPage);
    register_static(
        &mut registry,
        &[ShortcutKey::char('g'), ShortcutKey::char('g')],
        Command::FirstPage,
    );
    register_static(&mut registry, &[ShortcutKey::char('G')], Command::LastPage);
    register_numeric_prefix(&mut registry, ShortcutKey::char('G'), |page| {
        Command::GotoPage { page }
    });
    register_static(&mut registry, &[ShortcutKey::char('+')], Command::ZoomIn);
    register_static(&mut registry, &[ShortcutKey::char('-')], Command::ZoomOut);
    register_static(&mut registry, &[ShortcutKey::char('=')], Command::ZoomReset);
    register_static(
        &mut registry,
        &[ShortcutKey::ctrl('o')],
        Command::HistoryBack,
    );
    register_static(
        &mut registry,
        &[ShortcutKey::ctrl('i')],
        Command::HistoryForward,
    );
    register_static(
        &mut registry,
        &[ShortcutKey::char('n')],
        Command::NextSearchHit,
    );
    register_static(
        &mut registry,
        &[ShortcutKey::char('N')],
        Command::PrevSearchHit,
    );
    register_static(&mut registry, &[ShortcutKey::char('q')], Command::Quit);
    register_static(
        &mut registry,
        &[ShortcutKey::key(KeyCode::Esc)],
        Command::Cancel,
    );

    registry
}

pub fn map_help_mode_key(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Esc => Some(Command::CloseHelp),
        _ => None,
    }
}

fn register_static(registry: &mut SequenceRegistry, keys: &[ShortcutKey], command: Command) {
    registry
        .register_static(keys, command)
        .expect("built-in key binding should register");
}

fn register_numeric_prefix(
    registry: &mut SequenceRegistry,
    suffix: ShortcutKey,
    factory: fn(usize) -> Command,
) {
    registry
        .register_numeric_prefix(suffix, factory)
        .expect("built-in numeric key binding should register");
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::command::{Command, PanAmount, PanDirection};

    use super::{build_builtin_sequence_registry, map_help_mode_key};
    use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceResolution, SequenceResolver};

    #[test]
    fn builtins_preserve_existing_single_key_bindings() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let search = resolver.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(search, SequenceResolution::Dispatch(Command::OpenSearch));

        let help = resolver.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(help, SequenceResolution::Dispatch(Command::OpenHelp));

        let back = resolver.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(back, SequenceResolution::Dispatch(Command::HistoryBack));
    }

    #[test]
    fn builtins_require_double_g_for_first_page() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let first_g = resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(first_g, SequenceResolution::Pending);

        let second_g = resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(second_g, SequenceResolution::Dispatch(Command::FirstPage));
    }

    #[test]
    fn builtins_support_numeric_goto_prefix() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let four = resolver.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        assert_eq!(four, SequenceResolution::Pending);

        let two = resolver.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(two, SequenceResolution::Pending);

        let goto = resolver.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
        assert_eq!(
            goto,
            SequenceResolution::Dispatch(Command::GotoPage { page: 42 })
        );
    }

    #[test]
    fn builtins_map_equal_to_zoom_reset() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let reset = resolver.handle_key(KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE));
        assert_eq!(reset, SequenceResolution::Dispatch(Command::ZoomReset));
    }

    #[test]
    fn builtins_include_pan_keys() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let left = resolver.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE));
        assert_eq!(
            left,
            SequenceResolution::Dispatch(Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::DefaultStep,
            })
        );

        let down = resolver.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE));
        assert_eq!(
            down,
            SequenceResolution::Dispatch(Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            })
        );
    }

    #[test]
    fn builtins_accept_shift_modified_char_events_for_uppercase_commands() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let last_page = resolver.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(last_page, SequenceResolution::Dispatch(Command::LastPage));

        let pan_down = resolver.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(
            pan_down,
            SequenceResolution::Dispatch(Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            })
        );
    }

    #[test]
    fn builtins_accept_ctrl_shift_letter_as_ctrl_shortcut() {
        let registry = build_builtin_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let back = resolver.handle_key(KeyEvent::new(
            KeyCode::Char('O'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(back, SequenceResolution::Dispatch(Command::HistoryBack));
    }

    #[test]
    fn help_mode_maps_escape_to_close_help() {
        let close_help = map_help_mode_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(close_help, Some(Command::CloseHelp));

        let question_mark_in_help =
            map_help_mode_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(question_mark_in_help, None);
    }
}
