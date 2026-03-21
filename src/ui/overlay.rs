use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::palette::{PaletteItemView, PaletteView};

use super::layout::centered_rect;

pub fn draw_loading_overlay(frame: &mut Frame<'_>, area: Rect, label: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_width = area.width.min(34);
    let popup_height = area.height.min(5);
    let popup = centered_rect(area, popup_width, popup_height);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title("Loading")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let message = Paragraph::new(format!("Loading... {label}"))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White));
    frame.render_widget(message, inner);
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

    if item.selected {
        spans.push(Span::styled(" ┃ ", Style::default().fg(Color::White)));
    } else {
        spans.push(Span::raw("   "));
    }

    spans.push(Span::raw(item.label.clone()));

    if let Some(detail) = &item.detail {
        spans.push(Span::raw(" "));
        let detail_style = if item.selected {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(detail.clone(), detail_style));
    }

    let line_style = if item.selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };

    let label_width = UnicodeWidthStr::width(item.label.as_str());
    let detail_width = item
        .detail
        .as_ref()
        .map(|detail| 1 + UnicodeWidthStr::width(detail.as_str()))
        .unwrap_or(0);
    let total_width = 3 + label_width + detail_width;
    let padding = " ".repeat(width.saturating_sub(total_width));
    spans.push(Span::raw(padding));

    Line::from(spans).style(line_style)
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

    use crate::palette::{PaletteItemView, PaletteKind, PaletteView};

    use super::{build_palette_input_line, build_palette_item_line, draw_palette_overlay};

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

    fn test_view(input: &str, cursor: usize) -> PaletteView {
        PaletteView {
            title: "Command".to_string(),
            kind: PaletteKind::Command,
            input: input.to_string(),
            cursor,
            assistive_text: None,
            items: vec![PaletteItemView {
                label: "open".to_string(),
                detail: None,
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
                label: "goto-page".to_string(),
                detail: Some("<page> | Jump".to_string()),
                selected: false,
            },
            40,
        );

        assert_eq!(rendered_candidate_text(&line), "goto-page <page> | Jump");
    }
}
