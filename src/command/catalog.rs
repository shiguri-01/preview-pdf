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
const REQUIRES_HELP_MODE: [CommandCondition; 1] = [CommandCondition::HelpMode];
const ARGS_GOTO_PAGE: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
    hint: ArgHint::None,
}];
const ARGS_ZOOM: [ArgSpec; 1] = [ArgSpec {
    name: "ratio",
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
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub fn all() -> &'static [Self] {
                Self::ALL
            }

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
        id: "layout-single",
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
        id: "layout-spread",
        title: "Spread Layout",
        args: &ARGS_PAGE_LAYOUT_SPREAD,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: (super::parse::parse_page_layout_spread),
        exec: super::handlers::page_layout_spread,
    }
    DebugStatusShow {
        id: "debug-show",
        title: "Show Debug Info",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::debug_status_show,
    }
    DebugStatusHide {
        id: "debug-hide",
        title: "Hide Debug Info",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::debug_status_hide,
    }
    DebugStatusToggle {
        id: "debug-toggle",
        title: "Toggle Debug Info",
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
    HelpScrollDown {
        id: "help-scroll-down",
        title: "Scroll Help Down",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_HELP_MODE),
        parse: no_args,
        exec: super::handlers::help_scroll_down,
    }
    HelpScrollUp {
        id: "help-scroll-up",
        title: "Scroll Help Up",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_HELP_MODE),
        parse: no_args,
        exec: super::handlers::help_scroll_up,
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
    CancelSearch {
        id: "cancel-search",
        title: "Cancel Search",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::AllOf(&REQUIRES_SEARCH_ACTIVE),
        parse: no_args,
        exec: super::handlers::cancel_search,
    }
    ReloadDocument {
        id: "reload",
        title: "Reload PDF",
        args: &NO_ARGS,
        exposure: CommandExposure::Public,
        invocation: CommandInvocationPolicy::User,
        availability: CommandAvailability::Always,
        parse: no_args,
        exec: super::handlers::reload_document,
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::app::{AppState, PaletteRequest};
    use crate::backend::{OutlineNode, PdfBackend, RgbaFrame, SharedPdfBackend, TextPage};
    use crate::extension::ExtensionHost;

    use super::{
        CommandExecContext, CommandId, command_registry, execute_registered_command,
        find_command_spec, parse_registered_command,
    };

    struct StubPdf {
        path: PathBuf,
        page_count: usize,
    }

    impl StubPdf {
        fn new(page_count: usize) -> Self {
            Self {
                path: PathBuf::from("stub.pdf"),
                page_count,
            }
        }
    }

    impl PdfBackend for StubPdf {
        fn path(&self) -> &Path {
            &self.path
        }

        fn doc_id(&self) -> u64 {
            7
        }

        fn page_count(&self) -> usize {
            self.page_count
        }

        fn page_dimensions(&self, _page: usize) -> crate::error::AppResult<(f32, f32)> {
            Ok((612.0, 792.0))
        }

        fn render_page(&self, _page: usize, _scale: f32) -> crate::error::AppResult<RgbaFrame> {
            Ok(RgbaFrame {
                width: 1,
                height: 1,
                pixels: vec![0; 4].into(),
            })
        }

        fn extract_text(&self, _page: usize) -> crate::error::AppResult<String> {
            Ok(String::new())
        }

        fn extract_positioned_text(&self, _page: usize) -> crate::error::AppResult<TextPage> {
            Ok(TextPage {
                width_pt: 612.0,
                height_pt: 792.0,
                glyphs: Vec::new(),
                dropped_glyphs: 0,
            })
        }

        fn extract_outline(&self) -> crate::error::AppResult<Vec<OutlineNode>> {
            Ok(Vec::new())
        }
    }

    fn test_pdf() -> SharedPdfBackend {
        Arc::new(StubPdf::new(3)) as SharedPdfBackend
    }

    #[test]
    fn command_ids_resolve_to_matching_specs() {
        assert_eq!(CommandId::all().len(), command_registry().len());

        for id in CommandId::all() {
            let spec = find_command_spec(id.as_str()).expect("command id should resolve to a spec");
            assert_eq!(spec.id, id.as_str());
        }
    }

    #[test]
    fn command_registry_entries_have_matching_ids() {
        let ids = CommandId::all()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>();

        for spec in command_registry() {
            assert!(
                ids.contains(&spec.id),
                "registered spec should have a CommandId: {}",
                spec.id
            );
        }
    }

    #[test]
    fn parser_rejects_unknown_registered_command_id() {
        assert!(parse_registered_command("missing-command", "").is_err());
    }

    #[test]
    fn no_arg_commands_parse_to_matching_ids_and_execute() {
        for spec in command_registry()
            .iter()
            .filter(|spec| spec.args.is_empty())
        {
            let command =
                parse_registered_command(spec.id, "").expect("no-arg command should parse");
            assert_eq!(command.id(), spec.id);

            let mut app = AppState::default();
            let mut extension_host = ExtensionHost::default();
            let mut palette_requests = VecDeque::<PaletteRequest>::new();
            let mut ctx = CommandExecContext {
                app: &mut app,
                view_policy: crate::config::ViewPolicy::default(),
                pdf: test_pdf(),
                extension_host: &mut extension_host,
                palette_requests: &mut palette_requests,
            };

            execute_registered_command(&mut ctx, command)
                .expect("parsed no-arg command should execute");
        }
    }
}
