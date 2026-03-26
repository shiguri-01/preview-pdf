use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::palette::{PaletteItemView, PaletteView};

use super::layout::centered_rect;

const PALETTE_ITEM_DECORATION_WIDTH: usize = 3;
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
    let message = Paragraph::new(message).style(Style::default().fg(Color::White));
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
        .title("Render Error")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Red));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let text = truncate_to_width(message, inner.width as usize);
    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White));
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
        .border_style(Style::default().fg(Color::DarkGray));
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
    let sep_style = Style::default().fg(Color::DarkGray);
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
            Span::styled(assistive, Style::default().fg(Color::DarkGray)),
        ]));
        overhead_lines += 1;
    }

    if !view.items.is_empty() {
        let max_items = (list_area.height as usize).saturating_sub(overhead_lines);
        if max_items > 0 {
            let selected_idx = view.selected_idx.min(view.items.len().saturating_sub(1));

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
    spans.extend(rendered.left.spans);
    if rendered.gap > 0 {
        spans.push(Span::raw(" ".repeat(rendered.gap)));
    }
    spans.extend(rendered.right.spans);
    if rendered.trailing_padding > 0 {
        spans.push(Span::raw(" ".repeat(rendered.trailing_padding)));
    }

    let line = Line::from(spans);
    if item.selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
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
    left: RenderedTextParts,
    gap: usize,
    right: RenderedTextParts,
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
            left: RenderedTextParts::empty(),
            gap: 0,
            right: RenderedTextParts::empty(),
            trailing_padding: 0,
        },
        PaletteRowPlan::Single {
            text_width,
            trailing_padding,
        } => {
            let left = render_palette_text_parts(&item.left, text_width, item.selected);
            let gap = text_width.saturating_sub(left.width);
            RenderedPaletteRow {
                left,
                gap,
                right: RenderedTextParts::empty(),
                trailing_padding,
            }
        }
        PaletteRowPlan::Split {
            left_width,
            right_width,
            gap,
            trailing_padding,
        } => RenderedPaletteRow {
            left: render_palette_text_parts(&item.left, left_width, item.selected),
            gap,
            right: render_palette_text_parts(&item.right, right_width, item.selected),
            trailing_padding,
        },
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

    let left_width = measure_palette_text_width(&item.left);
    let right_width = measure_palette_text_width(&item.right);

    if !item.right.is_empty()
        && left_width.saturating_add(right_width).saturating_add(1) <= text_width
    {
        return PaletteRowPlan::Split {
            left_width,
            right_width,
            gap: text_width.saturating_sub(left_width + right_width),
            trailing_padding,
        };
    }

    PaletteRowPlan::Single {
        text_width,
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
        crate::palette::PaletteTextTone::Primary => Style::default(),
        crate::palette::PaletteTextTone::Secondary => Style::default().fg(Color::DarkGray),
    }
}

fn selected_text_style() -> Style {
    Style::default().fg(Color::White)
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut width = 0usize;
    for grapheme in text.graphemes(true) {
        let w = UnicodeWidthStr::width(grapheme);
        if width.saturating_add(w) > max_width {
            break;
        }
        out.push_str(grapheme);
        width = width.saturating_add(w);
    }
    out
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

    let prefix = truncate_to_width(text, max_width - ellipsis_width);
    format!("{prefix}{ELLIPSIS}")
}

fn build_loading_message(label: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let text = truncate_to_width(&format!("Loading {label}"), width);
    let text_width = UnicodeWidthStr::width(text.as_str());
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
        Span::styled("> ".to_string(), Style::default().fg(Color::White)),
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

    use crate::palette::{PaletteItemView, PaletteKind, PaletteView};

    use super::{
        build_loading_message, build_palette_input_line, build_palette_item_line,
        draw_loading_overlay, draw_palette_overlay,
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
                left: Vec::new(),
                right: Vec::new(),
                selected: true,
            }],
            selected_idx: 0,
        }
    }

    #[test]
    fn palette_overlay_positions_cursor_on_character() {
        let layout = build_palette_input_line("abc", 1, 12);
        assert_eq!(layout.cursor_col, 4);
        assert_eq!(rendered_input_text(&layout), "abc");
    }

    #[test]
    fn palette_overlay_positions_cursor_at_end_of_input() {
        let layout = build_palette_input_line("abc", 3, 12);
        assert_eq!(layout.cursor_col, 6);
    }

    #[test]
    fn palette_overlay_handles_multibyte_input_without_panic() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        terminal
            .draw(|frame| {
                draw_palette_overlay(frame, Rect::new(0, 0, 30, 10), &test_view("あい", 1));
            })
            .expect("draw should pass");
    }

    #[test]
    fn palette_overlay_positions_cursor_at_wide_char_boundary() {
        let layout = build_palette_input_line("あい", 2, 12);
        assert_eq!(layout.cursor_col, 5);
    }

    #[test]
    fn palette_overlay_keeps_combining_sequence() {
        let layout = build_palette_input_line("e\u{301}", 0, 12);
        assert_eq!(rendered_input_text(&layout), "e\u{301}");
        assert_eq!(layout.cursor_col, 3);
    }

    #[test]
    fn palette_overlay_keeps_zwj_emoji_sequence() {
        let layout = build_palette_input_line("👩\u{200d}💻", 0, 12);
        assert_eq!(rendered_input_text(&layout), "👩\u{200d}💻");
        assert_eq!(layout.cursor_col, 3);
    }

    #[test]
    fn palette_overlay_scrolls_cursor_with_long_input() {
        let layout = build_palette_input_line("abcdefghij", 10, 8);
        assert_eq!(layout.cursor_col, 7);
        assert_eq!(rendered_input_text(&layout), "ghij");
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
    fn palette_item_line_uses_single_space_before_detail() {
        let line = build_palette_item_line(
            &PaletteItemView {
                left: vec![crate::palette::PaletteTextPart {
                    text: "goto-page".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                right: vec![crate::palette::PaletteTextPart {
                    text: "Jump".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                selected: false,
            },
            40,
        );

        let rendered = rendered_candidate_text(&line);
        assert!(rendered.starts_with("goto-page"));
        assert!(rendered.ends_with("Jump"));
        assert!(rendered.contains(" "));
    }

    #[test]
    fn palette_item_line_truncates_label_before_detail() {
        let line = build_palette_item_line(
            &PaletteItemView {
                left: vec![crate::palette::PaletteTextPart {
                    text: "very long outline title".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                right: vec![crate::palette::PaletteTextPart {
                    text: "p.12".to_string(),
                    tone: crate::palette::PaletteTextTone::Secondary,
                }],
                selected: false,
            },
            18,
        );

        let rendered = rendered_candidate_text(&line);
        assert!(rendered.contains("…"));
        assert!(rendered.starts_with("very long"));
    }

    #[test]
    fn palette_item_line_hides_detail_when_width_is_too_narrow() {
        let line = build_palette_item_line(
            &PaletteItemView {
                left: vec![crate::palette::PaletteTextPart {
                    text: "outline".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                right: vec![crate::palette::PaletteTextPart {
                    text: "p.12".to_string(),
                    tone: crate::palette::PaletteTextTone::Secondary,
                }],
                selected: false,
            },
            8,
        );

        assert!(rendered_candidate_text(&line).starts_with("out"));
    }

    #[test]
    fn palette_item_line_fills_full_row_width() {
        let line = build_palette_item_line(
            &PaletteItemView {
                left: vec![crate::palette::PaletteTextPart {
                    text: "open".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                right: vec![crate::palette::PaletteTextPart {
                    text: "Command".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                selected: true,
            },
            20,
        );

        assert_eq!(rendered_candidate_width(&line), 17);
    }

    #[test]
    fn palette_item_line_reserves_trailing_padding() {
        let line = build_palette_item_line(
            &PaletteItemView {
                left: vec![crate::palette::PaletteTextPart {
                    text: "open".to_string(),
                    tone: crate::palette::PaletteTextTone::Primary,
                }],
                right: Vec::new(),
                selected: false,
            },
            12,
        );

        assert_eq!(rendered_candidate_width(&line), 9);
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
}
