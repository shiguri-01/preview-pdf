use crate::app::Mode;
use crate::command::{
    Command, CommandInvocationPolicy, CommandTargetRequirement, find_command_spec, first_token,
    parse_command_text,
};
use crate::condition::{ConditionExpr, RuntimeCondition};
use crate::error::{AppError, AppResult};
use crate::input::keymap::build_default_sequence_registry;
use crate::input::sequence::{SequenceRegistrationError, SequenceRegistry};
use crate::input::shortcut::{ShortcutKey, parse_shortcut_sequence};
use crate::palette::PaletteKind;

const WHEN_NORMAL: [RuntimeCondition; 1] = [RuntimeCondition::ModeIs(Mode::Normal)];
const WHEN_NORMAL_SEARCH_ACTIVE: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Normal),
    RuntimeCondition::SearchIsActive,
];
const WHEN_NORMAL_SEARCH_INACTIVE: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Normal),
    RuntimeCondition::SearchIsInactive,
];
const WHEN_HELP: [RuntimeCondition; 1] = [RuntimeCondition::ModeIs(Mode::Help)];
const WHEN_PALETTE: [RuntimeCondition; 1] = [RuntimeCondition::ModeIs(Mode::Palette)];
const WHEN_PALETTE_COMMAND: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteKindIs(PaletteKind::Command),
];
const WHEN_PALETTE_SEARCH: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteKindIs(PaletteKind::Search),
];
const WHEN_PALETTE_SEARCH_RESULTS: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteKindIs(PaletteKind::SearchResults),
];
const WHEN_PALETTE_HISTORY: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteKindIs(PaletteKind::History),
];
const WHEN_PALETTE_OUTLINE: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteKindIs(PaletteKind::Outline),
];
const WHEN_PALETTE_WITH_INPUT_HISTORY: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteInputHistoryIsAvailable,
];
const WHEN_PALETTE_NO_INPUT_HISTORY: [RuntimeCondition; 2] = [
    RuntimeCondition::ModeIs(Mode::Palette),
    RuntimeCondition::PaletteInputHistoryIsUnavailable,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapWhen {
    Normal,
    NormalSearchActive,
    NormalSearchInactive,
    Help,
    Palette,
    PaletteCommand,
    PaletteSearch,
    PaletteSearchResults,
    PaletteHistory,
    PaletteOutline,
    PaletteWithInputHistory,
    PaletteNoInputHistory,
}

impl KeymapWhen {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "normal.search-active" => Some(Self::NormalSearchActive),
            "normal.search-inactive" => Some(Self::NormalSearchInactive),
            "help" => Some(Self::Help),
            "palette" => Some(Self::Palette),
            "palette.command" => Some(Self::PaletteCommand),
            "palette.search" => Some(Self::PaletteSearch),
            "palette.search-results" => Some(Self::PaletteSearchResults),
            "palette.history" => Some(Self::PaletteHistory),
            "palette.outline" => Some(Self::PaletteOutline),
            "palette.with-input-history" => Some(Self::PaletteWithInputHistory),
            "palette.no-input-history" => Some(Self::PaletteNoInputHistory),
            _ => None,
        }
    }

    pub(crate) fn condition(self) -> ConditionExpr {
        match self {
            Self::Normal => ConditionExpr::All(&WHEN_NORMAL),
            Self::NormalSearchActive => ConditionExpr::All(&WHEN_NORMAL_SEARCH_ACTIVE),
            Self::NormalSearchInactive => ConditionExpr::All(&WHEN_NORMAL_SEARCH_INACTIVE),
            Self::Help => ConditionExpr::All(&WHEN_HELP),
            Self::Palette => ConditionExpr::All(&WHEN_PALETTE),
            Self::PaletteCommand => ConditionExpr::All(&WHEN_PALETTE_COMMAND),
            Self::PaletteSearch => ConditionExpr::All(&WHEN_PALETTE_SEARCH),
            Self::PaletteSearchResults => ConditionExpr::All(&WHEN_PALETTE_SEARCH_RESULTS),
            Self::PaletteHistory => ConditionExpr::All(&WHEN_PALETTE_HISTORY),
            Self::PaletteOutline => ConditionExpr::All(&WHEN_PALETTE_OUTLINE),
            Self::PaletteWithInputHistory => ConditionExpr::All(&WHEN_PALETTE_WITH_INPUT_HISTORY),
            Self::PaletteNoInputHistory => ConditionExpr::All(&WHEN_PALETTE_NO_INPUT_HISTORY),
        }
    }

    pub(crate) fn includes_palette(self) -> bool {
        matches!(
            self,
            Self::Palette
                | Self::PaletteCommand
                | Self::PaletteSearch
                | Self::PaletteSearchResults
                | Self::PaletteHistory
                | Self::PaletteOutline
                | Self::PaletteWithInputHistory
                | Self::PaletteNoInputHistory
        )
    }

    pub(crate) fn includes_help(self) -> bool {
        self == Self::Help
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeymapBinding {
    Exact {
        when: KeymapWhen,
        keys: Vec<ShortcutKey>,
        command: Command,
    },
    NumericPrefix {
        when: KeymapWhen,
        suffix: ShortcutKey,
        command_id: &'static str,
    },
    UnbindExact {
        when: KeymapWhen,
        keys: Vec<ShortcutKey>,
    },
    UnbindNumericPrefix {
        when: KeymapWhen,
        suffix: ShortcutKey,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeymapOptions {
    pub bindings: Vec<KeymapBinding>,
}

impl KeymapOptions {
    pub(crate) fn merge(mut self, next: Self) -> Self {
        self.bindings.extend(next.bindings);
        self
    }
}

pub(crate) fn resolve_sequence_registry(options: &KeymapOptions) -> SequenceRegistry {
    let mut registry = build_default_sequence_registry();

    for binding in &options.bindings {
        match binding {
            KeymapBinding::Exact {
                when,
                keys,
                command,
            } => {
                registry
                    .register_exact(when.condition(), keys, command.clone())
                    .expect("validated exact keymap binding should register");
            }
            KeymapBinding::NumericPrefix {
                when,
                suffix,
                command_id,
            } => {
                let factory = numeric_prefix_factory(command_id)
                    .expect("validated numeric prefix command should have a factory");
                registry
                    .register_numeric_prefix(when.condition(), command_id, *suffix, factory)
                    .expect("validated numeric keymap binding should register");
            }
            KeymapBinding::UnbindExact { when, keys } => {
                registry
                    .unregister_exact(when.condition(), keys)
                    .expect("validated exact keymap binding should unregister");
            }
            KeymapBinding::UnbindNumericPrefix { when, suffix } => {
                registry
                    .unregister_numeric_prefix(when.condition(), *suffix)
                    .expect("validated numeric keymap binding should unregister");
            }
        }
    }

    registry
}

pub(crate) fn parse_keymap_when(value: &str) -> AppResult<KeymapWhen> {
    KeymapWhen::parse(value).ok_or(AppError::invalid_argument("unknown keymap condition"))
}

pub(crate) fn parse_keymap_binding(
    when_text: &str,
    key_text: &str,
    command_text: Option<&str>,
) -> AppResult<KeymapBinding> {
    let when = parse_keymap_when(when_text)?;

    if let Some(suffix_text) = key_text.strip_prefix("[count]") {
        let suffix = parse_numeric_suffix(suffix_text)?;
        validate_numeric_suffix(suffix)?;
        let Some(command_text) = command_text else {
            return Ok(KeymapBinding::UnbindNumericPrefix { when, suffix });
        };
        let command_id = parse_numeric_prefix_command(command_text)?;
        validate_command_for_keymap_condition(command_id, when)?;
        return Ok(KeymapBinding::NumericPrefix {
            when,
            suffix,
            command_id,
        });
    }

    let keys = parse_exact_keys(key_text)?;
    let Some(command_text) = command_text else {
        return Ok(KeymapBinding::UnbindExact { when, keys });
    };
    let command = parse_command_text(command_text)?;
    validate_command_for_keymap_condition(first_token(command_text), when)?;
    Ok(KeymapBinding::Exact {
        when,
        keys,
        command,
    })
}

fn parse_exact_keys(value: &str) -> AppResult<Vec<ShortcutKey>> {
    let keys = parse_shortcut_sequence(value).map_err(|err| {
        AppError::invalid_argument(format!("invalid key sequence {value:?}: {err}"))
    })?;
    validate_exact_keys(&keys)?;
    Ok(keys)
}

fn parse_numeric_suffix(value: &str) -> AppResult<ShortcutKey> {
    let keys = parse_shortcut_sequence(value).map_err(|err| {
        AppError::invalid_argument(format!("invalid count key suffix {value:?}: {err}"))
    })?;
    let [suffix] = keys.as_slice() else {
        return Err(AppError::invalid_argument(
            "count key binding suffix must be exactly one key",
        ));
    };
    Ok(*suffix)
}

fn parse_numeric_prefix_command(command_text: &str) -> AppResult<&'static str> {
    let command_id = command_text.trim();
    if command_id.is_empty() || command_id.split_whitespace().count() != 1 {
        return Err(AppError::invalid_argument(
            "count key binding command must be a command id",
        ));
    }
    if numeric_prefix_factory(command_id).is_none() {
        return Err(AppError::invalid_argument(
            "count key binding currently supports only goto-page",
        ));
    }
    Ok("goto-page")
}

fn validate_command_for_keymap_condition(id: &str, when: KeymapWhen) -> AppResult<()> {
    let Some(spec) = find_command_spec(id) else {
        return Err(AppError::invalid_argument("unknown command id"));
    };
    if !matches!(
        spec.invocation,
        CommandInvocationPolicy::User | CommandInvocationPolicy::BindingOnly
    ) {
        return Err(AppError::invalid_argument(format!(
            "{} is an internal command and cannot be invoked directly",
            spec.id
        )));
    }

    match spec.target {
        CommandTargetRequirement::App => Ok(()),
        CommandTargetRequirement::ActivePalette if when.includes_palette() => Ok(()),
        CommandTargetRequirement::ActiveHelp if when.includes_help() => Ok(()),
        CommandTargetRequirement::ActivePalette => Err(AppError::invalid_argument(format!(
            "{} requires an active palette",
            spec.id
        ))),
        CommandTargetRequirement::ActiveHelp => Err(AppError::invalid_argument(format!(
            "{} requires active help",
            spec.id
        ))),
    }
}

fn validate_exact_keys(keys: &[ShortcutKey]) -> AppResult<()> {
    SequenceRegistry::new()
        .register_exact(ConditionExpr::Always, keys, Command::Quit)
        .map(|_| ())
        .map_err(key_registration_error)
}

fn validate_numeric_suffix(suffix: ShortcutKey) -> AppResult<()> {
    SequenceRegistry::new()
        .register_numeric_prefix(ConditionExpr::Always, "goto-page", suffix, |page| {
            Command::GotoPage { page }
        })
        .map(|_| ())
        .map_err(key_registration_error)
}

fn key_registration_error(err: SequenceRegistrationError) -> AppError {
    AppError::invalid_argument(match err {
        SequenceRegistrationError::EmptySequence => "key sequence must not be empty",
        SequenceRegistrationError::ReservedKeyInSequence => {
            "<esc> cannot be used inside multi-key bindings"
        }
        SequenceRegistrationError::InvalidNumericSuffix => {
            "count key binding suffix must not be a digit"
        }
        SequenceRegistrationError::ShiftCharBindingUnsupported => {
            "shifted character bindings must use the resulting character"
        }
    })
}

fn numeric_prefix_factory(command_id: &str) -> Option<fn(usize) -> Command> {
    match command_id {
        "goto-page" => Some(|page| Command::GotoPage { page }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::Mode;
    use crate::command::Command;
    use crate::condition::RuntimeConditionContext;
    use crate::extension::ExtensionUiSnapshot;
    use crate::input::sequence::{
        DEFAULT_SEQUENCE_TIMEOUT, KeyBindingContext, SequenceResolution, SequenceResolver,
    };
    use crate::input::shortcut::ShortcutKey;
    use crate::palette::PaletteKind;

    use super::{KeymapBinding, KeymapOptions, KeymapWhen, resolve_sequence_registry};

    #[test]
    fn configured_bindings_resolve_against_their_when_condition() {
        let registry = resolve_sequence_registry(&KeymapOptions {
            bindings: vec![
                KeymapBinding::UnbindExact {
                    when: KeymapWhen::Normal,
                    keys: vec![ShortcutKey::char('j')],
                },
                KeymapBinding::Exact {
                    when: KeymapWhen::Normal,
                    keys: vec![ShortcutKey::char('x')],
                    command: Command::NextPage,
                },
                KeymapBinding::Exact {
                    when: KeymapWhen::Help,
                    keys: vec![ShortcutKey::char('x')],
                    command: Command::HelpScrollDown,
                },
            ],
        });
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
        let extensions = ExtensionUiSnapshot::default();

        assert_eq!(
            resolver.handle_key_in_context(
                KeyBindingContext::normal(&extensions),
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            ),
            SequenceResolution::Noop
        );
        assert_eq!(
            resolver.handle_key_in_context(
                KeyBindingContext::normal(&extensions),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            ),
            SequenceResolution::Dispatch(Command::NextPage)
        );
        assert_eq!(
            resolver.handle_key_in_context(
                KeyBindingContext {
                    runtime: RuntimeConditionContext::new(Mode::Help, None, &extensions),
                },
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            ),
            SequenceResolution::Dispatch(Command::HelpScrollDown)
        );
    }

    #[test]
    fn palette_input_history_conditions_select_distinct_bindings() {
        let registry = resolve_sequence_registry(&KeymapOptions {
            bindings: vec![
                KeymapBinding::Exact {
                    when: KeymapWhen::PaletteWithInputHistory,
                    keys: vec![ShortcutKey::key(KeyCode::Up)],
                    command: Command::PaletteInputHistoryOlder,
                },
                KeymapBinding::Exact {
                    when: KeymapWhen::PaletteNoInputHistory,
                    keys: vec![ShortcutKey::key(KeyCode::Up)],
                    command: Command::PaletteSelectPrev,
                },
            ],
        });
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
        let extensions = ExtensionUiSnapshot::default();

        assert_eq!(
            resolver.handle_key_in_context(
                KeyBindingContext {
                    runtime: RuntimeConditionContext::new(
                        Mode::Palette,
                        Some(PaletteKind::Command),
                        &extensions,
                    ),
                },
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            ),
            SequenceResolution::Dispatch(Command::PaletteInputHistoryOlder)
        );
        assert_eq!(
            resolver.handle_key_in_context(
                KeyBindingContext {
                    runtime: RuntimeConditionContext::new(
                        Mode::Palette,
                        Some(PaletteKind::Outline),
                        &extensions,
                    ),
                },
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            ),
            SequenceResolution::Dispatch(Command::PaletteSelectPrev)
        );
    }
}
