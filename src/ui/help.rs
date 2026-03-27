use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
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

    let popup_width = area.width.min(84);
    let popup_height = area.height.clamp(10, 28);
    let popup = centered_rect(area, popup_width, popup_height);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(format!(" Help · {} ", preset.id()))
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = build_help_lines(preset);
    let scroll = scroll_offset.min(lines.len().saturating_sub(inner.height as usize)) as u16;

    let content = Paragraph::new(lines)
        .style(primary_text())
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left)
        .scroll((scroll, 0));
    frame.render_widget(content, inner);
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
                description: "Open search",
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
                description: "Open help",
            },
            HelpRow {
                keys: &[ShortcutKey::char('q')],
                description: "Quit",
            },
            HelpRow {
                keys: &[ShortcutKey::key(crossterm::event::KeyCode::Esc)],
                description: "Close help or cancel",
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
                description: "Open search",
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
                description: "Open help",
            },
            HelpRow {
                keys: &[ShortcutKey::char('q')],
                description: "Quit",
            },
            HelpRow {
                keys: &[ShortcutKey::key(crossterm::event::KeyCode::Esc)],
                description: "Close help or cancel",
            },
        ],
    },
];

fn build_help_lines(preset: KeymapPreset) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![Span::styled("Basic shortcuts", heading_text())]),
        Line::from(""),
    ];

    let sections = match preset {
        KeymapPreset::Default => DEFAULT_SECTIONS,
        KeymapPreset::Emacs => EMACS_SECTIONS,
    };

    for section in sections {
        lines.push(Line::from(vec![Span::styled(
            section.title,
            heading_text(),
        )]));
        for row in section.rows {
            lines.push(render_help_row(row));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![Span::styled(
        "j/k scroll  PgUp/PgDn page  Esc close",
        secondary_text(),
    )]));

    lines
}

fn render_help_row(row: &HelpRow) -> Line<'static> {
    let key_text = format_shortcut_sequence(row.keys);
    let key_span = Span::styled(format!("{key_text:<18}"), heading_text());
    Line::from(vec![key_span, Span::raw(row.description.to_string())])
}

#[cfg(test)]
mod tests {
    use crate::input::keymap::KeymapPreset;

    use super::build_help_lines;

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
        assert!(default_text.contains("Open help"));

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
}
