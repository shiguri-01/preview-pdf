use crate::condition::ConditionExpr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMatcherKind {
    ContainsInsensitive,
    ContainsSensitive,
}

impl SearchMatcherKind {
    const VARIANTS: [Self; 2] = [Self::ContainsInsensitive, Self::ContainsSensitive];
    const VALUES: &[&str] = &["contains-insensitive", "contains-sensitive"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContainsInsensitive => "contains-insensitive",
            Self::ContainsSensitive => "contains-sensitive",
        }
    }

    pub fn id(self) -> &'static str {
        self.as_str()
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    pub fn values() -> &'static [&'static str] {
        Self::VALUES
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLayoutModeArg {
    Single,
    Spread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadDirectionArg {
    Ltr,
    Rtl,
}

impl SpreadDirectionArg {
    const VARIANTS: [Self; 2] = [Self::Ltr, Self::Rtl];
    const VALUES: &[&str] = &["ltr", "rtl"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    pub fn values() -> &'static [&'static str] {
        Self::VALUES
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadCoverPolicyArg {
    Paired,
    Cover,
}

impl SpreadCoverPolicyArg {
    const VARIANTS: [Self; 2] = [Self::Paired, Self::Cover];
    const VALUES: &[&str] = &["paired", "cover"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paired => "paired",
            Self::Cover => "cover",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    pub fn values() -> &'static [&'static str] {
        Self::VALUES
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PanDirection {
    const VARIANTS: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];
    const VALUES: &[&str] = &["left", "right", "up", "down"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == value)
    }

    pub fn values() -> &'static [&'static str] {
        Self::VALUES
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanAmount {
    DefaultStep,
    Cells(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExposure {
    Public,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRole {
    UserIntent,
    SurfaceControl,
    InternalEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandInvocationPolicy {
    User,
    BindingOnly,
    InternalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTargetRequirement {
    App,
    ActivePalette,
    ActiveHelp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandInvocationSource {
    Binding,
    CommandPaletteInput,
    Internal,
}

#[cfg(test)]
mod tests {
    use super::{PanDirection, SearchMatcherKind, SpreadCoverPolicyArg, SpreadDirectionArg};

    #[test]
    fn enum_command_arguments_round_trip_through_strings() {
        for direction in [
            PanDirection::Left,
            PanDirection::Right,
            PanDirection::Up,
            PanDirection::Down,
        ] {
            assert_eq!(PanDirection::parse(direction.as_str()), Some(direction));
        }

        for direction in [SpreadDirectionArg::Ltr, SpreadDirectionArg::Rtl] {
            assert_eq!(
                SpreadDirectionArg::parse(direction.as_str()),
                Some(direction)
            );
        }

        for matcher in [
            SearchMatcherKind::ContainsInsensitive,
            SearchMatcherKind::ContainsSensitive,
        ] {
            assert_eq!(SearchMatcherKind::parse(matcher.as_str()), Some(matcher));
        }

        for policy in [SpreadCoverPolicyArg::Paired, SpreadCoverPolicyArg::Cover] {
            assert_eq!(SpreadCoverPolicyArg::parse(policy.as_str()), Some(policy));
        }
    }

    #[test]
    fn enum_value_lists_match_variant_strings() {
        assert_eq!(
            PanDirection::values(),
            &[
                PanDirection::Left.as_str(),
                PanDirection::Right.as_str(),
                PanDirection::Up.as_str(),
                PanDirection::Down.as_str(),
            ]
        );
        assert_eq!(
            SpreadDirectionArg::values(),
            &[
                SpreadDirectionArg::Ltr.as_str(),
                SpreadDirectionArg::Rtl.as_str(),
            ]
        );
        assert_eq!(
            SpreadCoverPolicyArg::values(),
            &[
                SpreadCoverPolicyArg::Paired.as_str(),
                SpreadCoverPolicyArg::Cover.as_str(),
            ]
        );
        assert_eq!(
            SearchMatcherKind::values(),
            &[
                SearchMatcherKind::ContainsInsensitive.as_str(),
                SearchMatcherKind::ContainsSensitive.as_str(),
            ]
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    F32,
    I32,
    String,
}

#[derive(Debug, Clone, Copy)]
pub enum ArgHint {
    None,
    Enum(fn() -> &'static [&'static str]),
}

impl PartialEq for ArgHint {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::None, Self::None) => true,
            (Self::Enum(lhs), Self::Enum(rhs)) => std::ptr::fn_addr_eq(lhs, rhs),
            _ => false,
        }
    }
}

impl Eq for ArgHint {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    pub required: bool,
    pub hint: ArgHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub args: &'static [ArgSpec],
    pub role: CommandRole,
    pub exposure: CommandExposure,
    pub invocation: CommandInvocationPolicy,
    pub target: CommandTargetRequirement,
    pub enabled_when: ConditionExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Applied,
    Noop,
}
