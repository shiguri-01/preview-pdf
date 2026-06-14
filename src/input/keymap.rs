use crate::command::{Command, PanAmount, PanDirection};
use crate::condition::{ConditionExpr, RuntimeCondition};
use crate::palette::PaletteKind;

use super::sequence::{GeneratedCommand, GeneratedKeyMatcher, KeyBindingScope, SequenceRegistry};
use super::shortcut::ShortcutKey;

const WHEN_SEARCH_ACTIVE: [RuntimeCondition; 1] = [RuntimeCondition::SearchIsActive];
const WHEN_PALETTE_INPUT_HISTORY_AVAILABLE: [RuntimeCondition; 1] =
    [RuntimeCondition::PaletteInputHistoryIsAvailable];
const WHEN_PALETTE_INPUT_HISTORY_UNAVAILABLE: [RuntimeCondition; 1] =
    [RuntimeCondition::PaletteInputHistoryIsUnavailable];

pub fn build_builtin_sequence_registry() -> SequenceRegistry {
    let mut registry = SequenceRegistry::new();

    register_static(
        &mut registry,
        &[ShortcutKey::char(':')],
        Command::OpenPalette {
            kind: PaletteKind::Command,
            payload: None,
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
    register_numeric_prefix(&mut registry, "goto-page", ShortcutKey::char('G'), |page| {
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
    register_static_with_condition(
        &mut registry,
        KeyBindingScope::Normal,
        ConditionExpr::All(&WHEN_SEARCH_ACTIVE),
        &[ShortcutKey::key(crossterm::event::KeyCode::Esc)],
        Command::CancelSearch,
    );
    register_builtin_focused_bindings(&mut registry);
    registry
}

fn register_static(registry: &mut SequenceRegistry, keys: &[ShortcutKey], command: Command) {
    register_static_with_condition(
        registry,
        KeyBindingScope::Normal,
        ConditionExpr::Always,
        keys,
        command,
    );
}

fn register_static_with_condition(
    registry: &mut SequenceRegistry,
    scope: KeyBindingScope,
    enabled_when: ConditionExpr,
    keys: &[ShortcutKey],
    command: Command,
) {
    registry
        .register_static_in_scope(scope, enabled_when, keys, command)
        .expect("built-in key binding should register");
}

fn register_numeric_prefix(
    registry: &mut SequenceRegistry,
    command_id: &'static str,
    suffix: ShortcutKey,
    factory: fn(usize) -> Command,
) {
    registry
        .register_numeric_prefix(command_id, suffix, factory)
        .expect("built-in numeric key binding should register");
}

pub(crate) fn register_builtin_focused_bindings(registry: &mut SequenceRegistry) {
    use crossterm::event::{KeyCode, KeyModifiers};

    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Esc)],
        Command::ClosePalette,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Enter)],
        Command::PaletteSubmit,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Tab)],
        Command::PaletteComplete,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('p')],
        Command::PaletteSelectPrev,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('n')],
        Command::PaletteSelectNext,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::All(&WHEN_PALETTE_INPUT_HISTORY_AVAILABLE),
        &[ShortcutKey::key(KeyCode::Up)],
        Command::PaletteInputHistoryOlder,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::All(&WHEN_PALETTE_INPUT_HISTORY_AVAILABLE),
        &[ShortcutKey::key(KeyCode::Down)],
        Command::PaletteInputHistoryNewer,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::All(&WHEN_PALETTE_INPUT_HISTORY_UNAVAILABLE),
        &[ShortcutKey::key(KeyCode::Up)],
        Command::PaletteSelectPrev,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::All(&WHEN_PALETTE_INPUT_HISTORY_UNAVAILABLE),
        &[ShortcutKey::key(KeyCode::Down)],
        Command::PaletteSelectNext,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Backspace)],
        Command::TextDeleteBackward,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('h')],
        Command::TextDeleteBackward,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Delete)],
        Command::TextDeleteForward,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::new(KeyCode::Delete, KeyModifiers::CONTROL)],
        Command::TextDeleteNextWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Left)],
        Command::TextMoveLeft,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('b')],
        Command::TextMoveLeft,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Right)],
        Command::TextMoveRight,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('f')],
        Command::TextMoveRight,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Home)],
        Command::TextMoveStart,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('a')],
        Command::TextMoveStart,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::End)],
        Command::TextMoveEnd,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('e')],
        Command::TextMoveEnd,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::new(KeyCode::Left, KeyModifiers::CONTROL)],
        Command::TextMovePrevWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::alt('b')],
        Command::TextMovePrevWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::new(KeyCode::Right, KeyModifiers::CONTROL)],
        Command::TextMoveNextWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::alt('f')],
        Command::TextMoveNextWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('w')],
        Command::TextDeletePrevWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::alt('d')],
        Command::TextDeleteNextWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::new(KeyCode::Backspace, KeyModifiers::ALT)],
        Command::TextDeletePrevWord,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('u')],
        Command::TextDeleteLine,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('k')],
        Command::TextDeleteToEnd,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        &[ShortcutKey::ctrl('y')],
        Command::TextYank,
    );
    registry.register_generated(
        KeyBindingScope::Palette,
        ConditionExpr::Always,
        GeneratedKeyMatcher::PrintableCharacter,
        GeneratedCommand::TextInsert,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Help,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Esc)],
        Command::CloseHelp,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Help,
        ConditionExpr::Always,
        &[ShortcutKey::char('j')],
        Command::HelpScrollDown,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Help,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Down)],
        Command::HelpScrollDown,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Help,
        ConditionExpr::Always,
        &[ShortcutKey::char('k')],
        Command::HelpScrollUp,
    );
    register_static_with_condition(
        registry,
        KeyBindingScope::Help,
        ConditionExpr::Always,
        &[ShortcutKey::key(KeyCode::Up)],
        Command::HelpScrollUp,
    );
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::Mode;
    use crate::command::{Command, PanAmount, PanDirection};
    use crate::condition::RuntimeConditionContext;
    use crate::extension::ExtensionUiSnapshot;
    use crate::input::sequence::KeyBindingContext;
    use crate::palette::PaletteKind;

    use super::build_builtin_sequence_registry;
    use crate::input::sequence::{
        DEFAULT_SEQUENCE_TIMEOUT, KeyBindingScope, SequenceResolution, SequenceResolver,
    };

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
    fn palette_builtins_map_common_line_editing_shortcuts() {
        let cases = [
            (
                KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
                Command::TextMoveStart,
            ),
            (
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                Command::TextMoveStart,
            ),
            (
                KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
                Command::TextMoveEnd,
            ),
            (
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                Command::TextMoveEnd,
            ),
            (
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
                Command::TextDeleteLine,
            ),
            (
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
                Command::TextDeletePrevWord,
            ),
            (
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
                Command::TextDeleteNextWord,
            ),
            (
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
                Command::TextDeleteToEnd,
            ),
            (
                KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
                Command::TextMovePrevWord,
            ),
            (
                KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
                Command::TextMoveNextWord,
            ),
            (
                KeyEvent::new(KeyCode::Delete, KeyModifiers::CONTROL),
                Command::TextDeleteNextWord,
            ),
            (
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
                Command::TextYank,
            ),
        ];

        for (key, expected) in cases {
            let registry = build_builtin_sequence_registry();
            let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
            let extensions = ExtensionUiSnapshot::default();

            assert_eq!(
                resolver.handle_key_in_context(
                    palette_key_context(PaletteKind::Search, &extensions),
                    key,
                ),
                SequenceResolution::Dispatch(expected),
                "unexpected palette command for {key:?}",
            );
        }
    }

    fn palette_key_context<'a>(
        kind: PaletteKind,
        extensions: &'a ExtensionUiSnapshot,
    ) -> KeyBindingContext<'a> {
        KeyBindingContext {
            scope: KeyBindingScope::Palette,
            runtime: RuntimeConditionContext::new(Mode::Normal, Some(kind), extensions),
        }
    }
}
