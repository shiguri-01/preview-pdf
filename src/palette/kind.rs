#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteKind {
    Command,
    Search,
    History,
}

impl PaletteKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Search => "search",
            Self::History => "history",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "command" => Some(Self::Command),
            "search" => Some(Self::Search),
            "history" => Some(Self::History),
            _ => None,
        }
    }
}
