use crate::palette::PaletteKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMatcherKind {
    ContainsInsensitive,
    ContainsSensitive,
}

impl SearchMatcherKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::ContainsInsensitive => "contains-insensitive",
            Self::ContainsSensitive => "contains-sensitive",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "contains-insensitive" => Some(Self::ContainsInsensitive),
            "contains-sensitive" => Some(Self::ContainsSensitive),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    NextPage,
    PrevPage,
    FirstPage,
    LastPage,
    GotoPage {
        page: usize,
    },
    SetZoom {
        value: f32,
    },
    ZoomIn,
    ZoomOut,
    Scroll {
        dx: i32,
        dy: i32,
    },
    DebugStatusShow,
    DebugStatusHide,
    DebugStatusToggle,
    OpenPalette {
        kind: PaletteKind,
        seed: Option<String>,
    },
    ClosePalette,
    OpenSearch,
    SubmitSearch {
        query: String,
        matcher: SearchMatcherKind,
    },
    NextSearchHit,
    PrevSearchHit,
    HistoryBack,
    HistoryForward,
    HistoryGoto {
        page: usize,
    },
    OpenHistory,
    Cancel,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionId {
    NextPage,
    PrevPage,
    FirstPage,
    LastPage,
    GotoPage,
    SetZoom,
    ZoomIn,
    ZoomOut,
    Scroll,
    DebugStatusShow,
    DebugStatusHide,
    DebugStatusToggle,
    OpenPalette,
    ClosePalette,
    Search,
    SubmitSearch,
    NextSearchHit,
    PrevSearchHit,
    HistoryBack,
    HistoryForward,
    HistoryGoto,
    History,
    Cancel,
    Quit,
    RenderQueue,
    PrefetchEncode,
    Input,
    RenderWorker,
    UpdateSearchQuery,
    SearchProgress,
    SearchComplete,
    SearchFailed,
    RenderPage,
    RenderPending,
}

impl ActionId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NextPage => "next-page",
            Self::PrevPage => "prev-page",
            Self::FirstPage => "first-page",
            Self::LastPage => "last-page",
            Self::GotoPage => "goto-page",
            Self::SetZoom => "set-zoom",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::Scroll => "scroll",
            Self::DebugStatusShow => "debug-status-show",
            Self::DebugStatusHide => "debug-status-hide",
            Self::DebugStatusToggle => "debug-status-toggle",
            Self::OpenPalette => "open-palette",
            Self::ClosePalette => "close-palette",
            Self::Search => "search",
            Self::SubmitSearch => "submit-search",
            Self::NextSearchHit => "next-search-hit",
            Self::PrevSearchHit => "prev-search-hit",
            Self::HistoryBack => "history-back",
            Self::HistoryForward => "history-forward",
            Self::HistoryGoto => "history-goto",
            Self::History => "history",
            Self::Cancel => "cancel",
            Self::Quit => "quit",
            Self::RenderQueue => "render-queue",
            Self::PrefetchEncode => "prefetch-encode",
            Self::Input => "input",
            Self::RenderWorker => "render-worker",
            Self::UpdateSearchQuery => "update-search-query",
            Self::SearchProgress => "search-progress",
            Self::SearchComplete => "search-complete",
            Self::SearchFailed => "search-failed",
            Self::RenderPage => "render-page",
            Self::RenderPending => "render-pending",
        }
    }
}

impl Command {
    pub fn action_id(&self) -> ActionId {
        match self {
            Self::NextPage => ActionId::NextPage,
            Self::PrevPage => ActionId::PrevPage,
            Self::FirstPage => ActionId::FirstPage,
            Self::LastPage => ActionId::LastPage,
            Self::GotoPage { .. } => ActionId::GotoPage,
            Self::SetZoom { .. } => ActionId::SetZoom,
            Self::ZoomIn => ActionId::ZoomIn,
            Self::ZoomOut => ActionId::ZoomOut,
            Self::Scroll { .. } => ActionId::Scroll,
            Self::DebugStatusShow => ActionId::DebugStatusShow,
            Self::DebugStatusHide => ActionId::DebugStatusHide,
            Self::DebugStatusToggle => ActionId::DebugStatusToggle,
            Self::OpenPalette { .. } => ActionId::OpenPalette,
            Self::ClosePalette => ActionId::ClosePalette,
            Self::OpenSearch => ActionId::Search,
            Self::SubmitSearch { .. } => ActionId::SubmitSearch,
            Self::NextSearchHit => ActionId::NextSearchHit,
            Self::PrevSearchHit => ActionId::PrevSearchHit,
            Self::HistoryBack => ActionId::HistoryBack,
            Self::HistoryForward => ActionId::HistoryForward,
            Self::HistoryGoto { .. } => ActionId::HistoryGoto,
            Self::OpenHistory => ActionId::History,
            Self::Cancel => ActionId::Cancel,
            Self::Quit => ActionId::Quit,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::PaletteKind;

    use super::{ActionId, Command, SearchMatcherKind};

    #[test]
    fn command_action_id_maps_search_and_history_variants() {
        assert_eq!(Command::OpenSearch.action_id(), ActionId::Search);
        assert_eq!(
            Command::SubmitSearch {
                query: "q".to_string(),
                matcher: SearchMatcherKind::ContainsInsensitive,
            }
            .action_id(),
            ActionId::SubmitSearch
        );
        assert_eq!(Command::HistoryBack.action_id(), ActionId::HistoryBack);
        assert_eq!(Command::OpenHistory.action_id(), ActionId::History);
        assert_eq!(
            Command::OpenPalette {
                kind: PaletteKind::Command,
                seed: None,
            }
            .action_id(),
            ActionId::OpenPalette
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    F32,
    I32,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub args: &'static [ArgSpec],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Applied,
    Noop,
    QuitRequested,
}
