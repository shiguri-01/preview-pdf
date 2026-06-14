use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Mode;
use crate::command::Command;
use crate::condition::{ConditionExpr, RuntimeConditionContext, evaluate_condition};
use crate::extension::ExtensionUiSnapshot;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBindingScope {
    Normal,
    Palette,
    Help,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyBindingContext<'a> {
    pub scope: KeyBindingScope,
    pub runtime: RuntimeConditionContext<'a>,
}

impl<'a> KeyBindingContext<'a> {
    pub fn normal(extensions: &'a ExtensionUiSnapshot) -> Self {
        Self {
            scope: KeyBindingScope::Normal,
            runtime: RuntimeConditionContext {
                mode: Mode::Normal,
                active_palette: None,
                focused_text_input: false,
                text_history_available: false,
                extensions,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedKeyMatcher {
    PrintableCharacter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedCommand {
    TextInsert,
}

impl GeneratedCommand {
    fn command_id(self) -> &'static str {
        match self {
            Self::TextInsert => "text.insert",
        }
    }
}

#[derive(Debug, Clone)]
enum SequenceBinding {
    Exact {
        scope: KeyBindingScope,
        enabled_when: ConditionExpr,
        keys: Vec<ShortcutKey>,
        command: Command,
    },
    NumericPrefix {
        scope: KeyBindingScope,
        enabled_when: ConditionExpr,
        suffix: ShortcutKey,
        command_id: &'static str,
        factory: NumericCommandFactory,
    },
    Generated {
        scope: KeyBindingScope,
        enabled_when: ConditionExpr,
        matcher: GeneratedKeyMatcher,
        command: GeneratedCommand,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactSequenceBinding {
    pub scope: KeyBindingScope,
    pub enabled_when: ConditionExpr,
    pub keys: Vec<ShortcutKey>,
    pub command_id: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumericSequenceBinding {
    pub scope: KeyBindingScope,
    pub enabled_when: ConditionExpr,
    pub suffix: ShortcutKey,
    pub command_id: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedSequenceBinding {
    pub scope: KeyBindingScope,
    pub enabled_when: ConditionExpr,
    pub matcher: GeneratedKeyMatcher,
    pub command_id: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SequenceRegistrySnapshot {
    pub exact_bindings: Vec<ExactSequenceBinding>,
    pub numeric_prefix_bindings: Vec<NumericSequenceBinding>,
    pub generated_bindings: Vec<GeneratedSequenceBinding>,
}

#[derive(Debug, Clone, Default)]
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
        self.register_static_in_scope(
            KeyBindingScope::Normal,
            ConditionExpr::Always,
            keys,
            command,
        )
    }

    pub fn register_static_in_scope(
        &mut self,
        scope: KeyBindingScope,
        enabled_when: ConditionExpr,
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

        self.bindings.retain(|binding| {
            !matches!(
                binding,
                SequenceBinding::Exact {
                    scope: existing_scope,
                    enabled_when: existing_enabled_when,
                    keys: existing,
                    ..
                } if *existing_scope == scope
                    && *existing_enabled_when == enabled_when
                    && existing == &keys
            )
        });
        self.bindings.push(SequenceBinding::Exact {
            scope,
            enabled_when,
            keys,
            command,
        });
        Ok(())
    }

    pub fn register_numeric_prefix(
        &mut self,
        command_id: &'static str,
        suffix: ShortcutKey,
        factory: NumericCommandFactory,
    ) -> Result<(), SequenceRegistrationError> {
        self.register_numeric_prefix_in_scope(
            KeyBindingScope::Normal,
            ConditionExpr::Always,
            command_id,
            suffix,
            factory,
        )
    }

    pub fn register_numeric_prefix_in_scope(
        &mut self,
        scope: KeyBindingScope,
        enabled_when: ConditionExpr,
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
            !matches!(
                binding,
                SequenceBinding::NumericPrefix {
                    scope: existing_scope,
                    enabled_when: existing_enabled_when,
                    suffix: existing,
                    ..
                } if *existing_scope == scope
                    && *existing_enabled_when == enabled_when
                    && *existing == suffix
            )
        });
        self.bindings.push(SequenceBinding::NumericPrefix {
            scope,
            enabled_when,
            suffix,
            command_id,
            factory,
        });
        Ok(())
    }

    pub fn register_generated(
        &mut self,
        scope: KeyBindingScope,
        enabled_when: ConditionExpr,
        matcher: GeneratedKeyMatcher,
        command: GeneratedCommand,
    ) {
        self.bindings.retain(|binding| {
            !matches!(
                binding,
                SequenceBinding::Generated {
                    scope: existing_scope,
                    enabled_when: existing_enabled_when,
                    matcher: existing_matcher,
                    ..
                } if *existing_scope == scope
                    && *existing_enabled_when == enabled_when
                    && *existing_matcher == matcher
            )
        });
        self.bindings.push(SequenceBinding::Generated {
            scope,
            enabled_when,
            matcher,
            command,
        });
    }

    pub fn unregister_static(
        &mut self,
        keys: &[ShortcutKey],
    ) -> Result<bool, SequenceRegistrationError> {
        if keys.is_empty() {
            return Err(SequenceRegistrationError::EmptySequence);
        }
        let keys = keys
            .iter()
            .copied()
            .map(canonicalize_binding_key)
            .collect::<Result<Vec<_>, _>>()?;
        let original_len = self.bindings.len();
        self.bindings.retain(|binding| {
            !matches!(
                binding,
                SequenceBinding::Exact {
                    scope: KeyBindingScope::Normal,
                    keys: existing,
                    ..
                } if existing == &keys
            )
        });
        Ok(self.bindings.len() != original_len)
    }

    pub fn unregister_numeric_prefix(
        &mut self,
        suffix: ShortcutKey,
    ) -> Result<bool, SequenceRegistrationError> {
        let suffix = canonicalize_binding_key(suffix)?;
        if is_digit_key(suffix) {
            return Err(SequenceRegistrationError::InvalidNumericSuffix);
        }
        let original_len = self.bindings.len();
        self.bindings.retain(|binding| {
            !matches!(
                binding,
                SequenceBinding::NumericPrefix {
                    scope: KeyBindingScope::Normal,
                    suffix: existing,
                    ..
                } if *existing == suffix
            )
        });
        Ok(self.bindings.len() != original_len)
    }

    pub fn snapshot(&self) -> SequenceRegistrySnapshot {
        let mut snapshot = SequenceRegistrySnapshot::default();
        for binding in &self.bindings {
            match binding {
                SequenceBinding::Exact {
                    scope,
                    enabled_when,
                    keys,
                    command,
                } => {
                    snapshot.exact_bindings.push(ExactSequenceBinding {
                        scope: *scope,
                        enabled_when: *enabled_when,
                        keys: keys.clone(),
                        command_id: command.id(),
                    });
                }
                SequenceBinding::NumericPrefix {
                    scope,
                    enabled_when,
                    suffix,
                    command_id,
                    ..
                } => snapshot
                    .numeric_prefix_bindings
                    .push(NumericSequenceBinding {
                        scope: *scope,
                        enabled_when: *enabled_when,
                        suffix: *suffix,
                        command_id,
                    }),
                SequenceBinding::Generated {
                    scope,
                    enabled_when,
                    matcher,
                    command,
                } => snapshot.generated_bindings.push(GeneratedSequenceBinding {
                    scope: *scope,
                    enabled_when: *enabled_when,
                    matcher: *matcher,
                    command_id: command.command_id(),
                }),
            }
        }
        snapshot
    }

    fn match_buffer(&self, buffer: &[ShortcutKey], ctx: KeyBindingContext<'_>) -> RegistryMatch {
        let mut exact = None;
        let mut has_prefix = false;

        for binding in &self.bindings {
            if !binding_matches_context(binding, ctx) {
                continue;
            }

            match binding {
                SequenceBinding::Exact { keys, command, .. } => {
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
                SequenceBinding::Generated {
                    matcher, command, ..
                } => {
                    if buffer.len() == 1
                        && let Some(command) =
                            generated_command_for_key(*matcher, *command, buffer[0], ctx)
                    {
                        exact = Some(command);
                    }
                }
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
    DispatchWithRedraw(Command),
    // When a pending sequence is committed before processing the latest key, the
    // old command must be emitted first and the new key must still be processed.
    DispatchThen {
        first: Command,
        next: Box<SequenceResolution>,
        redraw: bool,
    },
}

#[derive(Debug, Clone)]
struct SequenceState {
    buffer: Vec<ShortcutKey>,
    scope: Option<KeyBindingScope>,
    last_update: Option<Instant>,
    timeout: Duration,
}

impl SequenceState {
    fn new(timeout: Duration) -> Self {
        Self {
            buffer: Vec::new(),
            scope: None,
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

    fn push(&mut self, key: ShortcutKey, scope: KeyBindingScope) {
        self.buffer.push(key);
        self.scope = Some(scope);
        self.last_update = Some(Instant::now());
    }

    fn clear(&mut self) -> bool {
        let had_buffer = !self.buffer.is_empty();
        self.buffer.clear();
        self.scope = None;
        self.last_update = None;
        had_buffer
    }

    fn is_in_scope(&self, scope: KeyBindingScope) -> bool {
        self.scope
            .is_none_or(|pending_scope| pending_scope == scope)
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
        let extensions = ExtensionUiSnapshot::default();
        self.handle_key_in_context(KeyBindingContext::normal(&extensions), key)
    }

    pub fn handle_key_in_context(
        &mut self,
        ctx: KeyBindingContext<'_>,
        key: KeyEvent,
    ) -> SequenceResolution {
        if !self.state.is_empty() && !self.state.is_in_scope(ctx.scope) {
            self.state.clear();
        }

        match normalize_key(key) {
            Some(key) => self.handle_normalized_key(ctx, key),
            None if self.state.is_empty() => SequenceResolution::Noop,
            None if self.state.is_timed_out() => self.confirm_pending(),
            None => {
                self.state.clear();
                SequenceResolution::Cleared
            }
        }
    }

    fn handle_normalized_key(
        &mut self,
        ctx: KeyBindingContext<'_>,
        key: ShortcutKey,
    ) -> SequenceResolution {
        let pending_exact = (!self.state.is_empty())
            .then(|| self.registry.match_buffer(self.state.as_slice(), ctx).exact)
            .flatten();
        let had_pending = !self.state.is_empty();

        if had_pending {
            if self.state.is_timed_out() {
                // Timeout is checked again on the next key so a sequence still commits even
                // if no wake event arrived before the user continued typing.
                return match self.confirm_pending() {
                    SequenceResolution::Dispatch(command) => SequenceResolution::DispatchThen {
                        first: command,
                        next: Box::new(self.handle_normalized_key(ctx, key)),
                        redraw: true,
                    },
                    SequenceResolution::Cleared | SequenceResolution::Noop => {
                        self.handle_normalized_key(ctx, key)
                    }
                    SequenceResolution::Pending
                    | SequenceResolution::DispatchWithRedraw(_)
                    | SequenceResolution::DispatchThen { .. } => {
                        unreachable!(
                            "confirming a timed out sequence cannot return a derived resolution"
                        )
                    }
                };
            }

            if key.code() == KeyCode::Esc {
                self.state.clear();
                return SequenceResolution::Cleared;
            }
        }

        self.state.push(key, ctx.scope);
        self.resolve_pending_after_input(ctx, key, pending_exact)
    }

    pub fn flush_timeout(&mut self) -> SequenceResolution {
        if self.state.is_empty() || !self.state.is_timed_out() {
            return SequenceResolution::Noop;
        }

        let extensions = ExtensionUiSnapshot::default();
        let ctx = KeyBindingContext {
            scope: self.state.scope.unwrap_or(KeyBindingScope::Normal),
            ..KeyBindingContext::normal(&extensions)
        };
        match self.registry.match_buffer(self.state.as_slice(), ctx).exact {
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

    pub fn timeout(&self) -> Duration {
        self.state.timeout
    }

    pub fn pending_display(&self) -> Option<String> {
        (!self.state.is_empty()).then(|| format_pending_buffer(self.state.as_slice()))
    }

    pub fn has_pending(&self) -> bool {
        !self.state.is_empty()
    }

    fn confirm_pending(&mut self) -> SequenceResolution {
        let extensions = ExtensionUiSnapshot::default();
        let ctx = KeyBindingContext {
            scope: self.state.scope.unwrap_or(KeyBindingScope::Normal),
            ..KeyBindingContext::normal(&extensions)
        };
        match self.registry.match_buffer(self.state.as_slice(), ctx).exact {
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

    fn resolve_pending_after_input(
        &mut self,
        ctx: KeyBindingContext<'_>,
        latest_key: ShortcutKey,
        pending_exact: Option<Command>,
    ) -> SequenceResolution {
        let matched = self.registry.match_buffer(self.state.as_slice(), ctx);
        match (matched.exact, matched.has_prefix) {
            (Some(command), false) => {
                self.state.clear();
                SequenceResolution::Dispatch(command)
            }
            (Some(_), true) | (None, true) => SequenceResolution::Pending,
            (None, false) => {
                let had_pending = self.state.as_slice().len() > 1;
                self.state.clear();
                if !had_pending {
                    return SequenceResolution::Noop;
                }

                if let Some(command) = pending_exact {
                    if command_stops_followup_reprocessing(&command) {
                        return SequenceResolution::Dispatch(command);
                    }

                    SequenceResolution::DispatchThen {
                        first: command,
                        next: Box::new(self.handle_normalized_key(ctx, latest_key)),
                        redraw: true,
                    }
                } else {
                    match self.handle_normalized_key(ctx, latest_key) {
                        SequenceResolution::Noop => SequenceResolution::Cleared,
                        SequenceResolution::Dispatch(command) => {
                            SequenceResolution::DispatchWithRedraw(command)
                        }
                        next => next,
                    }
                }
            }
        }
    }
}

fn command_stops_followup_reprocessing(command: &Command) -> bool {
    matches!(
        command,
        Command::OpenHelp
            | Command::CloseHelp
            | Command::OpenPalette { .. }
            | Command::ClosePalette
            | Command::OpenSearch
            | Command::OpenSearchResults
            | Command::OpenHistory
            | Command::OpenOutline
    )
}

fn binding_matches_context(binding: &SequenceBinding, ctx: KeyBindingContext<'_>) -> bool {
    let (scope, enabled_when) = match binding {
        SequenceBinding::Exact {
            scope,
            enabled_when,
            ..
        }
        | SequenceBinding::NumericPrefix {
            scope,
            enabled_when,
            ..
        }
        | SequenceBinding::Generated {
            scope,
            enabled_when,
            ..
        } => (*scope, *enabled_when),
    };

    scope == ctx.scope && evaluate_condition(enabled_when, &ctx.runtime)
}

fn generated_command_for_key(
    matcher: GeneratedKeyMatcher,
    command: GeneratedCommand,
    key: ShortcutKey,
    _ctx: KeyBindingContext<'_>,
) -> Option<Command> {
    match (matcher, command) {
        (GeneratedKeyMatcher::PrintableCharacter, GeneratedCommand::TextInsert) => {
            if key
                .modifiers()
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                return None;
            }

            match key.code() {
                KeyCode::Char(ch) => Some(Command::TextInsert {
                    text: ch.to_string(),
                }),
                _ => None,
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
    matches!(key.code(), KeyCode::Esc)
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
        DEFAULT_SEQUENCE_TIMEOUT, GeneratedCommand, GeneratedKeyMatcher, KeyBindingContext,
        KeyBindingScope, SequenceRegistrationError, SequenceRegistry, SequenceResolution,
        SequenceResolver,
    };
    use crate::app::Mode;
    use crate::command::Command;
    use crate::condition::{ConditionExpr, RuntimeCondition, RuntimeConditionContext};
    use crate::extension::ExtensionUiSnapshot;
    use crate::input::shortcut::ShortcutKey;
    use crate::palette::PaletteKind;
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
    fn ambiguous_exact_waits_for_timeout() {
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
    fn enter_dispatches_as_a_regular_exact_binding() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::key(KeyCode::Enter)], Command::NextPage)
            .expect("enter binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        let resolution = resolver.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(resolution, SequenceResolution::Dispatch(Command::NextPage));
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn scoped_bindings_resolve_only_in_matching_scope() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static_in_scope(
                KeyBindingScope::Palette,
                ConditionExpr::Always,
                &[ShortcutKey::key(KeyCode::Enter)],
                Command::PaletteSubmit,
            )
            .expect("palette binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
        let extensions = ExtensionUiSnapshot::default();

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            SequenceResolution::Noop
        );
        assert_eq!(
            resolver.handle_key_in_context(
                palette_context(&extensions),
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            ),
            SequenceResolution::Dispatch(Command::PaletteSubmit)
        );
    }

    #[test]
    fn generated_printable_binding_builds_text_insert_command() {
        let mut registry = SequenceRegistry::new();
        registry.register_generated(
            KeyBindingScope::Palette,
            ConditionExpr::All(&[RuntimeCondition::TextInputIsFocused]),
            GeneratedKeyMatcher::PrintableCharacter,
            GeneratedCommand::TextInsert,
        );
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
        let extensions = ExtensionUiSnapshot::default();

        assert_eq!(
            resolver.handle_key_in_context(
                palette_context(&extensions),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
            ),
            SequenceResolution::Dispatch(Command::TextInsert {
                text: "x".to_string(),
            })
        );
    }

    fn palette_context<'a>(extensions: &'a ExtensionUiSnapshot) -> KeyBindingContext<'a> {
        KeyBindingContext {
            scope: KeyBindingScope::Palette,
            runtime: RuntimeConditionContext {
                mode: Mode::Palette,
                active_palette: Some(PaletteKind::Command),
                focused_text_input: true,
                text_history_available: true,
                extensions,
            },
        }
    }

    #[test]
    fn enter_after_pending_exact_match_reprocesses_as_next_key() {
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
            .register_static(&[ShortcutKey::key(KeyCode::Enter)], Command::NextPage)
            .expect("enter binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let resolution = resolver.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            resolution,
            SequenceResolution::DispatchThen {
                first: Command::FirstPage,
                next: Box::new(SequenceResolution::Dispatch(Command::NextPage)),
                redraw: true,
            }
        );
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
                redraw: true,
            }
        );
    }

    #[test]
    fn enter_can_be_part_of_a_multi_key_binding() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::key(KeyCode::Enter)],
                Command::FirstPage,
            )
            .expect("enter should be usable in multi-key bindings");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let resolution = resolver.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(resolution, SequenceResolution::Dispatch(Command::FirstPage));
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn mismatch_after_pending_exact_match_reprocesses_latest_key() {
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
            .register_static(&[ShortcutKey::char('x')], Command::NextPage)
            .expect("single-key binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let mismatch = resolver.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            mismatch,
            SequenceResolution::DispatchThen {
                first: Command::FirstPage,
                next: Box::new(SequenceResolution::Dispatch(Command::NextPage)),
                redraw: true,
            }
        );
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn mismatch_after_pending_prefix_reprocesses_latest_key() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::char('g')],
                Command::FirstPage,
            )
            .expect("multi-key binding should register");
        registry
            .register_static(&[ShortcutKey::char('x')], Command::NextPage)
            .expect("single-key binding should register");
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);

        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            SequenceResolution::Pending
        );

        let mismatch = resolver.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            mismatch,
            SequenceResolution::DispatchWithRedraw(Command::NextPage)
        );
        assert_eq!(resolver.pending_display(), None);
    }

    #[test]
    fn mismatch_after_pending_prefix_reports_clear_when_latest_key_is_unbound() {
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
    fn unregister_static_removes_exact_binding() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('j')], Command::NextPage)
            .expect("binding should register");

        assert!(
            registry
                .unregister_static(&[ShortcutKey::char('j')])
                .expect("binding should unregister")
        );
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            SequenceResolution::Noop
        );
    }

    #[test]
    fn unregister_numeric_prefix_removes_count_binding() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_numeric_prefix("goto-page", ShortcutKey::char('G'), |page| {
                Command::GotoPage { page }
            })
            .expect("binding should register");

        assert!(
            registry
                .unregister_numeric_prefix(ShortcutKey::char('G'))
                .expect("binding should unregister")
        );
        let mut resolver = SequenceResolver::new(registry, DEFAULT_SEQUENCE_TIMEOUT);
        assert_eq!(
            resolver.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)),
            SequenceResolution::Noop
        );
    }

    #[test]
    fn registry_rejects_escape_in_multikey_sequences() {
        let mut registry = SequenceRegistry::new();

        let error = registry
            .register_static(
                &[ShortcutKey::char('g'), ShortcutKey::key(KeyCode::Esc)],
                Command::FirstPage,
            )
            .expect_err("Esc should be reserved in multi-key bindings");
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

        let resolution =
            resolver.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::SUPER));

        assert_eq!(resolution, SequenceResolution::Cleared);
        assert_eq!(resolver.pending_display(), None);
    }
}
