use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_segmentation::UnicodeSegmentation;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

use crate::palette::{PaletteItemView, PaletteView};

use super::layout::centered_rect;
use super::{border, error_text, hit_highlight_text, primary_text, secondary_text};

const PALETTE_ITEM_DECORATION_WIDTH: usize = 3;
const MIN_VISIBLE_SIDE_WIDTH: usize = 4;
const MIN_PALETTE_COLUMN_GAP: usize = 1;
const ELLIPSIS: &str = "…";

pub fn draw_loading_overlay(frame: &mut Frame<'_>, area: Rect, label: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_width = area.width.min(28);
    let popup_height = area.height.min(3);
    let popup = centered_rect(area, popup_width, popup_height);
    frame.render_widget(Clear, popup);

    if popup.width == 0 || popup.height == 0 {
        return;
    }

    let message = build_loading_message(label, popup.width as usize);
    let message_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(popup)[1];
    let message = Paragraph::new(message).style(primary_text());
    frame.render_widget(message, message_area);
}

pub fn draw_error_overlay(frame: &mut Frame<'_>, area: Rect, message: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_width = area.width.min(52);
    let popup_height = area.height.min(6);
    let popup = centered_rect(area, popup_width, popup_height);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title("Unable to display")
        .borders(Borders::ALL)
        .style(error_text());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (text, _) = message.unicode_truncate(inner.width as usize);
    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Center)
        .style(error_text());
    frame.render_widget(paragraph, inner);
}

pub fn draw_palette_overlay(frame: &mut Frame<'_>, area: Rect, view: &PaletteView) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_width = area.width.min(72);
    let popup_height = area.height.clamp(7, 24);
    let popup = centered_rect(area, popup_width, popup_height);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(format!(" {} ", view.title))
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height < 3 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Input
            Constraint::Length(1), // Separator
            Constraint::Min(1),    // List/Candidates
        ])
        .split(inner);

    // 1. Input line
    let input_layout = build_palette_input_line(&view.input, view.cursor, chunks[0].width as usize);
    frame.render_widget(Paragraph::new(input_layout.line), chunks[0]);
    frame.set_cursor_position((chunks[0].x + input_layout.cursor_col, chunks[0].y));

    // 2. Separator
    let sep_style = secondary_text();
    let sep_char = "─";
    frame.render_widget(
        Paragraph::new(sep_char.repeat(inner.width as usize)).style(sep_style),
        chunks[1],
    );

    // 3. Candidates List
    let list_area = chunks[2];
    let mut lines = Vec::new();

    // Assistive text if any
    let mut overhead_lines = 0;
    if let Some(assistive) = &view.assistive_text
        && !assistive.is_empty()
    {
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(assistive, secondary_text()),
        ]));
        overhead_lines += 1;
    }

    if !view.items.is_empty() {
        let max_items = (list_area.height as usize).saturating_sub(overhead_lines);
        if max_items > 0 {
            debug_assert!(
                view.selected_idx.is_some(),
                "non-empty palette view must have a selected item"
            );
            let selected_idx = view
                .selected_idx
                .unwrap_or(0)
                .min(view.items.len().saturating_sub(1));

            // Simple scroll logic: ensure selected_idx is within [start, start + max_items)
            let start_idx = if view.items.len() <= max_items || selected_idx < max_items / 2 {
                0
            } else if selected_idx >= view.items.len() - max_items / 2 {
                view.items.len().saturating_sub(max_items)
            } else {
                selected_idx.saturating_sub(max_items / 2)
            };

            for item in view.items.iter().skip(start_idx).take(max_items) {
                lines.push(build_palette_item_line(item, inner.width as usize));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), list_area);
}

fn build_palette_item_line(item: &PaletteItemView, width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let content_width = width.saturating_sub(PALETTE_ITEM_DECORATION_WIDTH);

    if item.selected {
        spans.push(Span::styled(" ┃ ", selected_text_style()));
    } else {
        spans.push(Span::raw("   "));
    }

    let rendered = render_palette_row(item, content_width);
    spans.extend(rendered.label.spans);
    if rendered.gap > 0 {
        if item.selected {
            spans.push(Span::styled(
                " ".repeat(rendered.gap),
                selected_text_style(),
            ));
        } else {
            spans.push(Span::raw(" ".repeat(rendered.gap)));
        }
    }
    spans.extend(rendered.detail.spans);
    if rendered.trailing_padding > 0 {
        if item.selected {
            spans.push(Span::styled(
                " ".repeat(rendered.trailing_padding),
                selected_text_style(),
            ));
        } else {
            spans.push(Span::raw(" ".repeat(rendered.trailing_padding)));
        }
    }

    Line::from(spans)
}

struct RenderedTextParts {
    spans: Vec<Span<'static>>,
    width: usize,
}

impl RenderedTextParts {
    fn empty() -> Self {
        Self {
            spans: Vec::new(),
            width: 0,
        }
    }
}

struct RenderedPaletteRow {
    label: RenderedTextParts,
    gap: usize,
    detail: RenderedTextParts,
    trailing_padding: usize,
}

enum PaletteRowPlan {
    Empty,
    Single {
        text_width: usize,
        trailing_padding: usize,
    },
    Split {
        left_width: usize,
        right_width: usize,
        gap: usize,
        trailing_padding: usize,
    },
}

fn render_palette_row(item: &PaletteItemView, content_width: usize) -> RenderedPaletteRow {
    let plan = plan_palette_row(item, content_width);
    match plan {
        PaletteRowPlan::Empty => RenderedPaletteRow {
            label: RenderedTextParts::empty(),
            gap: 0,
            detail: RenderedTextParts::empty(),
            trailing_padding: 0,
        },
        PaletteRowPlan::Single {
            text_width,
            trailing_padding,
        } => {
            let left = render_palette_text_parts(&item.label, text_width, item.selected);
            let gap = text_width.saturating_sub(left.width);
            RenderedPaletteRow {
                label: left,
                gap,
                detail: RenderedTextParts::empty(),
                trailing_padding,
            }
        }
        PaletteRowPlan::Split {
            left_width,
            right_width,
            gap,
            trailing_padding,
        } => {
            let left = render_palette_text_parts(&item.label, left_width, item.selected);
            let right = render_palette_text_parts(&item.detail, right_width, item.selected);
            let gap = gap
                .saturating_add(left_width.saturating_sub(left.width))
                .saturating_add(right_width.saturating_sub(right.width));
            RenderedPaletteRow {
                label: left,
                gap,
                detail: right,
                trailing_padding,
            }
        }
    }
}

fn plan_palette_row(item: &PaletteItemView, content_width: usize) -> PaletteRowPlan {
    if content_width == 0 {
        return PaletteRowPlan::Empty;
    }

    let trailing_padding = 1.min(content_width);
    let text_width = content_width.saturating_sub(trailing_padding);
    if text_width == 0 {
        return PaletteRowPlan::Empty;
    }

    let left_width = measure_palette_text_width(&item.label);
    let right_width = measure_palette_text_width(&item.detail);

    if item.detail.is_empty() {
        return PaletteRowPlan::Single {
            text_width,
            trailing_padding,
        };
    }

    if left_width
        .saturating_add(right_width)
        .saturating_add(MIN_PALETTE_COLUMN_GAP)
        <= text_width
    {
        // Keep both sides when they fit naturally; only collapse to single-side
        // rendering when the row is too narrow to keep both fragments legible.
        let gap = text_width.saturating_sub(left_width + right_width);
        return PaletteRowPlan::Split {
            left_width,
            right_width,
            gap,
            trailing_padding,
        };
    }

    if text_width < MIN_VISIBLE_SIDE_WIDTH * 2 {
        return PaletteRowPlan::Single {
            text_width,
            trailing_padding,
        };
    }

    let gap = MIN_PALETTE_COLUMN_GAP.min(text_width);
    let available = text_width.saturating_sub(gap);
    if available < MIN_VISIBLE_SIDE_WIDTH * 2 {
        return PaletteRowPlan::Single {
            text_width,
            trailing_padding,
        };
    }

    let mut right_target = right_width
        .min((available / 3).max(MIN_VISIBLE_SIDE_WIDTH))
        .min(available.saturating_sub(MIN_VISIBLE_SIDE_WIDTH));
    let mut left_target = available.saturating_sub(right_target);

    if left_target < MIN_VISIBLE_SIDE_WIDTH {
        left_target = MIN_VISIBLE_SIDE_WIDTH;
        right_target = available.saturating_sub(left_target);
    }

    if right_target < MIN_VISIBLE_SIDE_WIDTH {
        right_target = MIN_VISIBLE_SIDE_WIDTH.min(available.saturating_sub(MIN_VISIBLE_SIDE_WIDTH));
        left_target = available.saturating_sub(right_target);
    }

    if left_target == 0 || right_target == 0 {
        return PaletteRowPlan::Single {
            text_width,
            trailing_padding,
        };
    }

    PaletteRowPlan::Split {
        left_width: left_target,
        right_width: right_target,
        gap,
        trailing_padding,
    }
}

fn render_palette_text_parts(
    parts: &[crate::palette::PaletteTextPart],
    max_width: usize,
    selected: bool,
) -> RenderedTextParts {
    let mut spans = Vec::new();
    let mut remaining = max_width;
    let mut width = 0usize;

    for part in parts {
        if remaining == 0 {
            break;
        }

        let part_width = UnicodeWidthStr::width(part.text.as_str());
        if part_width <= remaining {
            spans.push(styled_text_part(part.text.clone(), part.tone, selected));
            width = width.saturating_add(part_width);
            remaining -= part_width;
            continue;
        }

        let truncated = truncate_with_ellipsis(&part.text, remaining);
        if !truncated.is_empty() {
            width = width.saturating_add(UnicodeWidthStr::width(truncated.as_str()));
            spans.push(styled_text_part(truncated, part.tone, selected));
        }
        break;
    }

    RenderedTextParts { spans, width }
}

fn measure_palette_text_width(parts: &[crate::palette::PaletteTextPart]) -> usize {
    parts
        .iter()
        .map(|part| UnicodeWidthStr::width(part.text.as_str()))
        .sum()
}

fn styled_text_part(
    text: String,
    tone: crate::palette::PaletteTextTone,
    selected: bool,
) -> Span<'static> {
    let style = palette_text_style(tone, selected);
    Span::styled(text, style)
}

fn palette_text_style(tone: crate::palette::PaletteTextTone, selected: bool) -> Style {
    if selected {
        return selected_text_style();
    }

    match tone {
        crate::palette::PaletteTextTone::Primary => primary_text(),
        crate::palette::PaletteTextTone::Secondary => secondary_text(),
        crate::palette::PaletteTextTone::Highlight => hit_highlight_text(),
    }
}

fn selected_text_style() -> Style {
    primary_text().add_modifier(Modifier::REVERSED)
}

fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let text_width = UnicodeWidthStr::width(text);
    if text_width <= max_width {
        return text.to_string();
    }
    let ellipsis_width = UnicodeWidthStr::width(ELLIPSIS);
    if max_width <= ellipsis_width {
        return ELLIPSIS.to_string();
    }

    let (prefix, _) = text.unicode_truncate(max_width - ellipsis_width);
    format!("{prefix}{ELLIPSIS}")
}

fn build_loading_message(label: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let message = format!("Loading {label}");
    let (text, text_width) = message.unicode_truncate(width);
    let left_padding = width.saturating_sub(text_width) / 2;
    let right_padding = width
        .saturating_sub(text_width)
        .saturating_sub(left_padding);

    format!(
        "{}{}{}",
        " ".repeat(left_padding),
        text,
        " ".repeat(right_padding)
    )
}

struct PaletteInputLineLayout {
    line: Line<'static>,
    cursor_col: u16,
}

fn build_palette_input_line(input: &str, cursor: usize, width: usize) -> PaletteInputLineLayout {
    let prefix_spans = vec![
        Span::raw(" ".to_string()),
        Span::styled("> ".to_string(), primary_text()),
    ];
    let prefix_width = 3;
    let max_text_width = width.saturating_sub(prefix_width);

    if max_text_width == 0 {
        return PaletteInputLineLayout {
            line: Line::from(prefix_spans),
            cursor_col: 0,
        };
    }

    #[derive(Clone)]
    struct Glyph {
        symbol: String,
        start: usize,
        end: usize,
        width: usize,
    }

    let mut glyphs = Vec::new();
    let mut total_width = 0usize;
    for grapheme in input.graphemes(true) {
        let cell_width = UnicodeWidthStr::width(grapheme);
        let start = total_width;
        total_width = total_width.saturating_add(cell_width);
        glyphs.push(Glyph {
            symbol: grapheme.to_string(),
            start,
            end: total_width,
            width: cell_width,
        });
    }
    let cursor = cursor.min(total_width);

    let mut start_col = if cursor >= max_text_width {
        cursor.saturating_sub(max_text_width.saturating_sub(1))
    } else {
        0
    };
    if let Some(glyph) = glyphs
        .iter()
        .find(|glyph| glyph.start < start_col && start_col < glyph.end)
    {
        start_col = glyph.start;
    }
    let end_col = start_col.saturating_add(max_text_width);

    let mut spans = prefix_spans;
    let mut consumed = 0usize;
    for glyph in &glyphs {
        if glyph.end <= start_col {
            continue;
        }
        if glyph.start >= end_col || glyph.end > end_col {
            break;
        }
        spans.push(Span::raw(glyph.symbol.clone()));
        consumed = consumed.saturating_add(glyph.width);
    }

    if consumed < max_text_width {
        spans.push(Span::raw(" ".repeat(max_text_width - consumed)));
    }

    let cursor_rel = cursor
        .saturating_sub(start_col)
        .min(max_text_width.saturating_sub(1));
    let cursor_col = prefix_width.saturating_add(cursor_rel) as u16;

    PaletteInputLineLayout {
        line: Line::from(spans),
        cursor_col,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::{Backend, TestBackend};
    use ratatui::layout::Rect;
    use unicode_width::UnicodeWidthStr;

    use crate::palette::{
        PaletteItemView, PaletteKind, PaletteTextPart, PaletteTextTone, PaletteView,
    };

    use super::{
        build_loading_message, build_palette_input_line, build_palette_item_line,
        draw_error_overlay, draw_loading_overlay, draw_palette_overlay,
    };

    fn rendered_input_text(layout: &super::PaletteInputLineLayout) -> String {
        layout
            .line
            .spans
            .iter()
            .skip(2)
            .map(|span| span.content.as_ref())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn rendered_candidate_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .skip(1)
            .map(|span| span.content.as_ref())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn rendered_candidate_width(line: &ratatui::text::Line<'_>) -> usize {
        line.spans
            .iter()
            .skip(1)
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum()
    }

    fn test_view(input: &str, cursor: usize) -> PaletteView {
        PaletteView {
            title: "Command".to_string(),
            kind: PaletteKind::Command,
            input: input.to_string(),
            cursor,
            assistive_text: None,
            items: vec![PaletteItemView {
                label: Vec::new(),
                detail: Vec::new(),
                selected: true,
            }],
            selected_idx: Some(0),
        }
    }

    fn palette_item(label: &str, detail: &str, selected: bool) -> PaletteItemView {
        PaletteItemView {
            label: vec![PaletteTextPart {
                text: label.to_string(),
                tone: PaletteTextTone::Primary,
            }],
            detail: (!detail.is_empty())
                .then(|| PaletteTextPart {
                    text: detail.to_string(),
                    tone: PaletteTextTone::Secondary,
                })
                .into_iter()
                .collect(),
            selected,
        }
    }

    #[test]
    fn palette_input_layout_preserves_graphemes_and_keeps_cursor_visible() {
        let cases = [
            ("character", "abc", 1, 12, "abc", 4),
            ("end", "abc", 3, 12, "abc", 6),
            ("inside wide character", "あい", 1, 12, "あい", 4),
            ("wide character boundary", "あい", 2, 12, "あい", 5),
            ("combining sequence", "e\u{301}", 0, 12, "e\u{301}", 3),
            (
                "ZWJ emoji sequence",
                "👩\u{200d}💻",
                0,
                12,
                "👩\u{200d}💻",
                3,
            ),
            ("scrolled input", "abcdefghij", 10, 8, "ghij", 7),
        ];

        for (name, input, cursor, width, expected_text, expected_cursor_col) in cases {
            let layout = build_palette_input_line(input, cursor, width);
            assert_eq!(rendered_input_text(&layout), expected_text, "{name}");
            assert_eq!(layout.cursor_col, expected_cursor_col, "{name}");
        }
    }

    #[test]
    fn draw_palette_overlay_sets_terminal_cursor_position() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                draw_palette_overlay(frame, Rect::new(0, 0, 30, 10), &test_view("abc", 1));
            })
            .expect("draw should pass");

        let position = terminal
            .backend_mut()
            .get_cursor_position()
            .expect("cursor position should be available");
        assert_eq!((position.x, position.y), (5, 1));
    }

    #[test]
    fn palette_item_line_preserves_content_and_row_width_contracts() {
        let cases = [
            (
                "detail with gap",
                "goto-page",
                "Jump",
                false,
                40,
                false,
                true,
                None,
            ),
            (
                "long label keeps detail",
                "very long outline title",
                "p.12",
                false,
                18,
                true,
                true,
                None,
            ),
            (
                "tight row hides detail",
                "outline",
                "p.12",
                false,
                8,
                true,
                false,
                None,
            ),
            (
                "row fills available width",
                "open",
                "Command",
                true,
                20,
                false,
                true,
                Some(17),
            ),
            (
                "wide truncated label fills row",
                "界界界界界界界界界界",
                "p.9",
                false,
                18,
                true,
                true,
                Some(15),
            ),
            (
                "exact fit preserves column gap",
                "abcdefgh",
                "p.12",
                false,
                16,
                true,
                true,
                Some(13),
            ),
            (
                "row reserves trailing padding",
                "open",
                "",
                false,
                12,
                false,
                false,
                Some(9),
            ),
        ];

        for (name, label, detail, selected, width, elided, detail_visible, expected_width) in cases
        {
            let line = build_palette_item_line(&palette_item(label, detail, selected), width);
            let rendered = rendered_candidate_text(&line);

            assert_eq!(rendered.contains('…'), elided, "{name}");
            if detail_visible {
                assert!(rendered.ends_with(detail), "{name}: {rendered:?}");
                assert!(
                    rendered.contains(&format!(" {detail}")),
                    "{name}: {rendered:?}"
                );
            } else if !detail.is_empty() {
                assert!(!rendered.contains(detail), "{name}: {rendered:?}");
            }
            if let Some(expected_width) = expected_width {
                assert_eq!(rendered_candidate_width(&line), expected_width, "{name}");
            }
        }
    }

    #[test]
    fn palette_item_line_selected_uses_uniform_style() {
        let line = build_palette_item_line(
            &PaletteItemView {
                label: vec![
                    crate::palette::PaletteTextPart {
                        text: "open".to_string(),
                        tone: crate::palette::PaletteTextTone::Primary,
                    },
                    crate::palette::PaletteTextPart {
                        text: " now".to_string(),
                        tone: crate::palette::PaletteTextTone::Secondary,
                    },
                ],
                detail: vec![crate::palette::PaletteTextPart {
                    text: "Command".to_string(),
                    tone: crate::palette::PaletteTextTone::Secondary,
                }],
                selected: true,
            },
            24,
        );

        let expected = super::selected_text_style();
        assert!(line.spans.iter().all(|span| span.style == expected));
    }

    #[test]
    fn loading_overlay_uses_fixed_width_for_short_and_long_labels() {
        let short = build_loading_message("page 1/9", 28);
        let long = build_loading_message("page 123456789/999999", 28);

        assert_eq!(UnicodeWidthStr::width(short.as_str()), 28);
        assert_eq!(UnicodeWidthStr::width(long.as_str()), 28);
        assert!(short.contains("Loading page 1/9"));
        assert!(long.contains("Loading page 123456789/999"));
    }

    #[test]
    fn loading_overlay_keeps_message_centered_with_vertical_padding() {
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                draw_loading_overlay(frame, Rect::new(0, 0, 40, 7), "page 1/9");
            })
            .expect("draw should pass");

        let buffer = terminal.backend().buffer();
        let message_y = 3;
        let message = (0..buffer.area.width)
            .map(|x| buffer[(x, message_y)].symbol())
            .collect::<String>();

        assert!(message.contains("Loading page 1/9"));
        assert!(message.starts_with("      "));
        assert!(message.ends_with("      "));

        let blank_y = 2;
        let blank_line = (0..buffer.area.width)
            .map(|x| buffer[(x, blank_y)].symbol())
            .collect::<String>();
        assert_eq!(blank_line, " ".repeat(buffer.area.width as usize));
    }

    #[test]
    fn error_overlay_uses_friendly_title_and_message() {
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                draw_error_overlay(frame, Rect::new(0, 0, 40, 7), "Could not render p.12.");
            })
            .expect("draw should pass");

        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Unable to display"));
        assert!(rendered.contains("Could not render p.12."));
    }
}
