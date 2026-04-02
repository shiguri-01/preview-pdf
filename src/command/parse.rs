use std::num::IntErrorKind;

use crate::app::AppState;
use crate::error::{AppError, AppResult};
use crate::extension::ExtensionUiSnapshot;
use crate::palette::PaletteKind;

use super::spec::{CommandConditionContext, find_command_spec, validate_command_id_for_source};
use super::types::{
    Command, CommandInvocationSource, PageLayoutModeArg, PanAmount, PanDirection,
    SearchMatcherKind, SpreadDirectionArg,
};

pub fn parse_command_text(input: &str) -> AppResult<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("command must not be empty"));
    }

    let (id, args_text) = match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], trimmed[index..].trim_start()),
        None => (trimmed, ""),
    };

    if find_command_spec(id).is_none() {
        return Err(AppError::invalid_argument("unknown command id"));
    }

    match id {
        "next-page" => parse_no_args(id, args_text, Command::NextPage),
        "prev-page" => parse_no_args(id, args_text, Command::PrevPage),
        "first-page" => parse_no_args(id, args_text, Command::FirstPage),
        "last-page" => parse_no_args(id, args_text, Command::LastPage),
        "goto-page" => parse_goto_page(args_text),
        "zoom" => parse_zoom(args_text),
        "zoom-in" => parse_no_args(id, args_text, Command::ZoomIn),
        "zoom-out" => parse_no_args(id, args_text, Command::ZoomOut),
        "zoom-reset" => parse_no_args(id, args_text, Command::ZoomReset),
        "pan" => parse_pan(args_text),
        "page-layout-single" => parse_page_layout_single(args_text),
        "page-layout-spread" => parse_page_layout_spread(args_text),
        "debug-status-show" => parse_no_args(id, args_text, Command::DebugStatusShow),
        "debug-status-hide" => parse_no_args(id, args_text, Command::DebugStatusHide),
        "debug-status-toggle" => parse_no_args(id, args_text, Command::DebugStatusToggle),
        "open-palette" => parse_open_palette(args_text),
        "close-palette" => parse_no_args(id, args_text, Command::ClosePalette),
        "help" => parse_no_args(id, args_text, Command::OpenHelp),
        "close-help" => parse_no_args(id, args_text, Command::CloseHelp),
        "search" => parse_no_args(id, args_text, Command::OpenSearch),
        "submit-search" => parse_submit_search(args_text),
        "next-search-hit" => parse_no_args(id, args_text, Command::NextSearchHit),
        "prev-search-hit" => parse_no_args(id, args_text, Command::PrevSearchHit),
        "history-back" => parse_no_args(id, args_text, Command::HistoryBack),
        "history-forward" => parse_no_args(id, args_text, Command::HistoryForward),
        "history-goto" => parse_history_goto(args_text),
        "history" => parse_no_args(id, args_text, Command::OpenHistory),
        "outline" => parse_no_args(id, args_text, Command::OpenOutline),
        "outline-goto" => parse_outline_goto(args_text),
        "cancel" => parse_no_args(id, args_text, Command::Cancel),
        "quit" => parse_no_args(id, args_text, Command::Quit),
        _ => Err(AppError::unsupported(
            "command parser is out of sync with registry",
        )),
    }
}

pub fn parse_invocable_command_text(
    input: &str,
    source: CommandInvocationSource,
    app: &AppState,
    extensions: &ExtensionUiSnapshot,
) -> AppResult<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("command must not be empty"));
    }

    let id = first_token(trimmed);
    let ctx = CommandConditionContext {
        app,
        extensions,
        source,
    };
    validate_command_id_for_source(id, &ctx)?;
    parse_command_text(trimmed)
}

pub(crate) fn first_token(input: &str) -> &str {
    let trimmed = input.trim_start();
    match trimmed.find(char::is_whitespace) {
        Some(index) => &trimmed[..index],
        None => trimmed,
    }
}

fn parse_no_args(id: &str, args_text: &str, cmd: Command) -> AppResult<Command> {
    if args_text.is_empty() {
        return Ok(cmd);
    }

    Err(AppError::invalid_argument(match id {
        "next-page" => "next-page does not accept arguments",
        "prev-page" => "prev-page does not accept arguments",
        "first-page" => "first-page does not accept arguments",
        "last-page" => "last-page does not accept arguments",
        "page-layout-single" => "page-layout-single does not accept arguments",
        "zoom-in" => "zoom-in does not accept arguments",
        "zoom-out" => "zoom-out does not accept arguments",
        "zoom-reset" => "zoom-reset does not accept arguments",
        "debug-status-show" => "debug-status-show does not accept arguments",
        "debug-status-hide" => "debug-status-hide does not accept arguments",
        "debug-status-toggle" => "debug-status-toggle does not accept arguments",
        "close-palette" => "close-palette does not accept arguments",
        "help" => "help does not accept arguments",
        "close-help" => "close-help does not accept arguments",
        "search" => "search does not accept arguments",
        "next-search-hit" => "next-search-hit does not accept arguments",
        "prev-search-hit" => "prev-search-hit does not accept arguments",
        "history-back" => "history-back does not accept arguments",
        "history-forward" => "history-forward does not accept arguments",
        "history" => "history does not accept arguments",
        "outline" => "outline does not accept arguments",
        "cancel" => "cancel does not accept arguments",
        "quit" => "quit does not accept arguments",
        _ => "command does not accept arguments",
    }))
}

fn parse_open_palette(args_text: &str) -> AppResult<Command> {
    let trimmed = args_text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument(
            "open-palette requires 1 argument: kind",
        ));
    }

    let (kind_text, seed) = match trimmed.find(char::is_whitespace) {
        Some(index) => {
            let kind = trimmed[..index].trim();
            let seed = trimmed[index..].trim_start();
            let seed = if seed.is_empty() {
                None
            } else {
                Some(seed.to_string())
            };
            (kind, seed)
        }
        None => (trimmed, None),
    };

    let kind =
        PaletteKind::parse(kind_text).ok_or(AppError::invalid_argument("unknown palette kind"))?;

    Ok(Command::OpenPalette { kind, seed })
}

fn parse_goto_page(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(page_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "goto-page requires 1 argument: page",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "goto-page accepts exactly 1 argument",
        ));
    }

    let page = page_text
        .parse::<i32>()
        .map_err(|_| AppError::invalid_argument("goto-page page must be an integer"))?;
    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }

    Ok(Command::GotoPage {
        page: page as usize,
    })
}

fn parse_zoom(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(value_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "zoom requires 1 argument: value",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "zoom accepts exactly 1 argument",
        ));
    }

    let value = value_text
        .parse::<f32>()
        .map_err(|_| AppError::invalid_argument("zoom value must be f32"))?;

    Ok(Command::SetZoom { value })
}

fn parse_pan(args_text: &str) -> AppResult<Command> {
    let parts = args_text.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(AppError::invalid_argument(
            "pan requires direction [amount]",
        ));
    }

    if parts.len() > 2 {
        return Err(AppError::invalid_argument("pan accepts direction [amount]"));
    }

    let Some((direction, amount)) = parse_pan_direction(&parts)? else {
        return Err(AppError::invalid_argument(
            "pan direction must be one of: left, right, up, down",
        ));
    };

    Ok(Command::Pan { direction, amount })
}

fn parse_pan_direction(parts: &[&str]) -> AppResult<Option<(PanDirection, PanAmount)>> {
    let Some(direction) = parse_pan_direction_token(parts[0]) else {
        return Ok(None);
    };

    let amount = match parts.get(1) {
        Some(value_text) => PanAmount::Cells(parse_pan_amount(value_text)?),
        None => PanAmount::DefaultStep,
    };
    Ok(Some((direction, amount)))
}

fn parse_pan_amount(value_text: &str) -> AppResult<i32> {
    match value_text.parse::<i32>() {
        Ok(amount) => Ok(amount),
        Err(err) => match err.kind() {
            // Treat numeric overflow as a silent clamp so huge pan inputs behave like
            // ordinary edge-limited movement instead of surfacing an implementation limit.
            IntErrorKind::PosOverflow => Ok(i32::MAX),
            IntErrorKind::NegOverflow => Ok(i32::MIN),
            _ => Err(AppError::invalid_argument("pan amount must be an integer")),
        },
    }
}

fn parse_pan_direction_token(value: &str) -> Option<PanDirection> {
    PanDirection::parse(value)
}

fn parse_page_layout_single(args_text: &str) -> AppResult<Command> {
    parse_no_args(
        "page-layout-single",
        args_text,
        Command::SetPageLayout {
            mode: PageLayoutModeArg::Single,
            direction: None,
        },
    )
}

fn parse_page_layout_spread(args_text: &str) -> AppResult<Command> {
    let trimmed = args_text.trim();
    if trimmed.is_empty() {
        return Ok(Command::SetPageLayout {
            mode: PageLayoutModeArg::Spread,
            direction: None,
        });
    }

    let mut parts = trimmed.split_whitespace();
    let Some(direction_text) = parts.next() else {
        unreachable!("trimmed non-empty input should yield one token");
    };
    let direction = SpreadDirectionArg::parse(direction_text)
        .ok_or(AppError::invalid_argument("unknown spread direction"))?;
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "page-layout-spread accepts at most 1 argument",
        ));
    }

    Ok(Command::SetPageLayout {
        mode: PageLayoutModeArg::Spread,
        direction: Some(direction),
    })
}

fn parse_submit_search(args_text: &str) -> AppResult<Command> {
    let trimmed = args_text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument(
            "submit-search requires at least 1 argument: query",
        ));
    }

    let mut query = trimmed.to_string();
    let mut matcher = SearchMatcherKind::ContainsInsensitive;

    if let Some((head, tail)) = split_last_token(trimmed)
        && let Some(parsed) = SearchMatcherKind::parse(tail)
    {
        if head.trim().is_empty() {
            return Err(AppError::invalid_argument(
                "submit-search requires at least 1 argument: query",
            ));
        }
        query = head.trim().to_string();
        matcher = parsed;
    }

    Ok(Command::SubmitSearch { query, matcher })
}

fn parse_history_goto(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(page_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "history-goto requires 1 argument: page",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "history-goto accepts exactly 1 argument",
        ));
    }

    let page = page_text
        .parse::<i32>()
        .map_err(|_| AppError::invalid_argument("history-goto page must be an integer"))?;
    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }

    Ok(Command::HistoryGoto {
        page: page as usize,
    })
}

fn parse_outline_goto(args_text: &str) -> AppResult<Command> {
    let trimmed = args_text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument(
            "outline-goto requires 2 arguments: page title",
        ));
    }

    let Some((page_text, title)) = trimmed.split_once(char::is_whitespace) else {
        return Err(AppError::invalid_argument(
            "outline-goto requires 2 arguments: page title",
        ));
    };

    let page = page_text
        .parse::<i32>()
        .map_err(|_| AppError::invalid_argument("outline-goto page must be an integer"))?;
    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }

    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::invalid_argument(
            "outline-goto requires 2 arguments: page title",
        ));
    }

    Ok(Command::OutlineGoto {
        page: (page - 1) as usize,
        title: title.to_string(),
    })
}

fn split_last_token(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.rfind(char::is_whitespace) {
        Some(index) => Some((&trimmed[..index], trimmed[index + 1..].trim_start())),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{first_token, parse_command_text};
    use crate::command::{
        Command, PageLayoutModeArg, PanAmount, PanDirection, SearchMatcherKind, SpreadDirectionArg,
    };
    use crate::palette::PaletteKind;

    #[test]
    fn parses_basic_commands() {
        assert_eq!(
            parse_command_text("next-page").expect("parse should succeed"),
            Command::NextPage
        );
        assert_eq!(
            parse_command_text("zoom 1.25").expect("parse should succeed"),
            Command::SetZoom { value: 1.25 }
        );
        assert_eq!(
            parse_command_text("zoom-reset").expect("parse should succeed"),
            Command::ZoomReset
        );
        assert_eq!(
            parse_command_text("open-palette command").expect("parse should succeed"),
            Command::OpenPalette {
                kind: PaletteKind::Command,
                seed: None,
            }
        );
    }

    #[test]
    fn parse_search_without_args_opens_search_palette() {
        assert_eq!(
            parse_command_text("search").expect("parse should succeed"),
            Command::OpenSearch
        );
    }

    #[test]
    fn parse_outline_without_args_opens_outline_palette() {
        assert_eq!(
            parse_command_text("outline").expect("parse should succeed"),
            Command::OpenOutline
        );
    }

    #[test]
    fn parse_submit_search_accepts_optional_matcher() {
        assert_eq!(
            parse_command_text("submit-search hello world").expect("parse should succeed"),
            Command::SubmitSearch {
                query: "hello world".to_string(),
                matcher: SearchMatcherKind::ContainsInsensitive,
            }
        );
        assert_eq!(
            parse_command_text("submit-search hello contains-sensitive")
                .expect("parse should succeed"),
            Command::SubmitSearch {
                query: "hello".to_string(),
                matcher: SearchMatcherKind::ContainsSensitive,
            }
        );
    }

    #[test]
    fn parse_outline_goto_accepts_page_and_title() {
        assert_eq!(
            parse_command_text("outline-goto 3 Chapter 1").expect("parse should succeed"),
            Command::OutlineGoto {
                page: 2,
                title: "Chapter 1".to_string(),
            }
        );
    }

    #[test]
    fn parse_page_layout_aliases_support_mode_and_direction() {
        assert_eq!(
            parse_command_text("page-layout-single").expect("parse should succeed"),
            Command::SetPageLayout {
                mode: PageLayoutModeArg::Single,
                direction: None,
            }
        );
        assert_eq!(
            parse_command_text("page-layout-spread").expect("parse should succeed"),
            Command::SetPageLayout {
                mode: PageLayoutModeArg::Spread,
                direction: None,
            }
        );
        assert_eq!(
            parse_command_text("page-layout-spread rtl").expect("parse should succeed"),
            Command::SetPageLayout {
                mode: PageLayoutModeArg::Spread,
                direction: Some(SpreadDirectionArg::Rtl),
            }
        );
    }

    #[test]
    fn parse_pan_supports_directional_form_only() {
        assert_eq!(
            parse_command_text("pan down").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::DefaultStep,
            }
        );
        assert_eq!(
            parse_command_text("pan left 3").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::Cells(3),
            }
        );
        assert_eq!(
            parse_command_text("pan down 0").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::Cells(0),
            }
        );
        assert_eq!(
            parse_command_text("pan left -3").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::Cells(-3),
            }
        );
        assert!(
            parse_command_text("pan -2 4").is_err(),
            "raw dx dy form should be rejected"
        );
    }

    #[test]
    fn parse_pan_clamps_i32_min_for_left_and_up() {
        assert_eq!(
            parse_command_text("pan left -2147483648").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::Cells(i32::MIN),
            }
        );
        assert_eq!(
            parse_command_text("pan up -2147483648").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Up,
                amount: PanAmount::Cells(i32::MIN),
            }
        );
    }

    #[test]
    fn parse_pan_allows_i32_min_for_right_and_down() {
        assert_eq!(
            parse_command_text("pan right -2147483648").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Right,
                amount: PanAmount::Cells(i32::MIN),
            }
        );
        assert_eq!(
            parse_command_text("pan down -2147483648").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::Cells(i32::MIN),
            }
        );
    }

    #[test]
    fn parse_pan_clamps_out_of_i32_range_values() {
        assert_eq!(
            parse_command_text("pan right 999999999999").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Right,
                amount: PanAmount::Cells(i32::MAX),
            }
        );
        assert_eq!(
            parse_command_text("pan down -999999999999").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Down,
                amount: PanAmount::Cells(i32::MIN),
            }
        );
        assert_eq!(
            parse_command_text("pan left 999999999999").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Left,
                amount: PanAmount::Cells(i32::MAX),
            }
        );
        assert_eq!(
            parse_command_text("pan up -999999999999").expect("parse should succeed"),
            Command::Pan {
                direction: PanDirection::Up,
                amount: PanAmount::Cells(i32::MIN),
            }
        );
    }

    #[test]
    fn parse_pan_rejects_non_integer_amounts() {
        let err = parse_command_text("pan right nope").expect_err("parse should fail");
        assert_eq!(
            err.to_string(),
            "invalid argument: pan amount must be an integer"
        );
    }

    #[test]
    fn first_token_ignores_leading_whitespace() {
        assert_eq!(first_token("  next-page"), "next-page");
        assert_eq!(first_token("\tzoom 1.5"), "zoom");
    }
}
