mod control;
mod debug;
mod help;
mod history;
mod layout;
mod navigation;
mod outline;
mod palette;
mod search;
mod text;
mod viewport;

pub(super) use control::{cancel_search, quit, reload_document};
pub(super) use debug::{debug_status_hide, debug_status_show, debug_status_toggle};
pub(super) use help::{close_help, help_scroll_down, help_scroll_up, open_help};
pub(super) use history::{history_back, history_forward, history_goto, open_history};
pub(super) use layout::{page_layout_single, page_layout_spread};
pub(super) use navigation::{first_page, goto_page, last_page, next_page, prev_page};
pub(super) use outline::{open_outline, outline_goto};
pub(super) use palette::{
    close_palette, open_palette, palette_complete, palette_select_next, palette_select_prev,
    palette_submit,
};
pub(super) use search::{
    next_search_hit, open_search, open_search_results, prev_search_hit, search_result_goto,
    submit_search,
};
pub(super) use text::{
    palette_input_history_newer, palette_input_history_older, text_delete_backward,
    text_delete_forward, text_delete_line, text_delete_next_word, text_delete_prev_word,
    text_delete_to_end, text_insert, text_move_end, text_move_left, text_move_next_word,
    text_move_prev_word, text_move_right, text_move_start, text_yank,
};
pub(super) use viewport::{pan, set_zoom, zoom_in, zoom_out, zoom_reset};
