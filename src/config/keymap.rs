use crate::command::{
    Command, CommandInvocationSource, parse_command_text,
    validate_command_id_invocation_for_source, validate_command_invocation_for_source,
};
use crate::error::{AppError, AppResult};
use crate::input::keymap::build_builtin_sequence_registry;
use crate::input::sequence::{SequenceRegistrationError, SequenceRegistry};
use crate::input::shortcut::{ShortcutKey, parse_shortcut_sequence};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapTarget {
    Exact(Vec<ShortcutKey>),
    NumericPrefix(ShortcutKey),
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeymapBinding {
    Exact {
        keys: Vec<ShortcutKey>,
        command: Command,
    },
    NumericPrefix {
        suffix: ShortcutKey,
        command_id: &'static str,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeymapOptions {
    pub preset: Option<KeymapPreset>,
    pub unbind: Vec<KeymapTarget>,
    pub bindings: Vec<KeymapBinding>,
}

impl KeymapOptions {
    pub(crate) fn merge(mut self, next: Self) -> Self {
        self.preset = next.preset.or(self.preset);
        self.unbind.extend(next.unbind);
        self.bindings.extend(next.bindings);
        self
    }
}

pub(crate) fn resolve_sequence_registry(options: &KeymapOptions) -> SequenceRegistry {
    let mut registry = match options.preset.unwrap_or(KeymapPreset::Default) {
        KeymapPreset::Default => build_builtin_sequence_registry(),
        KeymapPreset::None => SequenceRegistry::new(),
    };

    for target in &options.unbind {
        match target {
            KeymapTarget::Exact(keys) => {
                registry
                    .unregister_static(keys)
                    .expect("validated exact keymap target should unregister");
            }
            KeymapTarget::NumericPrefix(suffix) => {
                registry
                    .unregister_numeric_prefix(*suffix)
                    .expect("validated numeric keymap target should unregister");
            }
        }
    }

    for binding in &options.bindings {
        match binding {
            KeymapBinding::Exact { keys, command } => {
                registry
                    .register_static(keys, command.clone())
                    .expect("validated exact keymap binding should register");
            }
            KeymapBinding::NumericPrefix { suffix, command_id } => {
                let factory = numeric_prefix_factory(command_id)
                    .expect("validated numeric prefix command should have a factory");
                registry
                    .register_numeric_prefix(command_id, *suffix, factory)
                    .expect("validated numeric keymap binding should register");
            }
        }
    }

    registry
}

pub(crate) fn parse_keymap_preset(value: &str) -> AppResult<KeymapPreset> {
    KeymapPreset::parse(value).ok_or(AppError::invalid_argument("unknown keymap preset"))
}

pub(crate) fn parse_keymap_target(value: &str) -> AppResult<KeymapTarget> {
    if let Some(suffix_text) = value.strip_prefix("[count]") {
        let suffix = parse_numeric_suffix(suffix_text)?;
        validate_configurable_key(&[suffix])?;
        validate_numeric_suffix(suffix)?;
        return Ok(KeymapTarget::NumericPrefix(suffix));
    }

    let keys = parse_exact_keys(value)?;
    Ok(KeymapTarget::Exact(keys))
}

pub(crate) fn parse_keymap_binding(key: &str, command_text: &str) -> AppResult<KeymapBinding> {
    match parse_keymap_target(key)? {
        KeymapTarget::Exact(keys) => {
            let command = parse_command_text(command_text)?;
            validate_command_invocation_for_source(&command, CommandInvocationSource::Keymap)?;
            validate_exact_keys(&keys)?;
            Ok(KeymapBinding::Exact { keys, command })
        }
        KeymapTarget::NumericPrefix(suffix) => {
            let command_id = parse_numeric_prefix_command(command_text)?;
            validate_command_id_invocation_for_source(command_id, CommandInvocationSource::Keymap)?;
            validate_numeric_suffix(suffix)?;
            Ok(KeymapBinding::NumericPrefix { suffix, command_id })
        }
    }
}

fn parse_exact_keys(value: &str) -> AppResult<Vec<ShortcutKey>> {
    let keys = parse_shortcut_sequence(value).map_err(|err| {
        AppError::invalid_argument(format!("invalid key sequence {value:?}: {err}"))
    })?;
    validate_configurable_key(&keys)?;
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

fn validate_configurable_key(keys: &[ShortcutKey]) -> AppResult<()> {
    if keys.iter().any(|key| {
        matches!(
            key.code(),
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Enter
        )
    }) {
        return Err(AppError::invalid_argument(
            "keymap bindings cannot use <esc> or <enter>",
        ));
    }
    Ok(())
}

fn validate_exact_keys(keys: &[ShortcutKey]) -> AppResult<()> {
    SequenceRegistry::new()
        .register_static(keys, Command::Quit)
        .map(|_| ())
        .map_err(key_registration_error)
}

fn validate_numeric_suffix(suffix: ShortcutKey) -> AppResult<()> {
    SequenceRegistry::new()
        .register_numeric_prefix("goto-page", suffix, |page| Command::GotoPage { page })
        .map(|_| ())
        .map_err(key_registration_error)
}

fn key_registration_error(err: SequenceRegistrationError) -> AppError {
    AppError::invalid_argument(match err {
        SequenceRegistrationError::EmptySequence => "key sequence must not be empty",
        SequenceRegistrationError::ReservedKeyInSequence => {
            "<esc> and <enter> cannot be used inside multi-key bindings"
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
