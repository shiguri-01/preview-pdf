#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteKind {
    Command,
    Search,
    SearchResults,
    History,
    Outline,
}

impl PaletteKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Search => "search",
            Self::SearchResults => "search-results",
            Self::History => "history",
            Self::Outline => "outline",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "command" => Some(Self::Command),
            "search" => Some(Self::Search),
            "search-results" => Some(Self::SearchResults),
            "history" => Some(Self::History),
            "outline" => Some(Self::Outline),
            _ => None,
        }
    }
}
