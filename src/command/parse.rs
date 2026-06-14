use std::num::IntErrorKind;

use crate::error::{AppError, AppResult};
use crate::palette::{PaletteKind, PaletteOpenPayload};

use super::catalog::{self, Command};
use super::spec::{CommandPolicyContext, find_command_spec, validate_command_id_for_policy};
use super::types::{
    PanAmount, PanDirection, SearchMatcherKind, SpreadCoverPolicyArg, SpreadDirectionArg,
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

    catalog::parse_registered_command(id, args_text)
}

pub fn parse_invocable_command_text(
    input: &str,
    ctx: &CommandPolicyContext<'_>,
) -> AppResult<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument("command must not be empty"));
    }

    let id = first_token(trimmed);
    validate_command_id_for_policy(id, ctx)?;
    parse_command_text(trimmed)
}

pub(crate) fn first_token(input: &str) -> &str {
    let trimmed = input.trim_start();
    match trimmed.find(char::is_whitespace) {
        Some(index) => &trimmed[..index],
        None => trimmed,
    }
}

pub(super) fn parse_no_args(id: &str, args_text: &str, cmd: Command) -> AppResult<Command> {
    if args_text.is_empty() {
        return Ok(cmd);
    }

    Err(AppError::invalid_argument(match id {
        "next-page" => "next-page does not accept arguments",
        "prev-page" => "prev-page does not accept arguments",
        "first-page" => "first-page does not accept arguments",
        "last-page" => "last-page does not accept arguments",
        "layout-single" => "layout-single does not accept arguments",
        "zoom-in" => "zoom-in does not accept arguments",
        "zoom-out" => "zoom-out does not accept arguments",
        "zoom-reset" => "zoom-reset does not accept arguments",
        "debug-show" => "debug-show does not accept arguments",
        "debug-hide" => "debug-hide does not accept arguments",
        "debug-toggle" => "debug-toggle does not accept arguments",
        "close-palette" => "close-palette does not accept arguments",
        "palette.submit" => "palette.submit does not accept arguments",
        "palette.complete" => "palette.complete does not accept arguments",
        "palette.select-next" => "palette.select-next does not accept arguments",
        "palette.select-prev" => "palette.select-prev does not accept arguments",
        "text.delete-backward" => "text.delete-backward does not accept arguments",
        "text.delete-forward" => "text.delete-forward does not accept arguments",
        "text.move-left" => "text.move-left does not accept arguments",
        "text.move-right" => "text.move-right does not accept arguments",
        "palette.input-history-older" => "palette.input-history-older does not accept arguments",
        "palette.input-history-newer" => "palette.input-history-newer does not accept arguments",
        "help" => "help does not accept arguments",
        "close-help" => "close-help does not accept arguments",
        "help-scroll-down" => "help-scroll-down does not accept arguments",
        "help-scroll-up" => "help-scroll-up does not accept arguments",
        "search" => "search does not accept arguments",
        "search-results" => "search-results does not accept arguments",
        "next-search-hit" => "next-search-hit does not accept arguments",
        "prev-search-hit" => "prev-search-hit does not accept arguments",
        "history-back" => "history-back does not accept arguments",
        "history-forward" => "history-forward does not accept arguments",
        "history" => "history does not accept arguments",
        "outline" => "outline does not accept arguments",
        "cancel-search" => "cancel-search does not accept arguments",
        "quit" => "quit does not accept arguments",
        _ => "command does not accept arguments",
    }))
}

pub(super) fn parse_open_palette(args_text: &str) -> AppResult<Command> {
    let trimmed = args_text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_argument(
            "open-palette requires 1 argument: kind",
        ));
    }

    let (kind_text, input) = match trimmed.find(char::is_whitespace) {
        Some(index) => {
            let kind = trimmed[..index].trim();
            let input = trimmed[index..].trim_start();
            (kind, input)
        }
        None => (trimmed, ""),
    };

    let kind =
        PaletteKind::parse(kind_text).ok_or(AppError::invalid_argument("unknown palette kind"))?;
    let payload = parse_open_palette_payload(kind, input);

    Ok(Command::OpenPalette { kind, payload })
}

fn parse_open_palette_payload(kind: PaletteKind, input: &str) -> Option<PaletteOpenPayload> {
    if input.is_empty() {
        return None;
    }

    Some(match kind {
        PaletteKind::Command => PaletteOpenPayload::CommandInput(input.to_string()),
        PaletteKind::Search => PaletteOpenPayload::Search {
            query: input.to_string(),
            matcher: SearchMatcherKind::ContainsInsensitive,
        },
        PaletteKind::SearchResults => PaletteOpenPayload::SearchResultsQuery(input.to_string()),
        PaletteKind::History => PaletteOpenPayload::HistorySeed(input.to_string()),
        PaletteKind::Outline => PaletteOpenPayload::OutlineQuery(input.to_string()),
    })
}

pub(super) fn parse_text_insert(args_text: &str) -> AppResult<Command> {
    let text = args_text.to_string();
    if text.is_empty() {
        return Err(AppError::invalid_argument(
            "text.insert requires 1 argument: text",
        ));
    }

    Ok(Command::TextInsert { text })
}

pub(super) fn parse_goto_page(args_text: &str) -> AppResult<Command> {
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

pub(super) fn parse_zoom(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(value_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "zoom requires 1 argument: ratio",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "zoom accepts exactly 1 argument",
        ));
    }

    let value = value_text
        .parse::<f32>()
        .map_err(|_| AppError::invalid_argument("zoom ratio must be f32"))?;

    Ok(Command::SetZoom { value })
}

pub(super) fn parse_pan(args_text: &str) -> AppResult<Command> {
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

pub(super) fn parse_page_layout_spread(args_text: &str) -> AppResult<Command> {
    let trimmed = args_text.trim();
    if trimmed.is_empty() {
        return Ok(Command::PageLayoutSpread {
            direction: None,
            cover_policy: None,
        });
    }

    let mut parts = trimmed.split_whitespace();
    let Some(direction_text) = parts.next() else {
        unreachable!("trimmed non-empty input should yield one token");
    };
    let direction = SpreadDirectionArg::parse(direction_text)
        .ok_or(AppError::invalid_argument("unknown spread direction"))?;
    let cover_policy = parts
        .next()
        .map(|policy_text| {
            SpreadCoverPolicyArg::parse(policy_text)
                .ok_or(AppError::invalid_argument("unknown spread cover policy"))
        })
        .transpose()?;
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "layout-spread accepts at most 2 arguments",
        ));
    }

    Ok(Command::PageLayoutSpread {
        direction: Some(direction),
        cover_policy,
    })
}

pub(super) fn parse_submit_search(args_text: &str) -> AppResult<Command> {
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

pub(super) fn parse_search_goto(args_text: &str) -> AppResult<Command> {
    let mut parts = args_text.split_whitespace();
    let Some(page_text) = parts.next() else {
        return Err(AppError::invalid_argument(
            "search-goto requires 1 argument: page",
        ));
    };
    if parts.next().is_some() {
        return Err(AppError::invalid_argument(
            "search-goto accepts exactly 1 argument",
        ));
    }

    let page = page_text
        .parse::<i32>()
        .map_err(|_| AppError::invalid_argument("search-goto page must be an integer"))?;
    if page < 1 {
        return Err(AppError::invalid_argument("page number must be >= 1"));
    }

    Ok(Command::SearchResultGoto {
        page: page as usize,
    })
}

pub(super) fn parse_history_goto(args_text: &str) -> AppResult<Command> {
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

pub(super) fn parse_outline_goto(args_text: &str) -> AppResult<Command> {
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
        ArgHint, ArgKind, ArgSpec, Command, CommandExposure, PanAmount, PanDirection,
        SearchMatcherKind, SpreadCoverPolicyArg, SpreadDirectionArg, all_command_specs,
    };
    use crate::palette::{PaletteKind, PaletteOpenPayload};

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
                payload: None,
            }
        );
        assert_eq!(
            parse_command_text("open-palette history b:11|c:12|f:13")
                .expect("parse should succeed"),
            Command::OpenPalette {
                kind: PaletteKind::History,
                payload: Some(PaletteOpenPayload::HistorySeed(
                    "b:11|c:12|f:13".to_string(),
                )),
            }
        );
        assert_eq!(
            parse_command_text("open-palette search needle").expect("parse should succeed"),
            Command::OpenPalette {
                kind: PaletteKind::Search,
                payload: Some(PaletteOpenPayload::Search {
                    query: "needle".to_string(),
                    matcher: SearchMatcherKind::ContainsInsensitive,
                }),
            }
        );
        assert_eq!(
            parse_command_text("open-palette outline appendix").expect("parse should succeed"),
            Command::OpenPalette {
                kind: PaletteKind::Outline,
                payload: Some(PaletteOpenPayload::OutlineQuery("appendix".to_string())),
            }
        );
        assert_eq!(
            parse_command_text("open-palette search-results needle").expect("parse should succeed"),
            Command::OpenPalette {
                kind: PaletteKind::SearchResults,
                payload: Some(PaletteOpenPayload::SearchResultsQuery("needle".to_string())),
            }
        );
    }

    #[test]
    fn parse_search_without_args_opens_search_palette() {
        assert_eq!(
            parse_command_text("search").expect("parse should succeed"),
            Command::OpenSearch
        );
        assert_eq!(
            parse_command_text("search-results").expect("parse should succeed"),
            Command::OpenSearchResults
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
    fn parse_search_goto_accepts_page() {
        assert_eq!(
            parse_command_text("search-goto 8").expect("parse should succeed"),
            Command::SearchResultGoto { page: 8 }
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
    fn parse_page_layout_commands_support_mode_and_direction() {
        assert_eq!(
            parse_command_text("layout-single").expect("parse should succeed"),
            Command::PageLayoutSingle
        );
        assert_eq!(
            parse_command_text("layout-spread").expect("parse should succeed"),
            Command::PageLayoutSpread {
                direction: None,
                cover_policy: None,
            }
        );
        assert_eq!(
            parse_command_text("layout-spread rtl").expect("parse should succeed"),
            Command::PageLayoutSpread {
                direction: Some(SpreadDirectionArg::Rtl),
                cover_policy: None,
            }
        );
        assert_eq!(
            parse_command_text("layout-spread rtl cover").expect("parse should succeed"),
            Command::PageLayoutSpread {
                direction: Some(SpreadDirectionArg::Rtl),
                cover_policy: Some(SpreadCoverPolicyArg::Cover),
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
    fn no_arg_public_command_specs_parse_by_id() {
        for spec in all_command_specs()
            .into_iter()
            .filter(|spec| spec.exposure == CommandExposure::Public && spec.args.is_empty())
        {
            let command = parse_command_text(spec.id).unwrap_or_else(|err| {
                panic!("{} should parse without arguments: {}", spec.id, err)
            });
            assert_eq!(command.id(), spec.id);
        }
    }

    #[test]
    fn specs_with_required_args_reject_missing_args() {
        for spec in all_command_specs()
            .into_iter()
            .filter(|spec| spec.args.iter().any(|arg| arg.required))
        {
            assert!(
                parse_command_text(spec.id).is_err(),
                "{} should reject missing required arguments",
                spec.id
            );
        }
    }

    #[test]
    fn enum_argument_hints_are_accepted_by_parser() {
        for spec in all_command_specs() {
            for (arg_index, arg) in spec.args.iter().enumerate() {
                let ArgHint::Enum(values) = arg.hint else {
                    continue;
                };

                for value in values() {
                    let command_text = command_text_with_arg_value(&spec, arg_index, value);
                    let command = parse_command_text(&command_text).unwrap_or_else(|err| {
                        panic!("{command_text:?} should parse enum value {value:?}: {err}")
                    });
                    assert_eq!(command.id(), spec.id);
                }
            }
        }
    }

    #[test]
    fn first_token_ignores_leading_whitespace() {
        assert_eq!(first_token("  next-page"), "next-page");
        assert_eq!(first_token("\tzoom 1.5"), "zoom");
    }

    fn command_text_with_arg_value(
        spec: &crate::command::CommandSpec,
        arg_index: usize,
        value: &str,
    ) -> String {
        let args = spec
            .args
            .iter()
            .enumerate()
            .filter_map(|(index, arg)| {
                if index == arg_index {
                    return Some(value);
                }
                if index < arg_index {
                    return sample_arg(arg);
                }
                required_arg_sample(arg)
            })
            .collect::<Vec<_>>();

        if args.is_empty() {
            spec.id.to_string()
        } else {
            format!("{} {}", spec.id, args.join(" "))
        }
    }

    fn required_arg_sample(arg: &ArgSpec) -> Option<&'static str> {
        if !arg.required {
            return None;
        }

        sample_arg(arg)
    }

    fn sample_arg(arg: &ArgSpec) -> Option<&'static str> {
        Some(match arg.kind {
            ArgKind::F32 => "1.25",
            ArgKind::I32 => "1",
            ArgKind::String => match arg.hint {
                ArgHint::Enum(values) => values()[0],
                ArgHint::None => "value",
            },
        })
    }
}
