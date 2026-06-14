use crate::app::Mode;
use crate::extension::ExtensionUiSnapshot;
use crate::palette::PaletteKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionExpr {
    Always,
    All(&'static [RuntimeCondition]),
    Any(&'static [RuntimeCondition]),
}

/// Shared runtime predicates for command `enabled_when` and key binding
/// `enabled_when`.
///
/// Conditions describe runtime state, not command target existence. Prefer
/// command target requirements for mandatory receivers such as an active
/// palette, focused text input, or active help.
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
    TextInputIsFocused,
    TextInputIsNotFocused,
    TextHistoryIsAvailable,
    TextHistoryIsUnavailable,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeConditionContext<'a> {
    pub mode: Mode,
    pub active_palette: Option<PaletteKind>,
    pub focused_text_input: bool,
    pub text_history_available: bool,
    pub extensions: &'a ExtensionUiSnapshot,
}

impl<'a> RuntimeConditionContext<'a> {
    pub fn new(
        mode: Mode,
        active_palette: Option<PaletteKind>,
        focused_text_input: bool,
        extensions: &'a ExtensionUiSnapshot,
    ) -> RuntimeConditionContext<'a> {
        RuntimeConditionContext {
            mode,
            active_palette,
            focused_text_input,
            text_history_available: focused_text_input
                && active_palette.is_some_and(PaletteKind::supports_text_history),
            extensions,
        }
    }

    pub fn normal(extensions: &'a ExtensionUiSnapshot) -> RuntimeConditionContext<'a> {
        Self::new(Mode::Normal, None, false, extensions)
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
        RuntimeCondition::TextInputIsFocused => ctx.focused_text_input,
        RuntimeCondition::TextInputIsNotFocused => !ctx.focused_text_input,
        RuntimeCondition::TextHistoryIsAvailable => ctx.text_history_available,
        RuntimeCondition::TextHistoryIsUnavailable => !ctx.text_history_available,
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Mode;
    use crate::extension::ExtensionUiSnapshot;
    use crate::palette::PaletteKind;

    use super::{RuntimeCondition, RuntimeConditionContext, runtime_condition_is_met};

    #[test]
    fn palette_kind_condition_requires_an_open_matching_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let closed = RuntimeConditionContext::normal(&extensions);
        assert!(!runtime_condition_is_met(
            RuntimeCondition::PaletteKindIs(PaletteKind::Command),
            &closed
        ));

        let open = RuntimeConditionContext::new(
            Mode::Palette,
            Some(PaletteKind::Command),
            true,
            &extensions,
        );
        assert!(runtime_condition_is_met(
            RuntimeCondition::PaletteKindIs(PaletteKind::Command),
            &open
        ));
    }

    #[test]
    fn text_history_requires_a_focused_history_capable_palette() {
        let extensions = ExtensionUiSnapshot::default();
        let focused_command = RuntimeConditionContext::new(
            Mode::Palette,
            Some(PaletteKind::Command),
            true,
            &extensions,
        );
        assert!(focused_command.text_history_available);

        let unfocused_command = RuntimeConditionContext::new(
            Mode::Palette,
            Some(PaletteKind::Command),
            false,
            &extensions,
        );
        assert!(!unfocused_command.text_history_available);

        let focused_outline = RuntimeConditionContext::new(
            Mode::Palette,
            Some(PaletteKind::Outline),
            true,
            &extensions,
        );
        assert!(!focused_outline.text_history_available);
    }
}
