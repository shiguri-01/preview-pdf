use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::input::sequence::SequenceRegistrySnapshot;
use crate::input::shortcut::{format_shortcut_key, format_shortcut_sequence};

use super::layout::centered_rect;
use super::{border, heading_text, primary_text, secondary_text};

pub fn draw_help_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    scroll_offset: usize,
    keymap: &SequenceRegistrySnapshot,
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

    let lines = build_help_lines(keymap);
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
    sources: &'static [HelpKeySource],
    description: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum HelpKeySource {
    ExactCommand(&'static str),
    NumericCommand(&'static str),
}

const DEFAULT_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        rows: &[
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("next-page")],
                description: "Next page",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("prev-page")],
                description: "Previous page",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("first-page")],
                description: "First page",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("last-page")],
                description: "Last page",
            },
            HelpRow {
                sources: &[HelpKeySource::NumericCommand("goto-page")],
                description: "Go to page (`42G`)",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("history-back")],
                description: "History back",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("history-forward")],
                description: "History forward",
            },
        ],
    },
    HelpSection {
        title: "View",
        rows: &[
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("zoom-in")],
                description: "Zoom in",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("zoom-out")],
                description: "Zoom out",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("zoom-reset")],
                description: "Reset zoom",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("pan")],
                description: "Pan",
            },
        ],
    },
    HelpSection {
        title: "Search",
        rows: &[
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("search")],
                description: "Search",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("next-search-hit")],
                description: "Next search hit",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("prev-search-hit")],
                description: "Previous search hit",
            },
        ],
    },
    HelpSection {
        title: "Other",
        rows: &[
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("open-palette")],
                description: "Command palette",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("help")],
                description: "Help",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("quit")],
                description: "Quit",
            },
            HelpRow {
                sources: &[HelpKeySource::ExactCommand("cancel")],
                description: "Cancel / Close",
            },
        ],
    },
];

fn build_help_lines(keymap: &SequenceRegistrySnapshot) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for (i, section) in DEFAULT_SECTIONS.iter().enumerate() {
        let rows = section
            .rows
            .iter()
            .filter_map(|row| render_help_row(row, keymap))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }

        if !lines.is_empty() && i < DEFAULT_SECTIONS.len() {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![Span::styled(
            section.title,
            heading_text(),
        )]));
        lines.extend(rows);
    }

    lines
}

fn render_help_row(row: &HelpRow, keymap: &SequenceRegistrySnapshot) -> Option<Line<'static>> {
    let mut labels = Vec::new();
    for source in row.sources {
        match source {
            HelpKeySource::ExactCommand(command_id) => {
                for binding in keymap
                    .exact_bindings
                    .iter()
                    .filter(|binding| binding.command_id == *command_id)
                {
                    push_unique_label(&mut labels, format_shortcut_sequence(&binding.keys));
                }
            }
            HelpKeySource::NumericCommand(command_id) => {
                for binding in keymap
                    .numeric_prefix_bindings
                    .iter()
                    .filter(|binding| binding.command_id == *command_id)
                {
                    push_unique_label(
                        &mut labels,
                        format!("[count]{}", format_shortcut_key(binding.suffix)),
                    );
                }
            }
        }
    }

    if labels.is_empty() {
        return None;
    }

    let key_text = labels.join(" / ");
    let key_span = Span::styled(format!("{key_text:<18}"), secondary_text());
    Some(Line::from(vec![
        key_span,
        Span::raw(row.description.to_string()),
    ]))
}

fn push_unique_label(labels: &mut Vec<String>, candidate: String) {
    if !labels.iter().any(|label| label == &candidate) {
        labels.push(candidate);
    }
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
    use super::{build_help_lines, help_rendered_height};
    use crate::command::{Command, PanAmount, PanDirection};
    use crate::input::keymap::build_builtin_sequence_registry;
    use crate::input::sequence::SequenceRegistry;
    use crate::input::shortcut::ShortcutKey;

    #[test]
    fn help_lines_include_runtime_bindings() {
        let keymap = build_builtin_sequence_registry().snapshot();
        let text = build_help_lines(&keymap)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("<c-o>"));
        assert!(text.contains("Help"));
        assert!(text.contains("Reset zoom"));
        assert!(text.contains("gg"));
        assert!(text.contains("[count]G"));
        assert!(text.contains("H / J / K / L"));
        assert!(!text.contains("<c-n>"));
        assert!(!text.contains("<m-x>"));
        assert!(!text.contains("<pgdn>"));
    }

    #[test]
    fn help_lines_reflect_runtime_registry_changes() {
        let mut registry = SequenceRegistry::new();
        registry
            .register_static(&[ShortcutKey::char('x')], Command::NextPage)
            .expect("next-page binding should register");
        registry
            .register_static(&[ShortcutKey::char('y')], Command::PrevPage)
            .expect("prev-page binding should register");
        registry
            .register_static(
                &[ShortcutKey::char('A')],
                Command::Pan {
                    direction: PanDirection::Left,
                    amount: PanAmount::DefaultStep,
                },
            )
            .expect("pan left should register");
        registry
            .register_static(
                &[ShortcutKey::char('S')],
                Command::Pan {
                    direction: PanDirection::Down,
                    amount: PanAmount::DefaultStep,
                },
            )
            .expect("pan down should register");
        registry
            .register_static(
                &[ShortcutKey::char('W')],
                Command::Pan {
                    direction: PanDirection::Up,
                    amount: PanAmount::DefaultStep,
                },
            )
            .expect("pan up should register");
        registry
            .register_static(
                &[ShortcutKey::char('D')],
                Command::Pan {
                    direction: PanDirection::Right,
                    amount: PanAmount::DefaultStep,
                },
            )
            .expect("pan right should register");

        let text = build_help_lines(&registry.snapshot())
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("x"));
        assert!(text.contains("y"));
        assert!(text.contains("A / S / W / D"));
        assert!(!text.contains("j"));
        assert!(!text.contains("H / J / K / L"));
    }

    #[test]
    fn help_scroll_limit_accounts_for_wrapping() {
        let keymap = build_builtin_sequence_registry().snapshot();
        let raw_limit = build_help_lines(&keymap).len().saturating_sub(5);
        let wrapped_limit = help_rendered_height(&build_help_lines(&keymap), 20, 5);

        assert!(wrapped_limit > raw_limit);
        assert!(wrapped_limit > 0);
    }
}
