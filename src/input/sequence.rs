use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::command::Command;

use super::shortcut::{ShortcutKey, format_shortcut_key};

pub const DEFAULT_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceRegistrationError {
    EmptySequence,
    ReservedKeyInSequence,
    InvalidNumericSuffix,
    ShiftCharBindingUnsupported,
}

type NumericCommandFactory = fn(usize) -> Command;

#[derive(Clone)]
enum SequenceBinding {
    Exact {
        keys: Vec<ShortcutKey>,
        command: Command,
    },
    NumericPrefix {
        suffix: ShortcutKey,
        command_id: &'static str,
        factory: NumericCommandFactory,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactSequenceBinding {
    pub keys: Vec<ShortcutKey>,
    pub command_id: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumericSequenceBinding {
    pub suffix: ShortcutKey,
    pub command_id: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SequenceRegistrySnapshot {
    pub exact_bindings: Vec<ExactSequenceBinding>,
    pub numeric_prefix_bindings: Vec<NumericSequenceBinding>,
}

#[derive(Clone, Default)]
pub struct SequenceRegistry {
    bindings: Vec<SequenceBinding>,
}

impl SequenceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_static(
        &mut self,
        keys: &[ShortcutKey],
        command: Command,
    ) -> Result<(), SequenceRegistrationError> {
        if keys.is_empty() {
            return Err(SequenceRegistrationError::EmptySequence);
        }
        if keys.len() > 1 && keys.iter().copied().any(is_reserved_sequence_key) {
            return Err(SequenceRegistrationError::ReservedKeyInSequence);
        }
        let keys = keys
            .iter()
            .copied()
            .map(canonicalize_binding_key)
            .collect::<Result<Vec<_>, _>>()?;

        self.bindings
            .retain(|binding| !matches!(binding, SequenceBinding::Exact { keys: existing, .. } if existing == &keys));
        self.bindings.push(SequenceBinding::Exact { keys, command });
        Ok(())
    }

    pub fn register_numeric_prefix(
        &mut self,
        command_id: &'static str,
        suffix: ShortcutKey,
        factory: NumericCommandFactory,
    ) -> Result<(), SequenceRegistrationError> {
        let suffix = canonicalize_binding_key(suffix)?;
        if is_reserved_sequence_key(suffix) {
            return Err(SequenceRegistrationError::ReservedKeyInSequence);
        }
        if is_digit_key(suffix) {
            return Err(SequenceRegistrationError::InvalidNumericSuffix);
        }

        self.bindings.retain(|binding| {
            !matches!(binding, SequenceBinding::NumericPrefix { suffix: existing, .. } if *existing == suffix)
        });
        self.bindings.push(SequenceBinding::NumericPrefix {
            suffix,
            command_id,
            factory,
        });
        Ok(())
    }

    pub fn snapshot(&self) -> SequenceRegistrySnapshot {
        let mut snapshot = SequenceRegistrySnapshot::default();
        for binding in &self.bindings {
            match binding {
                SequenceBinding::Exact { keys, command } => {
                    snapshot.exact_bindings.push(ExactSequenceBinding {
                        keys: keys.clone(),
                        command_id: command.id(),
                    });
                }
                SequenceBinding::NumericPrefix {
                    suffix, command_id, ..
                } => snapshot
                    .numeric_prefix_bindings
                    .push(NumericSequenceBinding {
                        suffix: *suffix,
                        command_id,
                    }),
            }
        }
        snapshot
    }

    fn match_buffer(&self, buffer: &[ShortcutKey]) -> RegistryMatch {
        let mut exact = None;
        let mut has_prefix = false;

        for binding in &self.bindings {
            match binding {
                SequenceBinding::Exact { keys, command } => {
                    if keys.as_slice() == buffer {
                        exact = Some(command.clone());
                    } else if keys.starts_with(buffer) {
                        has_prefix = true;
                    }
                }
                SequenceBinding::NumericPrefix {
                    suffix, factory, ..
                } => match match_numeric_prefix(buffer, *suffix, *factory) {
                    NumericMatch::None => {}
                    NumericMatch::Prefix => has_prefix = true,
                    NumericMatch::Exact(command) => exact = Some(command),
                },
            }
        }

        RegistryMatch { exact, has_prefix }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SequenceResolution {
    Noop,
    Pending,
    Cleared,
    Dispatch(Command),
    // When a timed-out prefix is confirmed by the next key press, the old command
    // must be emitted first and the new key must still be processed immediately.
    DispatchThen {
        first: Command,
        next: Box<SequenceResolution>,
    },
}

#[derive(Debug, Clone)]
struct SequenceState {
    buffer: Vec<ShortcutKey>,
    last_update: Option<Instant>,
    timeout: Duration,
}

impl SequenceState {
    fn new(timeout: Duration) -> Self {
        Self {
            buffer: Vec::new(),
            last_update: None,
            timeout,
        }
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn as_slice(&self) -> &[ShortcutKey] {
        &self.buffer
    }

    fn push(&mut self, key: ShortcutKey) {
        self.buffer.push(key);
        self.last_update = Some(Instant::now());
    }

    fn clear(&mut self) -> bool {
        let had_buffer = !self.buffer.is_empty();
        self.buffer.clear();
        self.last_update = None;
        had_buffer
    }

    fn is_timed_out(&self) -> bool {
        self.last_update
            .is_some_and(|last_update| last_update.elapsed() >= self.timeout)
    }
}

#[derive(Clone)]
pub struct SequenceResolver {
    registry: SequenceRegistry,
    state: SequenceState,
}

impl SequenceResolver {
    pub fn new(registry: SequenceRegistry, timeout: Duration) -> Self {
        Self {
            registry,
            state: SequenceState::new(timeout),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SequenceResolution {
        match normalize_key(key) {
            Some(key) => self.handle_normalized_key(key),
            None => SequenceResolution::Noop,
        }
    }

    fn handle_normalized_key(&mut self, key: ShortcutKey) -> SequenceResolution {
        let had_pending = !self.state.is_empty();

        if had_pending {
            if self.state.is_timed_out() {
                // Timeout is checked again on the next key so a sequence still commits even
                // if no wake event arrived before the user continued typing.
                return match self.confirm_pending() {
                    SequenceResolution::Dispatch(command) => SequenceResolution::DispatchThen {
                        first: command,
                        next: Box::new(self.handle_normalized_key(key)),
                    },
                    SequenceResolution::Cleared | SequenceResolution::Noop => {
                        self.handle_normalized_key(key)
                    }
                    SequenceResolution::Pending | SequenceResolution::DispatchThen { .. } => {
                        unreachable!("confirming a timed out sequence cannot remain pending")
                    }
                };
            }

            if key.code() == KeyCode::Esc {
                self.state.clear();
                return SequenceResolution::Cleared;
            }
            if key.code() == KeyCode::Enter {
                return self.confirm_pending();
            }
        }

        self.state.push(key);
        self.resolve_pending_after_input(had_pending)
    }

    pub fn flush_timeout(&mut self) -> SequenceResolution {
        if self.state.is_empty() || !self.state.is_timed_out() {
            return SequenceResolution::Noop;
        }

        match self.registry.match_buffer(self.state.as_slice()).exact {
            Some(command) => {
                self.state.clear();
                SequenceResolution::Dispatch(command)
            }
            None => {
                self.state.clear();
                SequenceResolution::Cleared
            }
        }
    }

    pub fn clear(&mut self) -> bool {
        self.state.clear()
    }

    pub fn snapshot(&self) -> SequenceRegistrySnapshot {
        self.registry.snapshot()
    }

    pub fn pending_display(&self) -> Option<String> {
        (!self.state.is_empty()).then(|| format_pending_buffer(self.state.as_slice()))
    }

    pub fn has_pending(&self) -> bool {
        !self.state.is_empty()
    }

    fn confirm_pending(&mut self) -> SequenceResolution {
        match self.registry.match_buffer(self.state.as_slice()).exact {
            Some(command) => {
                self.state.clear();
                SequenceResolution::Dispatch(command)
            }
            None => {
                self.state.clear();
                SequenceResolution::Cleared
            }
        }
    }

    fn resolve_pending_after_input(&mut self, had_pending: bool) -> SequenceResolution {
        let matched = self.registry.match_buffer(self.state.as_slice());
        match (matched.exact, matched.has_prefix) {
            (Some(command), false) => {
                self.state.clear();
                SequenceResolution::Dispatch(command)
            }
            (Some(_), true) | (None, true) => SequenceResolution::Pending,
            (None, false) => {
                self.state.clear();
                if had_pending {
                    SequenceResolution::Cleared
                } else {
                    SequenceResolution::Noop
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RegistryMatch {
    exact: Option<Command>,
    has_prefix: bool,
}

enum NumericMatch {
    None,
    Prefix,
    Exact(Command),
}

fn match_numeric_prefix(
    buffer: &[ShortcutKey],
    suffix: ShortcutKey,
    factory: NumericCommandFactory,
) -> NumericMatch {
    let mut digits = String::new();
    let mut index = 0;

    while let Some(key) = buffer.get(index) {
        if let KeyCode::Char(ch) = key.code()
            && key.modifiers() == KeyModifiers::NONE
            && ch.is_ascii_digit()
        {
            digits.push(ch);
            index += 1;
            continue;
        }
        break;
    }

    if digits.is_empty() {
        return NumericMatch::None;
    }

    if index == buffer.len() {
        return NumericMatch::Prefix;
    }

    if index + 1 != buffer.len() || buffer[index] != suffix {
        return NumericMatch::None;
    }

    match digits.parse::<usize>() {
        Ok(number) => NumericMatch::Exact(factory(number)),
        Err(_) => NumericMatch::None,
    }
}

fn normalize_key(key: KeyEvent) -> Option<ShortcutKey> {
    ShortcutKey::try_new(key.code, key.modifiers)
        .ok()
        .map(normalize_shortcut_key)
}

fn canonicalize_binding_key(key: ShortcutKey) -> Result<ShortcutKey, SequenceRegistrationError> {
    if matches!(key.code(), KeyCode::Char(_))
        && key.modifiers().contains(KeyModifiers::SHIFT)
        && !key
            .modifiers()
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return Err(SequenceRegistrationError::ShiftCharBindingUnsupported);
    }

    Ok(normalize_shortcut_key(key))
}

fn normalize_shortcut_key(key: ShortcutKey) -> ShortcutKey {
    match key.code() {
        KeyCode::Char(ch) => {
            let mut modifiers = key.modifiers();
            if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                modifiers.remove(KeyModifiers::SHIFT);
            }

            let normalized_char = if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && ch.is_ascii_alphabetic()
            {
                ch.to_ascii_lowercase()
            } else {
                ch
            };

            ShortcutKey::new(KeyCode::Char(normalized_char), modifiers)
        }
        _ => ShortcutKey::new(key.code(), key.modifiers()),
    }
}

fn is_reserved_sequence_key(key: ShortcutKey) -> bool {
    matches!(key.code(), KeyCode::Enter | KeyCode::Esc)
}

fn is_digit_key(key: ShortcutKey) -> bool {
    matches!(key.code(), KeyCode::Char(ch) if key.modifiers() == KeyModifiers::NONE && ch.is_ascii_digit())
}

fn format_pending_buffer(buffer: &[ShortcutKey]) -> String {
    let mut formatted = String::new();
    let mut previous_was_plain_char = false;

    for key in buffer {
        let (part, is_plain_char) = match key.code() {
            KeyCode::Char(ch) if key.modifiers() == KeyModifiers::NONE => (ch.to_string(), true),
            _ => (format_shortcut_key(*key), false),
        };

        if !(formatted.is_empty() || previous_was_plain_char && is_plain_char) {
            formatted.push(' ');
        }
        formatted.push_str(&part);
        previous_was_plain_char = is_plain_char;
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SEQUENCE_TIMEOUT, SequenceRegistrationError, SequenceRegistry, SequenceResolution,
        SequenceResolver,
    };
    use crate::command::Command;
    use crate::input::shortcut::ShortcutKey;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::Duration;

    #[test]
    fn exact_single_key_dispatches_immediately() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
            .expect("single-key binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let resolution = resolver.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        assert_eq!(resolution, SequenceResolution::Dispatch(Command::NextPage));
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn ambiguous_exact_waits_for_timeout_or_confirmation() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut resolver = SequenceResolver::new(registry, Duration::ZERO);

        let first = resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(first, SequenceResolution::Pending);
        assert_eq!(resolver.pending_display().as_deref(), Some("g"));

        let timeout = resolver.flush_timeout();
        assert_eq!(timeout, SequenceResolution::Dispatch(Command::FirstPage));
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn enter_confirms_pending_exact_match() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let confirm = resolver.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(confirm, SequenceResolution::Dispatch(Command::FirstPage));
    }

    #[test]
    fn expired_pending_sequence_dispatches_before_consuming_next_key() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('g')], Command::FirstPage)
            .expect("single-key binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::LastPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
            .expect("single-key binding should register");
        let mut resolver = SequenceResolver::new(registry, Duration::ZERO);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let resolution = resolver.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            resolution,
            SequenceResolution::DispatchThen {
                first: Command::FirstPage,
                next: Box::new(SequenceResolution::Dispatch(Command::NextPage)),
            }
        );
    }

    #[test]
    fn mismatch_clears_pending_buffer_without_retrying_latest_key() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let mismatch = resolver.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(mismatch, SequenceResolution::Cleared);
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn numeric_prefix_dispatches_and_formats_pending_digits() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_numeric_prefix("goto-page", ShortcutKey::char('G'), |page| {
                Command::GotoPage { page }
            })
            .expect("numeric prefix binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );
        assert_eq!(resolver.pending_display().as_deref(), Some("42"));

        let dispatch = resolver.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
        assert_eq!(
            dispatch,
            SequenceResolution::Dispatch(Command::GotoPage { page: 42 })
        );
    }

    #[test]
    fn non_digit_exact_binding_dispatches_immediately_alongside_numeric_prefix() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('=')], Command::ZoomReset)
            .expect("exact binding should register");
        registry
            .register_numeric_prefix("goto-page", ShortcutKey::char('G'), |page| {
                Command::GotoPage { page }
            })
            .expect("numeric prefix binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let reset = resolver.handle_key(KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE));
        assert_eq!(reset, SequenceResolution::Dispatch(Command::ZoomReset));
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn registry_rejects_reserved_keys_in_multikey_sequences() {
        let mut registry = SequenceRegistry::new();

        let error = registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::key(KeyCode::Enter)],
                Command::FirstPage,
            )
            .expect_err("Enter should be reserved in multi-key bindings");
        assert_eq!(error, SequenceRegistrationError::ReservedKeyInSequence);
    }

    #[test]
    fn registry_rejects_digit_numeric_suffixes() {
        let mut registry = SequenceRegistry::new();

        let error = registry
            .register_numeric_prefix("goto-page", ShortcutKey::char('5'), |page| {
                Command::GotoPage { page }
            })
            .expect_err("digit suffix should be rejected");
        assert_eq!(error, SequenceRegistrationError::InvalidNumericSuffix);
    }

    #[test]
    fn snapshot_includes_exact_and_numeric_bindings() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
            .expect("single-key binding should register");
        registry
            .register_numeric_prefix("goto-page", ShortcutKey::char('G'), |page| {
                Command::GotoPage { page }
            })
            .expect("numeric prefix binding should register");

        let snapshot = registry.snapshot();

        assert_eq!(snapshot.exact_bindings.len(), 1);
        assert_eq!(snapshot.exact_bindings[0].command_id, "next-page");
        assert_eq!(
            snapshot.exact_bindings[0].keys,
            vec![ShortcutKey::char('j')]
        );
        assert_eq!(snapshot.numeric_prefix_bindings.len(), 1);
        assert_eq!(snapshot.numeric_prefix_bindings[0].command_id, "goto-page");
        assert_eq!(
            snapshot.numeric_prefix_bindings[0].suffix,
            ShortcutKey::char('G')
        );
    }

    #[test]
    fn registry_rejects_shift_modified_printable_char_bindings() {
        let mut registry = SequenceRegistry::new();

        let error = registry
            .register_static(
                &[ShortcutKey::new(KeyCode::Char('a'), KeyModifiers::SHIFT)],
                Command::NextPage,
            )
            .expect_err("Shift+Char bindings should be rejected");
        assert_eq!(
            error,
            SequenceRegistrationError::ShiftCharBindingUnsupported
        );
    }

    #[test]
    fn registry_canonicalizes_ctrl_shift_letters_to_ctrl_letters() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
                &[ShortcutKey::new(
                    KeyCode::Char('O'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                )],
                Command::HistoryBack,
            )
            .expect("Ctrl+Shift+letter should normalize to Ctrl+letter");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let resolution =
            resolver.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(
            resolution,
            SequenceResolution::Dispatch(Command::HistoryBack)
        );
    }

    #[test]
    fn unsupported_modifier_input_is_ignored() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('k')], Command::PrevPage)
            .expect("single-key binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let resolution =
            resolver.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::SUPER));

        assert_eq!(resolution, SequenceResolution::Noop);
        assert_eq!(resolver.pending_display(), None);
    }
}
