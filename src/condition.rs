use crate::app::Mode;
use crate::extension::ExtensionUiSnapshot;
use crate::palette::PaletteKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionExpr {
    Always,
    All(&'static [RuntimeCondition]),
    Any(&'static [RuntimeCondition]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingConditionKind {
    Always,
    All,
    Any,
}

#[derive(Debug, Clone, Eq)]
pub struct BindingCondition {
    original: ConditionExpr,
    kind: BindingConditionKind,
    alternatives: Vec<Vec<RuntimeCondition>>,
}

impl PartialEq for BindingCondition {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.alternatives == other.alternatives
    }
}

impl BindingCondition {
    pub fn new(expr: ConditionExpr) -> Self {
        match expr {
            ConditionExpr::Always => Self {
                original: expr,
                kind: BindingConditionKind::Always,
                alternatives: Vec::new(),
            },
            ConditionExpr::All(conditions) => {
                let mut normalized = Vec::new();
                for condition in conditions {
                    add_condition(&mut normalized, *condition);
                }
                normalized.sort_by_key(|condition| condition_key(*condition));
                Self {
                    original: expr,
                    kind: BindingConditionKind::All,
                    alternatives: vec![normalized],
                }
            }
            ConditionExpr::Any(conditions) => {
                let mut alternatives = Vec::new();
                for condition in conditions {
                    let mut normalized = Vec::new();
                    add_condition(&mut normalized, *condition);
                    normalized.sort_by_key(|condition| condition_key(*condition));
                    if !alternatives.contains(&normalized) {
                        alternatives.push(normalized);
                    }
                }
                alternatives.sort_by_key(|conditions| {
                    conditions
                        .iter()
                        .copied()
                        .map(condition_key)
                        .collect::<Vec<_>>()
                });
                Self {
                    original: expr,
                    kind: BindingConditionKind::Any,
                    alternatives,
                }
            }
        }
    }

    pub fn original(&self) -> ConditionExpr {
        self.original
    }

    pub fn priority_score(&self) -> u16 {
        match self.kind {
            BindingConditionKind::Always => 0,
            BindingConditionKind::All => self
                .alternatives
                .first()
                .map(|conditions| {
                    conditions
                        .iter()
                        .copied()
                        .map(condition_weight)
                        .sum::<u16>()
                })
                .unwrap_or(0),
            BindingConditionKind::Any => self
                .alternatives
                .iter()
                .map(|conditions| {
                    conditions
                        .iter()
                        .copied()
                        .map(condition_weight)
                        .sum::<u16>()
                })
                .max()
                .unwrap_or(0),
        }
    }

    pub fn is_met(&self, ctx: &RuntimeConditionContext<'_>) -> bool {
        match self.kind {
            BindingConditionKind::Always => true,
            BindingConditionKind::All => self.alternatives.first().is_some_and(|conditions| {
                conditions
                    .iter()
                    .copied()
                    .all(|condition| runtime_condition_is_met(condition, ctx))
            }),
            BindingConditionKind::Any => self.alternatives.iter().any(|conditions| {
                conditions
                    .iter()
                    .copied()
                    .all(|condition| runtime_condition_is_met(condition, ctx))
            }),
        }
    }
}

/// Shared runtime predicates for command `enabled_when` and key binding
/// `enabled_when`.
///
/// Conditions describe runtime state, not command target existence. Prefer
/// command target requirements for mandatory receivers such as an active
/// palette or active help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCondition {
    ModeIs(Mode),
    ModeIsNot(Mode),
    SearchIsActive,
    SearchIsInactive,
    PaletteIsOpen,
    PaletteIsClosed,
    /// True only when a palette is open and its active kind matches `PaletteKind`.
    /// A closed palette never matches a kind.
    PaletteKindIs(PaletteKind),
    HelpIsOpen,
    HelpIsClosed,
    PaletteInputHistoryIsAvailable,
    PaletteInputHistoryIsUnavailable,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeConditionContext<'a> {
    pub mode: Mode,
    pub active_palette: Option<PaletteKind>,
    pub palette_input_history_available: bool,
    pub extensions: &'a ExtensionUiSnapshot,
}

impl<'a> RuntimeConditionContext<'a> {
    pub fn new(
        mode: Mode,
        active_palette: Option<PaletteKind>,
        extensions: &'a ExtensionUiSnapshot,
    ) -> RuntimeConditionContext<'a> {
        RuntimeConditionContext {
            mode,
            active_palette,
            palette_input_history_available: active_palette
                .is_some_and(PaletteKind::supports_input_history),
            extensions,
        }
    }

    pub fn normal(extensions: &'a ExtensionUiSnapshot) -> RuntimeConditionContext<'a> {
        Self::new(Mode::Normal, None, extensions)
    }
}

pub fn evaluate_condition(expr: ConditionExpr, ctx: &RuntimeConditionContext<'_>) -> bool {
    match expr {
        ConditionExpr::Always => true,
        ConditionExpr::All(conditions) => conditions
            .iter()
            .copied()
            .all(|condition| runtime_condition_is_met(condition, ctx)),
        ConditionExpr::Any(conditions) => conditions
            .iter()
            .copied()
            .any(|condition| runtime_condition_is_met(condition, ctx)),
    }
}

pub fn first_unmet_condition(
    expr: ConditionExpr,
    ctx: &RuntimeConditionContext<'_>,
) -> Option<RuntimeCondition> {
    match expr {
        ConditionExpr::Always => None,
        ConditionExpr::All(conditions) => conditions
            .iter()
            .copied()
            .find(|condition| !runtime_condition_is_met(*condition, ctx)),
        ConditionExpr::Any(conditions) => {
            if conditions
                .iter()
                .copied()
                .any(|condition| runtime_condition_is_met(condition, ctx))
            {
                None
            } else {
                conditions.first().copied()
            }
        }
    }
}

pub fn runtime_condition_is_met(
    condition: RuntimeCondition,
    ctx: &RuntimeConditionContext<'_>,
) -> bool {
    match condition {
        RuntimeCondition::ModeIs(mode) => ctx.mode == mode,
        RuntimeCondition::ModeIsNot(mode) => ctx.mode != mode,
        RuntimeCondition::SearchIsActive => ctx.extensions.search_active,
        RuntimeCondition::SearchIsInactive => !ctx.extensions.search_active,
        RuntimeCondition::PaletteIsOpen => ctx.active_palette.is_some(),
        RuntimeCondition::PaletteIsClosed => ctx.active_palette.is_none(),
        RuntimeCondition::PaletteKindIs(kind) => ctx.active_palette == Some(kind),
        RuntimeCondition::HelpIsOpen => ctx.mode == Mode::Help,
        RuntimeCondition::HelpIsClosed => ctx.mode != Mode::Help,
        RuntimeCondition::PaletteInputHistoryIsAvailable => ctx.palette_input_history_available,
        RuntimeCondition::PaletteInputHistoryIsUnavailable => !ctx.palette_input_history_available,
    }
}

fn add_condition(conditions: &mut Vec<RuntimeCondition>, condition: RuntimeCondition) {
    match condition {
        RuntimeCondition::ModeIs(_)
        | RuntimeCondition::ModeIsNot(_)
        | RuntimeCondition::SearchIsActive
        | RuntimeCondition::SearchIsInactive
        | RuntimeCondition::PaletteIsClosed
        | RuntimeCondition::HelpIsClosed => add_atom(conditions, condition),
        RuntimeCondition::PaletteIsOpen => {
            add_atom(conditions, RuntimeCondition::ModeIs(Mode::Palette));
            add_atom(conditions, RuntimeCondition::PaletteIsOpen);
        }
        RuntimeCondition::PaletteKindIs(kind) => {
            add_atom(conditions, RuntimeCondition::ModeIs(Mode::Palette));
            add_atom(conditions, RuntimeCondition::PaletteIsOpen);
            add_atom(conditions, RuntimeCondition::PaletteKindIs(kind));
        }
        RuntimeCondition::HelpIsOpen => {
            add_atom(conditions, RuntimeCondition::ModeIs(Mode::Help));
            add_atom(conditions, RuntimeCondition::HelpIsOpen);
        }
        RuntimeCondition::PaletteInputHistoryIsAvailable => {
            add_atom(conditions, RuntimeCondition::ModeIs(Mode::Palette));
            add_atom(conditions, RuntimeCondition::PaletteIsOpen);
            add_atom(conditions, RuntimeCondition::PaletteInputHistoryIsAvailable);
        }
        RuntimeCondition::PaletteInputHistoryIsUnavailable => {
            add_atom(conditions, RuntimeCondition::ModeIs(Mode::Palette));
            add_atom(
                conditions,
                RuntimeCondition::PaletteInputHistoryIsUnavailable,
            );
        }
    }
}

fn add_atom(conditions: &mut Vec<RuntimeCondition>, condition: RuntimeCondition) {
    if !conditions.contains(&condition) {
        conditions.push(condition);
    }
}

fn condition_weight(condition: RuntimeCondition) -> u16 {
    match condition {
        RuntimeCondition::ModeIs(_)
        | RuntimeCondition::ModeIsNot(_)
        | RuntimeCondition::SearchIsActive
        | RuntimeCondition::SearchIsInactive
        | RuntimeCondition::PaletteIsOpen
        | RuntimeCondition::PaletteIsClosed
        | RuntimeCondition::PaletteKindIs(_)
        | RuntimeCondition::HelpIsOpen
        | RuntimeCondition::HelpIsClosed
        | RuntimeCondition::PaletteInputHistoryIsAvailable
        | RuntimeCondition::PaletteInputHistoryIsUnavailable => 1,
    }
}

fn condition_key(condition: RuntimeCondition) -> (u8, u8) {
    match condition {
        RuntimeCondition::ModeIs(mode) => (0, mode_key(mode)),
        RuntimeCondition::ModeIsNot(mode) => (1, mode_key(mode)),
        RuntimeCondition::SearchIsActive => (2, 0),
        RuntimeCondition::SearchIsInactive => (3, 0),
        RuntimeCondition::PaletteIsOpen => (4, 0),
        RuntimeCondition::PaletteIsClosed => (5, 0),
        RuntimeCondition::PaletteKindIs(kind) => (6, palette_kind_key(kind)),
        RuntimeCondition::HelpIsOpen => (7, 0),
        RuntimeCondition::HelpIsClosed => (8, 0),
        RuntimeCondition::PaletteInputHistoryIsAvailable => (9, 0),
        RuntimeCondition::PaletteInputHistoryIsUnavailable => (10, 0),
    }
}

fn mode_key(mode: Mode) -> u8 {
    match mode {
        Mode::Normal => 0,
        Mode::Palette => 1,
        Mode::Help => 2,
    }
}

fn palette_kind_key(kind: PaletteKind) -> u8 {
    match kind {
        PaletteKind::Command => 0,
        PaletteKind::Search => 1,
        PaletteKind::SearchResults => 2,
        PaletteKind::History => 3,
        PaletteKind::Outline => 4,
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Mode;
    use crate::extension::ExtensionUiSnapshot;
    use crate::palette::PaletteKind;

    use super::{
        BindingCondition, ConditionExpr, RuntimeCondition, RuntimeConditionContext,
        runtime_condition_is_met,
    };

    #[test]
    fn palette_kind_condition_requires_an_open_matching_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let closed = RuntimeConditionContext::normal(&extensions);
        assert!(!runtime_condition_is_met(
            RuntimeCondition::PaletteKindIs(PaletteKind::Command),
            &closed
        ));

        let open =
            RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Command), &extensions);
        assert!(runtime_condition_is_met(
            RuntimeCondition::PaletteKindIs(PaletteKind::Command),
            &open
        ));
    }

    #[test]
    fn palette_input_history_requires_a_history_capable_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let command =
            RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Command), &extensions);
        assert!(command.palette_input_history_available);

        let outline =
            RuntimeConditionContext::new(Mode::Palette, Some(PaletteKind::Outline), &extensions);
        assert!(!outline.palette_input_history_available);
    }

    #[test]
    fn binding_conditions_are_normalized_idempotently() {
        static PALETTE_COMMAND: &[RuntimeCondition] =
            &[RuntimeCondition::PaletteKindIs(PaletteKind::Command)];
        static PALETTE_PLUS_COMMAND: &[RuntimeCondition] = &[
            RuntimeCondition::ModeIs(Mode::Palette),
            RuntimeCondition::PaletteKindIs(PaletteKind::Command),
        ];
        static PALETTE_DUPLICATED: &[RuntimeCondition] = &[
            RuntimeCondition::ModeIs(Mode::Palette),
            RuntimeCondition::ModeIs(Mode::Palette),
        ];

        let command = BindingCondition::new(ConditionExpr::All(PALETTE_COMMAND));
        let palette_plus_command = BindingCondition::new(ConditionExpr::All(PALETTE_PLUS_COMMAND));
        assert_eq!(command, palette_plus_command);
        assert_eq!(
            command.priority_score(),
            palette_plus_command.priority_score()
        );

        let palette = BindingCondition::new(ConditionExpr::All(&[RuntimeCondition::ModeIs(
            Mode::Palette,
        )]));
        let duplicated = BindingCondition::new(ConditionExpr::All(PALETTE_DUPLICATED));
        assert_eq!(palette, duplicated);
        assert_eq!(palette.priority_score(), duplicated.priority_score());
    }

    #[test]
    fn binding_condition_priority_counts_distinct_normalized_constraints() {
        static PALETTE: &[RuntimeCondition] = &[RuntimeCondition::ModeIs(Mode::Palette)];
        static PALETTE_COMMAND: &[RuntimeCondition] =
            &[RuntimeCondition::PaletteKindIs(PaletteKind::Command)];
        static PALETTE_COMMAND_WITH_HISTORY: &[RuntimeCondition] = &[
            RuntimeCondition::PaletteKindIs(PaletteKind::Command),
            RuntimeCondition::PaletteInputHistoryIsAvailable,
        ];

        let palette = BindingCondition::new(ConditionExpr::All(PALETTE));
        let command = BindingCondition::new(ConditionExpr::All(PALETTE_COMMAND));
        let command_with_history =
            BindingCondition::new(ConditionExpr::All(PALETTE_COMMAND_WITH_HISTORY));

        assert!(palette.priority_score() < command.priority_score());
        assert!(command.priority_score() < command_with_history.priority_score());
    }
}
