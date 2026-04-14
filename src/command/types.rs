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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLayoutModeArg {
    Single,
    Spread,
}

impl PageLayoutModeArg {
    pub fn id(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Spread => "spread",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "single" => Some(Self::Single),
            "spread" => Some(Self::Spread),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadDirectionArg {
    Ltr,
    Rtl,
}

impl SpreadDirectionArg {
    pub fn id(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ltr" => Some(Self::Ltr),
            "rtl" => Some(Self::Rtl),
            _ => None,
        }
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
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanAmount {
    DefaultStep,
    Cells(i32),
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
    ZoomReset,
    Pan {
        direction: PanDirection,
        amount: PanAmount,
    },
    SetPageLayout {
        mode: PageLayoutModeArg,
        direction: Option<SpreadDirectionArg>,
    },
    DebugStatusShow,
    DebugStatusHide,
    DebugStatusToggle,
    OpenPalette {
        kind: PaletteKind,
        seed: Option<String>,
    },
    ClosePalette,
    OpenHelp,
    CloseHelp,
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
    OpenOutline,
    OutlineGoto {
        page: usize,
        title: String,
    },
    Cancel,
    Quit,
}

impl Command {
    pub fn id(&self) -> &'static str {
        match self {
            Self::NextPage => "next-page",
            Self::PrevPage => "prev-page",
            Self::FirstPage => "first-page",
            Self::LastPage => "last-page",
            Self::GotoPage { .. } => "goto-page",
            Self::SetZoom { .. } => "zoom",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::ZoomReset => "zoom-reset",
            Self::Pan { .. } => "pan",
            Self::SetPageLayout { mode, .. } => match mode {
                PageLayoutModeArg::Single => "page-layout-single",
                PageLayoutModeArg::Spread => "page-layout-spread",
            },
            Self::DebugStatusShow => "debug-status-show",
            Self::DebugStatusHide => "debug-status-hide",
            Self::DebugStatusToggle => "debug-status-toggle",
            Self::OpenPalette { .. } => "open-palette",
            Self::ClosePalette => "close-palette",
            Self::OpenHelp => "help",
            Self::CloseHelp => "close-help",
            Self::OpenSearch => "search",
            Self::SubmitSearch { .. } => "submit-search",
            Self::NextSearchHit => "next-search-hit",
            Self::PrevSearchHit => "prev-search-hit",
            Self::HistoryBack => "history-back",
            Self::HistoryForward => "history-forward",
            Self::HistoryGoto { .. } => "history-goto",
            Self::OpenHistory => "history",
            Self::OpenOutline => "outline",
            Self::OutlineGoto { .. } => "outline-goto",
            Self::Cancel => "cancel-search",
            Self::Quit => "quit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExposure {
    Public,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandInvocationPolicy {
    User,
    KeymapOnly,
    InternalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCondition {
    SearchActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAvailability {
    Always,
    AllOf(&'static [CommandCondition]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandInvocationSource {
    Keymap,
    CommandPaletteInput,
    PaletteProvider,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandRequest {
    pub command: Command,
    pub source: CommandInvocationSource,
}

impl CommandRequest {
    pub fn new(command: Command, source: CommandInvocationSource) -> Self {
        Self { command, source }
    }
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
    ZoomReset,
    Pan,
    SetPageLayout,
    DebugStatusShow,
    DebugStatusHide,
    DebugStatusToggle,
    OpenPalette,
    ClosePalette,
    Help,
    CloseHelp,
    Search,
    SubmitSearch,
    NextSearchHit,
    PrevSearchHit,
    HistoryBack,
    HistoryForward,
    HistoryGoto,
    History,
    Outline,
    OutlineGoto,
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
            Self::SetZoom => "zoom",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::ZoomReset => "zoom-reset",
            Self::Pan => "pan",
            Self::SetPageLayout => "page-layout",
            Self::DebugStatusShow => "debug-status-show",
            Self::DebugStatusHide => "debug-status-hide",
            Self::DebugStatusToggle => "debug-status-toggle",
            Self::OpenPalette => "open-palette",
            Self::ClosePalette => "close-palette",
            Self::Help => "help",
            Self::CloseHelp => "close-help",
            Self::Search => "search",
            Self::SubmitSearch => "submit-search",
            Self::NextSearchHit => "next-search-hit",
            Self::PrevSearchHit => "prev-search-hit",
            Self::HistoryBack => "history-back",
            Self::HistoryForward => "history-forward",
            Self::HistoryGoto => "history-goto",
            Self::History => "history",
            Self::Outline => "outline",
            Self::OutlineGoto => "outline-goto",
            Self::Cancel => "cancel-search",
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
            Self::ZoomReset => ActionId::ZoomReset,
            Self::Pan { .. } => ActionId::Pan,
            Self::SetPageLayout { .. } => ActionId::SetPageLayout,
            Self::DebugStatusShow => ActionId::DebugStatusShow,
            Self::DebugStatusHide => ActionId::DebugStatusHide,
            Self::DebugStatusToggle => ActionId::DebugStatusToggle,
            Self::OpenPalette { .. } => ActionId::OpenPalette,
            Self::ClosePalette => ActionId::ClosePalette,
            Self::OpenHelp => ActionId::Help,
            Self::CloseHelp => ActionId::CloseHelp,
            Self::OpenSearch => ActionId::Search,
            Self::SubmitSearch { .. } => ActionId::SubmitSearch,
            Self::NextSearchHit => ActionId::NextSearchHit,
            Self::PrevSearchHit => ActionId::PrevSearchHit,
            Self::HistoryBack => ActionId::HistoryBack,
            Self::HistoryForward => ActionId::HistoryForward,
            Self::HistoryGoto { .. } => ActionId::HistoryGoto,
            Self::OpenHistory => ActionId::History,
            Self::OpenOutline => ActionId::Outline,
            Self::OutlineGoto { .. } => ActionId::OutlineGoto,
            Self::Cancel => ActionId::Cancel,
            Self::Quit => ActionId::Quit,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::PaletteKind;

    use super::{
        ActionId, Command, PageLayoutModeArg, PanAmount, PanDirection, SearchMatcherKind,
        SpreadDirectionArg,
    };

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
        assert_eq!(Command::ZoomReset.action_id(), ActionId::ZoomReset);
        assert_eq!(Command::HistoryBack.action_id(), ActionId::HistoryBack);
        assert_eq!(Command::OpenHistory.action_id(), ActionId::History);
        assert_eq!(Command::OpenOutline.action_id(), ActionId::Outline);
        assert_eq!(Command::OpenHelp.action_id(), ActionId::Help);
        assert_eq!(Command::CloseHelp.action_id(), ActionId::CloseHelp);
        assert_eq!(
            Command::Pan {
                direction: PanDirection::Right,
                amount: PanAmount::DefaultStep,
            }
            .action_id(),
            ActionId::Pan
        );
        assert_eq!(
            Command::OutlineGoto {
                page: 5,
                title: "Section".to_string(),
            }
            .action_id(),
            ActionId::OutlineGoto
        );
        assert_eq!(
            Command::SetPageLayout {
                mode: PageLayoutModeArg::Spread,
                direction: Some(SpreadDirectionArg::Rtl),
            }
            .action_id(),
            ActionId::SetPageLayout
        );
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
pub enum ArgHint {
    None,
    Enum(&'static [&'static str]),
}

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
    pub exposure: CommandExposure,
    pub invocation: CommandInvocationPolicy,
    pub availability: CommandAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Applied,
    Noop,
    QuitRequested,
}
