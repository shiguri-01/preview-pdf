use super::types::{ArgKind, ArgSpec, CommandSpec};

const NO_ARGS: [ArgSpec; 0] = [];
const ARGS_GOTO_PAGE: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
}];
const ARGS_ZOOM: [ArgSpec; 1] = [ArgSpec {
    name: "value",
    kind: ArgKind::F32,
    required: true,
}];
const ARGS_SCROLL: [ArgSpec; 2] = [
    ArgSpec {
        name: "direction",
        kind: ArgKind::String,
        required: true,
    },
    ArgSpec {
        name: "amount",
        kind: ArgKind::I32,
        required: false,
    },
];
const ARGS_PAGE_LAYOUT_SPREAD: [ArgSpec; 1] = [ArgSpec {
    name: "direction",
    kind: ArgKind::String,
    required: false,
}];
const ARGS_OPEN_PALETTE: [ArgSpec; 2] = [
    ArgSpec {
        name: "kind",
        kind: ArgKind::String,
        required: true,
    },
    ArgSpec {
        name: "seed",
        kind: ArgKind::String,
        required: false,
    },
];
const ARGS_SUBMIT_SEARCH: [ArgSpec; 2] = [
    ArgSpec {
        name: "query",
        kind: ArgKind::String,
        required: true,
    },
    ArgSpec {
        name: "matcher",
        kind: ArgKind::String,
        required: false,
    },
];
const ARGS_HISTORY_GOTO: [ArgSpec; 1] = [ArgSpec {
    name: "page",
    kind: ArgKind::I32,
    required: true,
}];

const COMMAND_SPECS: [CommandSpec; 26] = [
    CommandSpec {
        id: "next-page",
        title: "Next Page",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "prev-page",
        title: "Previous Page",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "first-page",
        title: "First Page",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "last-page",
        title: "Last Page",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "goto-page",
        title: "Go to Page",
        args: &ARGS_GOTO_PAGE,
    },
    CommandSpec {
        id: "zoom",
        title: "Zoom",
        args: &ARGS_ZOOM,
    },
    CommandSpec {
        id: "zoom-in",
        title: "Zoom In",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "zoom-out",
        title: "Zoom Out",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "scroll",
        title: "Scroll",
        args: &ARGS_SCROLL,
    },
    CommandSpec {
        id: "page-layout-single",
        title: "Single Page Layout",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "page-layout-spread",
        title: "Spread Page Layout",
        args: &ARGS_PAGE_LAYOUT_SPREAD,
    },
    CommandSpec {
        id: "debug-status-show",
        title: "Show Debug Status",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "debug-status-hide",
        title: "Hide Debug Status",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "debug-status-toggle",
        title: "Toggle Debug Status",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "open-palette",
        title: "Open Palette",
        args: &ARGS_OPEN_PALETTE,
    },
    CommandSpec {
        id: "close-palette",
        title: "Close Palette",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "search",
        title: "Search",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "submit-search",
        title: "Submit Search",
        args: &ARGS_SUBMIT_SEARCH,
    },
    CommandSpec {
        id: "next-search-hit",
        title: "Next Search Hit",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "prev-search-hit",
        title: "Previous Search Hit",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "history-back",
        title: "History Back",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "history-forward",
        title: "History Forward",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "history-goto",
        title: "History Go to Page",
        args: &ARGS_HISTORY_GOTO,
    },
    CommandSpec {
        id: "history",
        title: "Open History",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "cancel",
        title: "Cancel",
        args: &NO_ARGS,
    },
    CommandSpec {
        id: "quit",
        title: "Quit",
        args: &NO_ARGS,
    },
];

pub fn command_registry() -> &'static [CommandSpec] {
    &COMMAND_SPECS
}

pub fn all_command_specs() -> Vec<CommandSpec> {
    COMMAND_SPECS.to_vec()
}
