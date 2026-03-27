use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::input::keymap::KeymapPreset;
use crate::input::shortcut::{ShortcutKey, format_shortcut_sequence};

use super::layout::centered_rect;
use super::{border, heading_text, primary_text, secondary_text};

pub fn draw_help_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    preset: KeymapPreset,
    scroll_offset: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let popup_width = area.width.min(72);
    let popup_height = area.height.clamp(10, 28);
    let popup = centered_rect(area, popup_width, popup_height);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner)[1];
    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let lines = build_help_lines(preset);
    let scroll = scroll_offset.min(help_rendered_height(
        &lines,
        content_area.width,
        content_area.height,
    )) as u16;
    let content = Paragraph::new(lines)
        .style(primary_text())
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left)
        .scroll((scroll, 0));

    frame.render_widget(content, content_area);
}

#[derive(Debug, Clone, Copy)]
struct HelpSection {
    title: &'static str,
    rows: &'static [HelpRow],
}

#[derive(Debug, Clone, Copy)]
struct HelpRow {
    keys: &'static [ShortcutKey],
    description: &'static str,
}

const DEFAULT_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::char('j')],
                description: "Next page",
            },
            HelpRow {
                keys: &[ShortcutKey::char('k')],
                description: "Previous page",
            },
            HelpRow {
                keys: &[ShortcutKey::char('g')],
                description: "First page",
            },
            HelpRow {
                keys: &[ShortcutKey::char('G')],
                description: "Last page",
            },
            HelpRow {
                keys: &[ShortcutKey::ctrl('o')],
                description: "History back",
            },
            HelpRow {
                keys: &[ShortcutKey::ctrl('i')],
                description: "History forward",
            },
        ],
    },
    HelpSection {
        title: "View",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::char('+')],
                description: "Zoom in",
            },
            HelpRow {
                keys: &[ShortcutKey::char('-')],
                description: "Zoom out",
            },
            HelpRow {
                keys: &[
                    ShortcutKey::char('H'),
                    ShortcutKey::char('J'),
                    ShortcutKey::char('K'),
                    ShortcutKey::char('L'),
                ],
                description: "Scroll",
            },
        ],
    },
    HelpSection {
        title: "Search",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::char('/')],
                description: "Search",
            },
            HelpRow {
                keys: &[ShortcutKey::char('n')],
                description: "Next search hit",
            },
            HelpRow {
                keys: &[ShortcutKey::char('N')],
                description: "Previous search hit",
            },
        ],
    },
    HelpSection {
        title: "Other",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::char(':')],
                description: "Command palette",
            },
            HelpRow {
                keys: &[ShortcutKey::char('?')],
                description: "Help",
            },
            HelpRow {
                keys: &[ShortcutKey::char('q')],
                description: "Quit",
            },
            HelpRow {
                keys: &[ShortcutKey::key(crossterm::event::KeyCode::Esc)],
                description: "Cancel / Close",
            },
        ],
    },
];

const EMACS_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::ctrl('n')],
                description: "Next page",
            },
            HelpRow {
                keys: &[ShortcutKey::ctrl('p')],
                description: "Previous page",
            },
            HelpRow {
                keys: &[ShortcutKey::alt('v')],
                description: "Previous page",
            },
            HelpRow {
                keys: &[ShortcutKey::key(crossterm::event::KeyCode::PageDown)],
                description: "Next page",
            },
            HelpRow {
                keys: &[ShortcutKey::key(crossterm::event::KeyCode::PageUp)],
                description: "Previous page",
            },
            HelpRow {
                keys: &[ShortcutKey::char('j')],
                description: "Next page",
            },
            HelpRow {
                keys: &[ShortcutKey::char('k')],
                description: "Previous page",
            },
            HelpRow {
                keys: &[ShortcutKey::ctrl('o')],
                description: "History back",
            },
            HelpRow {
                keys: &[ShortcutKey::ctrl('i')],
                description: "History forward",
            },
        ],
    },
    HelpSection {
        title: "View",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::char('+')],
                description: "Zoom in",
            },
            HelpRow {
                keys: &[ShortcutKey::char('-')],
                description: "Zoom out",
            },
            HelpRow {
                keys: &[
                    ShortcutKey::char('H'),
                    ShortcutKey::char('J'),
                    ShortcutKey::char('K'),
                    ShortcutKey::char('L'),
                ],
                description: "Scroll",
            },
        ],
    },
    HelpSection {
        title: "Search",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::ctrl('s')],
                description: "Search",
            },
            HelpRow {
                keys: &[ShortcutKey::char('n')],
                description: "Next search hit",
            },
            HelpRow {
                keys: &[ShortcutKey::char('N')],
                description: "Previous search hit",
            },
        ],
    },
    HelpSection {
        title: "Other",
        rows: &[
            HelpRow {
                keys: &[ShortcutKey::alt('x')],
                description: "Command palette",
            },
            HelpRow {
                keys: &[ShortcutKey::char('?')],
                description: "Help",
            },
            HelpRow {
                keys: &[ShortcutKey::char('q')],
                description: "Quit",
            },
            HelpRow {
                keys: &[ShortcutKey::key(crossterm::event::KeyCode::Esc)],
                description: "Cancel / Close",
            },
        ],
    },
];

fn build_help_lines(preset: KeymapPreset) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let sections = match preset {
        KeymapPreset::Default => DEFAULT_SECTIONS,
        KeymapPreset::Emacs => EMACS_SECTIONS,
    };

    for (i, section) in sections.iter().enumerate() {
        lines.push(Line::from(vec![Span::styled(
            section.title,
            heading_text(),
        )]));
        for row in section.rows {
            lines.push(render_help_row(row));
        }

        if i + 1 < sections.len() {
            lines.push(Line::from(""));
        }
    }

    lines
}

fn render_help_row(row: &HelpRow) -> Line<'static> {
    let key_text = format_shortcut_sequence(row.keys);
    let key_span = Span::styled(format!("{key_text:<18}"), secondary_text());
    Line::from(vec![key_span, Span::raw(row.description.to_string())])
}

fn help_rendered_height(lines: &[Line<'static>], width: u16, height: u16) -> usize {
    if width == 0 {
        return 0;
    }

    let rendered_lines: usize = lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(usize::from(width))
            }
        })
        .sum();

    rendered_lines.saturating_sub(usize::from(height))
}

#[cfg(test)]
mod tests {
    use crate::input::keymap::KeymapPreset;

    use super::{build_help_lines, help_rendered_height};

    #[test]
    fn help_lines_include_current_preset_bindings() {
        let default_lines = build_help_lines(KeymapPreset::Default);
        let default_text = default_lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(default_text.contains("Ctrl+O"));
        assert!(default_text.contains("Help"));

        let emacs_lines = build_help_lines(KeymapPreset::Emacs);
        let emacs_text = emacs_lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(emacs_text.contains("Ctrl+N"));
        assert!(emacs_text.contains("Alt+X"));
        assert!(emacs_text.contains("PgDn"));
    }

    #[test]
    fn help_scroll_limit_accounts_for_wrapping() {
        let raw_limit = build_help_lines(KeymapPreset::Default)
            .len()
            .saturating_sub(5);
        let wrapped_limit = help_rendered_height(&build_help_lines(KeymapPreset::Default), 20, 5);

        assert!(wrapped_limit > raw_limit);
        assert!(wrapped_limit > 0);
    }
}
