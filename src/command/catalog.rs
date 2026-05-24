use crate::error::{AppError, AppResult};
use crate::palette::{PaletteKind, PaletteOpenPayload};

use super::dispatch::{CommandExecContext, CommandExecution};
use super::types::{
    ArgHint, ArgKind, ArgSpec, CommandAvailability, CommandCondition, CommandExposure,
    CommandInvocationPolicy, CommandInvocationSource, CommandSpec, PanAmount, PanDirection,
    SearchMatcherKind, SpreadCoverPolicyArg, SpreadDirectionArg,
};

const NO_ARGS: [ArgSpec; 0] = [];
const REQUIRES_SEARCH_ACTIVE: [CommandCondition; 1] = [CommandCondition::SearchActive];
const ARGS_GOTO_PAGE: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_ZOOM: [ArgSpec; 1] = [ArgSpec {
    name: "value",
    kind: ArgKind::F32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_PAN: [ArgSpec; 2] = [
    ArgSpec {
        name: "direction",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::Enum(PanDirection::values),
    },
    ArgSpec {
        name: "amount",
        kind: ArgKind::I32,
        required: false,
        hint: ArgHint::None,
    },
];
const ARGS_PAGE_LAYOUT_SPREAD: [ArgSpec; 2] = [
    ArgSpec {
        name: "direction",
        kind: ArgKind::String,
        required: false,
        hint: ArgHint::Enum(SpreadDirectionArg::values),
    },
    ArgSpec {
        name: "cover-policy",
        kind: ArgKind::String,
        required: false,
        hint: ArgHint::Enum(SpreadCoverPolicyArg::values),
    },
];
const ARGS_OPEN_PALETTE: [ArgSpec; 2] = [
    ArgSpec {
        name: "kind",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::None,
    },
    ArgSpec {
        name: "seed",
        kind: ArgKind::String,
        required: false,
        hint: ArgHint::None,
    },
];
const ARGS_SUBMIT_SEARCH: [ArgSpec; 2] = [
    ArgSpec {
        name: "query",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::None,
    },
    ArgSpec {
        name: "matcher",
        kind: ArgKind::String,
        required: false,
        hint: ArgHint::None,
    },
];
const ARGS_HELP_SCROLL: [ArgSpec; 1] = [ArgSpec {
    name: "delta",
    kind: ArgKind::I32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_OUTLINE_GOTO: [ArgSpec; 2] = [
    ArgSpec {
        name: "page",
        kind: ArgKind::I32,
        required: true,
        hint: ArgHint::None,
    },
    ArgSpec {
        name: "title",
        kind: ArgKind::String,
        required: true,
        hint: ArgHint::None,
    },
];

macro_rules! define_commands {
    (
        $(
            $variant:ident $( ( $($field:ident : $ty:ty),+ $(,)? ) )? {
                id: $id:literal,
                title: $title:literal,
                args: $args:expr,
                exposure: $exposure:expr,
                invocation: $invocation:expr,
                availability: $availability:expr,
                parse: $parser:tt,
                exec: $exec:path $(,)?
            }
        )+
    ) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum Command {
            $(
                $variant $( { $($field: $ty),+ } )?,
            )+
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum CommandId {
            $($variant,)+
        }

        impl CommandId {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)+
                }
            }
        }

        impl Command {
            pub fn command_id(&self) -> CommandId {
                match self {
                    $(Self::$variant $( { $($field: _),+ } )? => CommandId::$variant,)+
                }
            }

            pub fn id(&self) -> &'static str {
                self.command_id().as_str()
            }
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

        static COMMAND_SPECS: &[CommandSpec] = &[
            $(
                CommandSpec {
                    id: $id,
                    title: $title,
                    args: $args,
                    exposure: $exposure,
                    invocation: $invocation,
                    availability: $availability,
                },
            )+
        ];

        pub fn command_registry() -> &'static [CommandSpec] {
            COMMAND_SPECS
        }

        pub fn find_command_spec(id: &str) -> Option<CommandSpec> {
            COMMAND_SPECS.iter().find(|spec| spec.id == id).copied()
        }

        pub(super) fn parse_registered_command(id: &str, args_text: &str) -> AppResult<Command> {
            match id {
                $($id => define_commands!(@parse $parser, $id, args_text, $variant),)+
                _ => Err(AppError::unsupported("command parser is out of sync with registry")),
            }
        }

        pub(super) fn execute_registered_command(
            ctx: &mut CommandExecContext<'_>,
            command: Command,
        ) -> AppResult<CommandExecution> {
            match command {
                $(
                    Command::$variant $( { $($field),+ } )? => {
                        define_commands!(@exec $exec, ctx $(, $($field),+)?)
                    }
                )+
            }
        }
    };

    (@parse no_args, $id:expr, $args_text:expr, $variant:ident) => {
        super::parse::parse_no_args($id, $args_text, Command::$variant)
    };

    (@parse ($parser:path), $id:expr, $args_text:expr, $variant:ident) => {
        $parser($args_text)
    };

    (@exec $exec:path, $ctx:expr $(, $arg:expr)*) => {
        $exec($ctx $(, $arg)*)
    };
}

define_commands! {
    NextPage {
        id: "next-page",
        title: "Next Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::next_page,
    }
    PrevPage {
        id: "prev-page",
        title: "Previous Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::prev_page,
    }
    FirstPage {
        id: "first-page",
        title: "First Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::first_page,
    }
    LastPage {
        id: "last-page",
        title: "Last Page",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::last_page,
    }
    GotoPage(page: usize) {
        id: "goto-page",
        title: "Go to Page",
        args: &ARGS_GOTO_PAGE,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_goto_page),
        exec: super::handlers::goto_page,
    }
    SetZoom(value: f32) {
        id: "zoom",
        title: "Zoom",
        args: &ARGS_ZOOM,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_zoom),
        exec: super::handlers::set_zoom,
    }
    ZoomIn {
        id: "zoom-in",
        title: "Zoom In",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::zoom_in,
    }
    ZoomOut {
        id: "zoom-out",
        title: "Zoom Out",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::zoom_out,
    }
    ZoomReset {
        id: "zoom-reset",
        title: "Reset Zoom",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::zoom_reset,
    }
    Pan(direction: PanDirection, amount: PanAmount) {
        id: "pan",
        title: "Pan",
        args: &ARGS_PAN,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_pan),
        exec: super::handlers::pan,
    }
    PageLayoutSingle {
        id: "page-layout-single",
        title: "Single Page Layout",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::page_layout_single,
    }
    PageLayoutSpread(
        direction: Option<SpreadDirectionArg>,
        cover_policy: Option<SpreadCoverPolicyArg>,
    ) {
        id: "page-layout-spread",
        title: "Spread Page Layout",
        args: &ARGS_PAGE_LAYOUT_SPREAD,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_page_layout_spread),
        exec: super::handlers::page_layout_spread,
    }
    DebugStatusShow {
        id: "debug-status-show",
        title: "Show Debug Status",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::debug_status_show,
    }
    DebugStatusHide {
        id: "debug-status-hide",
        title: "Hide Debug Status",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::debug_status_hide,
    }
    DebugStatusToggle {
        id: "debug-status-toggle",
        title: "Toggle Debug Status",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::debug_status_toggle,
    }
    OpenPalette(
        kind: PaletteKind,
        payload: Option<PaletteOpenPayload>,
    ) {
        id: "open-palette",
        title: "Open Palette",
        args: &ARGS_OPEN_PALETTE,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::KeymapOnly,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_open_palette),
        exec: super::handlers::open_palette,
    }
    ClosePalette {
        id: "close-palette",
        title: "Close Palette",
        args: &NO_ARGS,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::close_palette,
    }
    OpenHelp {
        id: "help",
        title: "Open Help",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::open_help,
    }
    CloseHelp {
        id: "close-help",
        title: "Close Help",
        args: &NO_ARGS,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::KeymapOnly,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::close_help,
    }
    HelpScroll(delta: isize) {
        id: "help-scroll",
        title: "Scroll Help",
        args: &ARGS_HELP_SCROLL,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::KeymapOnly,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_help_scroll),
        exec: super::handlers::help_scroll,
    }
    OpenSearch {
        id: "search",
        title: "Search",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::open_search,
    }
    OpenSearchResults {
        id: "search-results",
        title: "Open Search Results",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
        parse: no_args,
        exec: super::handlers::open_search_results,
    }
    SubmitSearch(query: String, matcher: SearchMatcherKind) {
        id: "submit-search",
        title: "Submit Search",
        args: &ARGS_SUBMIT_SEARCH,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_submit_search),
        exec: super::handlers::submit_search,
    }
    SearchResultGoto(page: usize) {
        id: "search-goto",
        title: "Search Go to Result",
        args: &ARGS_GOTO_PAGE,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
        parse: (super::parse::parse_search_goto),
        exec: super::handlers::search_result_goto,
    }
    NextSearchHit {
        id: "next-search-hit",
        title: "Next Search Hit",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
        parse: no_args,
        exec: super::handlers::next_search_hit,
    }
    PrevSearchHit {
        id: "prev-search-hit",
        title: "Previous Search Hit",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
        parse: no_args,
        exec: super::handlers::prev_search_hit,
    }
    HistoryBack {
        id: "history-back",
        title: "History Back",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::history_back,
    }
    HistoryForward {
        id: "history-forward",
        title: "History Forward",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::history_forward,
    }
    HistoryGoto(page: usize) {
        id: "history-goto",
        title: "History Go to Page",
        args: &ARGS_GOTO_PAGE,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_history_goto),
        exec: super::handlers::history_goto,
    }
    OpenHistory {
        id: "history",
        title: "Open History",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::open_history,
    }
    OpenOutline {
        id: "outline",
        title: "Open Outline",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::open_outline,
    }
    OutlineGoto(page: usize, title: String) {
        id: "outline-goto",
        title: "Outline Go to Page",
        args: &ARGS_OUTLINE_GOTO,
        exposure: CommandExposure::Internal,
        invocation: CommandInvocationPolicy::InternalOnly,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_outline_goto),
        exec: super::handlers::outline_goto,
    }
    Cancel {
        id: "cancel",
        title: "Cancel",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::cancel,
    }
    Quit {
        id: "quit",
        title: "Quit",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::quit,
    }
}
