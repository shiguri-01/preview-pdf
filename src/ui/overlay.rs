use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::palette::PaletteView;

use super::layout::centered_rect;

pub fn draw_loading_overlay(frame: &mut Frame<'_>, area: Rect, page: usize) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_width = area.width.min(34);
    let popup_height = area.height.min(5);
    let popup = centered_rect(area, popup_width, popup_height);

    let block = Block::default()
        .title("Loading")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let message = Paragraph::new(format!("Loading... page {}", page))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White));
    frame.render_widget(message, inner);
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

    // 1. Input line (software caret to avoid terminal cursor ghosting/flicker)
    let input_line = build_palette_input_line(&view.input, view.cursor, chunks[0].width as usize);
    frame.render_widget(Paragraph::new(input_line), chunks[0]);

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
                let mut spans = Vec::new();

                // Selection indicator
                if item.selected {
                    spans.push(Span::styled(" ┃ ", Style::default().fg(Color::White)));
                } else {
                    spans.push(Span::raw("   "));
                }

                // Label
                spans.push(Span::raw(&item.label));

                // Detail
                if let Some(detail) = &item.detail {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(detail, Style::default().fg(Color::DarkGray)));
                }

                let line_style = if item.selected {
                    Style::default().bg(Color::Rgb(45, 45, 50))
                } else {
                    Style::default()
                };

                // Create a padded line to ensure the background covers the full width
                let label_len = item.label.chars().count();
                let detail_len = item
                    .detail
                    .as_ref()
                    .map(|d| d.chars().count() + 2)
                    .unwrap_or(0);
                let total_len = 3 + label_len + detail_len;
                let padding = " ".repeat((inner.width as usize).saturating_sub(total_len));
                spans.push(Span::raw(padding));

                lines.push(Line::from(spans).style(line_style));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), list_area);
}

fn build_palette_input_line(input: &str, cursor: usize, width: usize) -> Line<'static> {
    let prefix_spans = vec![
        Span::raw(" ".to_string()),
        Span::styled("> ".to_string(), Style::default().fg(Color::White)),
    ];
    let prefix_width = 3;
    let max_text_width = width.saturating_sub(prefix_width);

    if max_text_width == 0 {
        return Line::from(prefix_spans);
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
    for ch in input.chars() {
        let Some(cell_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if cell_width == 0 {
            continue;
        }
        let start = total_width;
        total_width = total_width.saturating_add(cell_width);
        glyphs.push(Glyph {
            symbol: ch.to_string(),
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

    let caret_glyph = if cursor < total_width {
        glyphs
            .iter()
            .position(|glyph| glyph.start <= cursor && cursor < glyph.end)
    } else {
        None
    };

    let mut spans = prefix_spans;
    let mut consumed = 0usize;
    for (idx, glyph) in glyphs.iter().enumerate() {
        if glyph.end <= start_col {
            continue;
        }
        if glyph.start >= end_col || glyph.end > end_col {
            break;
        }
        let style = if Some(idx) == caret_glyph {
            Style::default().reversed()
        } else {
            Style::default()
        };
        spans.push(Span::styled(glyph.symbol.clone(), style));
        consumed = consumed.saturating_add(glyph.width);
    }

    if cursor == total_width {
        let caret_rel = cursor
            .saturating_sub(start_col)
            .min(max_text_width.saturating_sub(1));
        if consumed < caret_rel {
            spans.push(Span::raw(" ".repeat(caret_rel - consumed)));
            consumed = caret_rel;
        }
        if consumed < max_text_width {
            spans.push(Span::styled(" ".to_string(), Style::default().reversed()));
            consumed = consumed.saturating_add(1);
        }
    }

    if consumed < max_text_width {
        spans.push(Span::raw(" ".repeat(max_text_width - consumed)));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;

    use crate::palette::{PaletteItemView, PaletteKind, PaletteView};

    use super::{build_palette_input_line, draw_palette_overlay};

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
    fn palette_overlay_highlights_caret_on_character() {
        let line = build_palette_input_line("abc", 1, 12);
        assert_eq!(line.spans[3].content.as_ref(), "b");
        assert!(
            line.spans[3]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn palette_overlay_highlights_trailing_space_at_end_cursor() {
        let line = build_palette_input_line("abc", 3, 12);
        assert_eq!(line.spans[5].content.as_ref(), " ");
        assert!(
            line.spans[5]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
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
    fn palette_overlay_highlights_next_wide_char_at_boundary_cursor() {
        let line = build_palette_input_line("あい", 2, 12);
        assert_eq!(line.spans[3].content.as_ref(), "い");
        assert!(
            line.spans[3]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }
}
