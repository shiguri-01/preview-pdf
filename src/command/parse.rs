use crate::error::{AppError, AppResult};
use crate::palette::PaletteKind;

use super::spec::all_command_specs;
use super::types::{Command, SearchMatcherKind};

pub fn parse_command_text(input: &str) -> AppResult<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("command must not be empty"));
    }

    let (id, args_text) = match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], trimmed[index..].trim_start()),
        None => (trimmed, ""),
    };

    if !all_command_specs().iter().any(|spec| spec.id == id) {
        return Err(AppError::invalid_argument("unknown command id"));
    }

    match id {
        "next-page" => parse_no_args(id, args_text, Command::NextPage),
        "prev-page" => parse_no_args(id, args_text, Command::PrevPage),
        "first-page" => parse_no_args(id, args_text, Command::FirstPage),
        "last-page" => parse_no_args(id, args_text, Command::LastPage),
        "goto-page" => parse_goto_page(args_text),
        "set-zoom" => parse_set_zoom(args_text),
        "zoom-in" => parse_no_args(id, args_text, Command::ZoomIn),
        "zoom-out" => parse_no_args(id, args_text, Command::ZoomOut),
        "scroll" => parse_scroll(args_text),
        "debug-status-show" => parse_no_args(id, args_text, Command::DebugStatusShow),
        "debug-status-hide" => parse_no_args(id, args_text, Command::DebugStatusHide),
        "debug-status-toggle" => parse_no_args(id, args_text, Command::DebugStatusToggle),
        "open-palette" => parse_open_palette(args_text),
        "close-palette" => parse_no_args(id, args_text, Command::ClosePalette),
        "search" => parse_no_args(id, args_text, Command::OpenSearch),
        "submit-search" => parse_submit_search(args_text),
        "next-search-hit" => parse_no_args(id, args_text, Command::NextSearchHit),
        "prev-search-hit" => parse_no_args(id, args_text, Command::PrevSearchHit),
        "history-back" => parse_no_args(id, args_text, Command::HistoryBack),
        "history-forward" => parse_no_args(id, args_text, Command::HistoryForward),
        "history-goto" => parse_history_goto(args_text),
        "history" => parse_no_args(id, args_text, Command::OpenHistory),
        "cancel" => parse_no_args(id, args_text, Command::Cancel),
        "quit" => parse_no_args(id, args_text, Command::Quit),
        _ => Err(AppError::unsupported(
            "command parser is out of sync with registry",
        )),
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
        "zoom-in" => "zoom-in does not accept arguments",
        "zoom-out" => "zoom-out does not accept arguments",
        "debug-status-show" => "debug-status-show does not accept arguments",
        "debug-status-hide" => "debug-status-hide does not accept arguments",
        "debug-status-toggle" => "debug-status-toggle does not accept arguments",
        "close-palette" => "close-palette does not accept arguments",
        "search" => "search does not accept arguments",
        "next-search-hit" => "next-search-hit does not accept arguments",
        "prev-search-hit" => "prev-search-hit does not accept arguments",
        "history-back" => "history-back does not accept arguments",
        "history-forward" => "history-forward does not accept arguments",
        "history" => "history does not accept arguments",
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

fn parse_set_zoom(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(value_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "set-zoom requires 1 argument: value",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "set-zoom accepts exactly 1 argument",
        ));
    }

    let value = value_text
        .parse::<f32>()
        .map_err(|_| AppError::invalid_argument("set-zoom value must be f32"))?;

    Ok(Command::SetZoom { value })
}

fn parse_scroll(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(dx_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "scroll requires 2 arguments: dx dy",
        ));
    };
    let Some(dy_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "scroll requires 2 arguments: dx dy",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "scroll accepts exactly 2 arguments",
        ));
    }

    let dx = dx_text
        .parse::<i32>()
        .map_err(|_| AppError::invalid_argument("scroll dx must be i32"))?;
    let dy = dy_text
        .parse::<i32>()
        .map_err(|_| AppError::invalid_argument("scroll dy must be i32"))?;

    Ok(Command::Scroll { dx, dy })
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
    use super::parse_command_text;
    use crate::command::{Command, SearchMatcherKind};
    use crate::palette::PaletteKind;

    #[test]
    fn parses_basic_commands() {
        assert_eq!(
            parse_command_text("next-page").expect("parse should succeed"),
            Command::NextPage
        );
        assert_eq!(
            parse_command_text("set-zoom 1.25").expect("parse should succeed"),
            Command::SetZoom { value: 1.25 }
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
}
