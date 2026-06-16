use crate::command::{Command, PanAmount, PanDirection};
use crate::condition::ConditionExpr;
use crate::palette::PaletteKind;

use crate::input::sequence::{GeneratedCommand, GeneratedKeyMatcher, SequenceRegistry};
use crate::input::shortcut::ShortcutKey;

use super::KeymapWhen;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapPreset {
    Default,
    None,
}

impl KeymapPreset {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    pub(crate) fn build_sequence_registry(self) -> SequenceRegistry {
        match self {
            Self::Default => build_default_sequence_registry(),
            Self::None => build_none_sequence_registry(),
        }
    }
}

pub(crate) fn build_default_sequence_registry() -> SequenceRegistry {
    let mut registry = build_base_sequence_registry();
    register_surface_open_bindings(&mut registry);
    register_page_navigation_bindings(&mut registry);
    register_view_bindings(&mut registry);
    register_history_bindings(&mut registry);
    register_search_navigation_bindings(&mut registry);
    register_quit_binding(&mut registry);
    registry
}

pub(crate) fn build_none_sequence_registry() -> SequenceRegistry {
    SequenceRegistry::new()
}

fn build_base_sequence_registry() -> SequenceRegistry {
    let mut registry = SequenceRegistry::new();
    register_search_cancellation_binding(&mut registry);
    register_palette_bindings(&mut registry);
    register_help_bindings(&mut registry);
    registry
}

fn register_surface_open_bindings(registry: &mut SequenceRegistry) {
    let when = KeymapWhen::Normal.condition();
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char(':')],
        Command::OpenPalette {
            kind: PaletteKind::Command,
            payload: None,
        },
    );
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('/')],
        Command::OpenSearch,
    );
    register_exact_binding(registry, when, &[ShortcutKey::char('?')], Command::OpenHelp);
}

fn register_page_navigation_bindings(registry: &mut SequenceRegistry) {
    let when = KeymapWhen::Normal.condition();
    register_exact_binding(registry, when, &[ShortcutKey::char('j')], Command::NextPage);
    register_exact_binding(registry, when, &[ShortcutKey::char('k')], Command::PrevPage);
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('g'), ShortcutKey::char('g')],
        Command::FirstPage,
    );
    register_exact_binding(registry, when, &[ShortcutKey::char('G')], Command::LastPage);
    register_numeric_prefix_binding(
        registry,
        when,
        "goto-page",
        ShortcutKey::char('G'),
        |page| Command::GotoPage { page },
    );
}

fn register_view_bindings(registry: &mut SequenceRegistry) {
    let when = KeymapWhen::Normal.condition();
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('H')],
        Command::Pan {
            direction: PanDirection::Left,
            amount: PanAmount::DefaultStep,
        },
    );
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('J')],
        Command::Pan {
            direction: PanDirection::Down,
            amount: PanAmount::DefaultStep,
        },
    );
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('K')],
        Command::Pan {
            direction: PanDirection::Up,
            amount: PanAmount::DefaultStep,
        },
    );
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('L')],
        Command::Pan {
            direction: PanDirection::Right,
            amount: PanAmount::DefaultStep,
        },
    );
    register_exact_binding(registry, when, &[ShortcutKey::char('+')], Command::ZoomIn);
    register_exact_binding(registry, when, &[ShortcutKey::char('-')], Command::ZoomOut);
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('=')],
        Command::ZoomReset,
    );
}

fn register_history_bindings(registry: &mut SequenceRegistry) {
    let when = KeymapWhen::Normal.condition();
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::ctrl('o')],
        Command::HistoryBack,
    );
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::ctrl('i')],
        Command::HistoryForward,
    );
}

fn register_search_navigation_bindings(registry: &mut SequenceRegistry) {
    let when = KeymapWhen::Normal.condition();
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('n')],
        Command::NextSearchHit,
    );
    register_exact_binding(
        registry,
        when,
        &[ShortcutKey::char('N')],
        Command::PrevSearchHit,
    );
}

fn register_quit_binding(registry: &mut SequenceRegistry) {
    register_exact_binding(
        registry,
        KeymapWhen::Normal.condition(),
        &[ShortcutKey::char('q')],
        Command::Quit,
    );
}

fn register_search_cancellation_binding(registry: &mut SequenceRegistry) {
    use crossterm::event::KeyCode;

    register_exact_binding(
        registry,
        KeymapWhen::NormalSearchActive.condition(),
        &[ShortcutKey::key(KeyCode::Esc)],
        Command::CancelSearch,
    );
}

fn register_palette_bindings(registry: &mut SequenceRegistry) {
    use crossterm::event::{KeyCode, KeyModifiers};

    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Esc)],
        Command::ClosePalette,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Enter)],
        Command::PaletteSubmit,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Tab)],
        Command::PaletteComplete,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('p')],
        Command::PaletteSelectPrev,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('n')],
        Command::PaletteSelectNext,
    );
    register_exact_binding(
        registry,
        KeymapWhen::PaletteWithInputHistory.condition(),
        &[ShortcutKey::key(KeyCode::Up)],
        Command::PaletteInputHistoryOlder,
    );
    register_exact_binding(
        registry,
        KeymapWhen::PaletteWithInputHistory.condition(),
        &[ShortcutKey::key(KeyCode::Down)],
        Command::PaletteInputHistoryNewer,
    );
    register_exact_binding(
        registry,
        KeymapWhen::PaletteNoInputHistory.condition(),
        &[ShortcutKey::key(KeyCode::Up)],
        Command::PaletteSelectPrev,
    );
    register_exact_binding(
        registry,
        KeymapWhen::PaletteNoInputHistory.condition(),
        &[ShortcutKey::key(KeyCode::Down)],
        Command::PaletteSelectNext,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Backspace)],
        Command::TextDeleteBackward,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('h')],
        Command::TextDeleteBackward,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Delete)],
        Command::TextDeleteForward,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::new(KeyCode::Delete, KeyModifiers::CONTROL)],
        Command::TextDeleteNextWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Left)],
        Command::TextMoveLeft,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('b')],
        Command::TextMoveLeft,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Right)],
        Command::TextMoveRight,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('f')],
        Command::TextMoveRight,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::Home)],
        Command::TextMoveStart,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('a')],
        Command::TextMoveStart,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::key(KeyCode::End)],
        Command::TextMoveEnd,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('e')],
        Command::TextMoveEnd,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::new(KeyCode::Left, KeyModifiers::CONTROL)],
        Command::TextMovePrevWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::alt('b')],
        Command::TextMovePrevWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::new(KeyCode::Right, KeyModifiers::CONTROL)],
        Command::TextMoveNextWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::alt('f')],
        Command::TextMoveNextWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('w')],
        Command::TextDeletePrevWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::alt('d')],
        Command::TextDeleteNextWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::new(KeyCode::Backspace, KeyModifiers::ALT)],
        Command::TextDeletePrevWord,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('u')],
        Command::TextDeleteLine,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('k')],
        Command::TextDeleteToEnd,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Palette.condition(),
        &[ShortcutKey::ctrl('y')],
        Command::TextYank,
    );
    registry.register_generated(
        KeymapWhen::Palette.condition(),
        GeneratedKeyMatcher::PrintableCharacter,
        GeneratedCommand::TextInsert,
    );
}

fn register_help_bindings(registry: &mut SequenceRegistry) {
    use crossterm::event::KeyCode;

    register_exact_binding(
        registry,
        KeymapWhen::Help.condition(),
        &[ShortcutKey::key(KeyCode::Esc)],
        Command::CloseHelp,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Help.condition(),
        &[ShortcutKey::char('j')],
        Command::HelpScrollDown,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Help.condition(),
        &[ShortcutKey::key(KeyCode::Down)],
        Command::HelpScrollDown,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Help.condition(),
        &[ShortcutKey::char('k')],
        Command::HelpScrollUp,
    );
    register_exact_binding(
        registry,
        KeymapWhen::Help.condition(),
        &[ShortcutKey::key(KeyCode::Up)],
        Command::HelpScrollUp,
    );
}

fn register_exact_binding(
    registry: &mut SequenceRegistry,
    enabled_when: ConditionExpr,
    keys: &[ShortcutKey],
    command: Command,
) {
    registry
        .register_exact(enabled_when, keys, command)
        .expect("key binding should register");
}

fn register_numeric_prefix_binding(
    registry: &mut SequenceRegistry,
    enabled_when: ConditionExpr,
    command_id: &'static str,
    suffix: ShortcutKey,
    factory: fn(usize) -> Command,
) {
    registry
        .register_numeric_prefix(enabled_when, command_id, suffix, factory)
        .expect("numeric key binding should register");
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

    use super::{KeymapPreset, build_default_sequence_registry};
    use crate::input::sequence::{DEFAULT_SEQUENCE_TIMEOUT, SequenceResolution, SequenceResolver};

    fn handle_normal_key(resolver: &mut SequenceResolver, key: KeyEvent) -> SequenceResolution {
        let extensions = ExtensionUiSnapshot::default();
        resolver.handle_key_in_context(KeyBindingContext::normal(&extensions), key)
    }

    #[test]
    fn none_preset_builds_empty_registry() {
        let snapshot = KeymapPreset::None.build_sequence_registry().snapshot();

        assert!(snapshot.exact_bindings.is_empty());
        assert!(snapshot.numeric_prefix_bindings.is_empty());
        assert!(snapshot.generated_bindings.is_empty());
    }

    #[test]
    fn defaults_preserve_existing_single_key_bindings() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let search = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        );
        assert_eq!(search, SequenceResolution::Dispatch(Command::OpenSearch));

        let help = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        );
        assert_eq!(help, SequenceResolution::Dispatch(Command::OpenHelp));

        let back = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        );
        assert_eq!(back, SequenceResolution::Dispatch(Command::HistoryBack));
    }

    #[test]
    fn defaults_require_double_g_for_first_page() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let first_g = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert_eq!(first_g, SequenceResolution::Pending);

        let second_g = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert_eq!(second_g, SequenceResolution::Dispatch(Command::FirstPage));
    }

    #[test]
    fn defaults_support_numeric_goto_prefix() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let four = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
        );
        assert_eq!(four, SequenceResolution::Pending);

        let two = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        );
        assert_eq!(two, SequenceResolution::Pending);

        let goto = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE),
        );
        assert_eq!(
            goto,
            SequenceResolution::Dispatch(Command::GotoPage { page: 42 })
        );
    }

    #[test]
    fn defaults_map_equal_to_zoom_reset() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let reset = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE),
        );
        assert_eq!(reset, SequenceResolution::Dispatch(Command::ZoomReset));
    }

    #[test]
    fn defaults_include_pan_keys() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let left = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE),
        );
        assert_eq!(
            left,
            SequenceResolution::Dispatch(Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::DefaultStep,
            })
        );

        let down = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
        );
        assert_eq!(
            down,
            SequenceResolution::Dispatch(Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            })
        );
    }

    #[test]
    fn defaults_accept_shift_modified_char_events_for_uppercase_commands() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let last_page = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
        );
        assert_eq!(last_page, SequenceResolution::Dispatch(Command::LastPage));

        let pan_down = handle_normal_key(
            &mut resolver,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT),
        );
        assert_eq!(
            pan_down,
            SequenceResolution::Dispatch(Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            })
        );
    }

    #[test]
    fn defaults_accept_ctrl_shift_letter_as_ctrl_shortcut() {
        let registry = build_default_sequence_registry();
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let back = handle_normal_key(
            &mut resolver,
            KeyEvent::new(
                KeyCode::Char('O'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        );
        assert_eq!(back, SequenceResolution::Dispatch(Command::HistoryBack));
    }

    #[test]
    fn palette_bindings_map_common_line_editing_shortcuts() {
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
            let registry = build_default_sequence_registry();
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

    #[test]
    fn palette_bindings_accept_meta_as_alt_for_word_editing() {
        let cases = [
            (
                KeyEvent::new(KeyCode::Char('b'), KeyModifiers::META),
                Command::TextMovePrevWord,
            ),
            (
                KeyEvent::new(KeyCode::Char('f'), KeyModifiers::META),
                Command::TextMoveNextWord,
            ),
            (
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::META),
                Command::TextDeleteNextWord,
            ),
            (
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::META),
                Command::TextDeletePrevWord,
            ),
        ];

        for (key, expected) in cases {
            let registry = build_default_sequence_registry();
            let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
            let extensions = ExtensionUiSnapshot::default();

            assert_eq!(
                resolver.handle_key_in_context(
                    palette_key_context(PaletteKind::Search, &extensions),
                    key,
                ),
                SequenceResolution::Dispatch(expected)
            );
        }
    }

    fn palette_key_context<'a>(
        kind: PaletteKind,
        extensions: &'a ExtensionUiSnapshot,
    ) -> KeyBindingContext<'a> {
        KeyBindingContext {
            runtime: RuntimeConditionContext::new(Mode::Palette, Some(kind), extensions),
        }
    }
}
